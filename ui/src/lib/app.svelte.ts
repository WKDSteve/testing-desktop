// The page on the hub: the session with it and its event stream. What it reads is `Reader`'s. Everything the page reaches
// outside (the hub, the storage of the tab, the clock) comes in through `Env`, so the unit tests run it without a browser.
// What is new comes by the event stream of the hub: nothing asks on a timer.

import { StreamSession } from 'appkit-ui/session';
import type { SseMessage } from 'appkit-ui/sse';
import { Reader } from './reader.svelte';
import { ApiError, type HubSource } from './source';

export type { Phase } from './reader.svelte';

/** Where the session token of the tab is kept: the tab only (`sessionStorage`), never longer. */
export const TOKEN_KEY = 'testing-desktop.session';

/** How long the page waits before it asks a hub that did not answer, or an engine that was not ready, again. */
export const RETRY_MS = 3000;

/** How often a pairing code is tried again when the hub says that there were too many attempts. */
export const EXCHANGE_RETRIES = 3;

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export interface Env {
  hub: HubSource;
  session: StorageLike;
  sleep: (ms: number) => Promise<void>;
}

/** A storage that throws (a browser that forbids it) is a storage that holds nothing. */
function safe<T>(fn: () => T): T | null {
  try {
    return fn();
  } catch {
    return null;
  }
}

export class App extends Reader {
  /** Why the engine is not ready, as the hub says it (a short code). */
  problem = $state<string | null>(null);
  /** The pairing code of the address was refused (used or too old). */
  pairRefused = $state(false);
  /** The hub took no more pairing attempts for now; the code was not looked at. */
  pairLimited = $state(false);

  /** The event stream of the hub (the kit's `StreamSession`). */
  readonly stream = new StreamSession();

  private readonly env: Env;

  constructor(env: Env) {
    super(env.hub);
    this.env = env;
  }

  protected override get live(): boolean {
    return !this.stopped && this.env.hub.hasToken;
  }

  /** Bring the page up. `code` is a pairing code taken from the address. */
  async start(code: string | null): Promise<void> {
    this.stopped = false;
    this.stream.resume();
    this.phase = 'starting';
    this.paletteOpen = false;
    this.pairRefused = false;
    this.pairLimited = false;
    let token = safe(() => this.env.session.getItem(TOKEN_KEY));
    if (code) {
      for (let attempt = 0; ; attempt++) {
        try {
          const traded = await this.env.hub.exchange(code);
          token = traded;
          safe(() => this.env.session.setItem(TOKEN_KEY, traded));
          break;
        } catch (error) {
          // A code that is spent or too old: a session the tab has is still good.
          if (error instanceof ApiError && error.status === 401) {
            this.pairRefused = token === null;
            break;
          }
          // Too many attempts: the hub did not look at the code, which is still good but no longer in the address.
          const limited = error instanceof ApiError && error.status === 429;
          if (limited && attempt < EXCHANGE_RETRIES) {
            await this.env.sleep(RETRY_MS);
            if (this.stopped) return;
            continue;
          }
          if (token !== null) break;
          if (limited) {
            this.pairLimited = true;
            this.phase = 'needs_pairing';
          } else {
            this.phase = 'offline';
          }
          return;
        }
      }
    }
    if (token === null) {
      this.phase = 'needs_pairing';
      return;
    }
    this.env.hub.setToken(token);
    this.env.hub.onUnauthorized = () => this.lostSession();

    for (;;) {
      try {
        const hello = await this.env.hub.hello();
        if (hello.engine.ready) break;
        this.problem = hello.engine.problem ?? 'unknown';
        this.phase = 'unavailable';
      } catch {
        this.phase = 'offline';
      }
      await this.env.sleep(RETRY_MS);
      if (this.stopped) return;
    }
    this.problem = null;
    await this.refresh();
    if (this.stopped || !this.env.hub.hasToken) return;
    void this.readAbout();
    // It runs for as long as the page: `start` does not wait for it. A start after a stop (a pairing link pasted into
    // the tab) finds the one that ran still going: the kit keeps it and it reads on with the new token.
    void this.followStream();
  }

  /** The words of the help menu. A hub that cannot say leaves the menu without them. */
  private async readAbout(): Promise<void> {
    try {
      const about = await this.env.hub.about();
      if (!this.stopped && this.env.hub.hasToken) this.about = about;
    } catch {
      // Nothing to show: the menu says what it knows.
    }
  }

  stop(): void {
    this.stopped = true;
    this.stream.stop();
    this.repoAbort?.abort();
  }

  private followStream(): Promise<void> {
    return this.stream.follow({
      fetchFn: this.env.hub.fetcher,
      url: () => this.env.hub.eventsUrl,
      bearer: () => this.env.hub.bearer,
      hasToken: () => this.env.hub.hasToken,
      sleep: (ms) => this.env.sleep(ms),
      // What happened while the stream was down shows in the `hello` that follows: nothing to read yet.
      onOpen: () => {},
      onMessage: (m) => void this.onMessage(m),
      onLost: () => this.lostSession(),
    });
  }

  /** One event of the stream. Returns when what it asked for is read (the tests wait for it). */
  async onMessage(message: SseMessage): Promise<void> {
    if (message.event === 'fleet' || message.event === 'resync') {
      // An event of this hub names no repository: it says that the fleet changed.
      await this.readAgain();
      return;
    }
    if (message.event === 'hello') {
      // Where the stream stands: another revision than ours came between our read and the stream.
      try {
        const doc = JSON.parse(message.data) as { rev?: unknown };
        if (typeof doc.rev !== 'string' || doc.rev === this.fleet?.rev) return;
      } catch {
        // Not readable: read to be sure.
      }
      await this.readAgain();
    }
  }

  private lostSession(): void {
    safe(() => this.env.session.removeItem(TOKEN_KEY));
    this.env.hub.setToken(null);
    this.stream.cut();
    this.repoAbort?.abort();
    this.phase = 'needs_pairing';
    this.paletteOpen = false;
    this.about = null;
    this.fleet = null;
    this.etag = null;
    this.close();
  }
}
