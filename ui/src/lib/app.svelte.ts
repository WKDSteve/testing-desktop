// The state of the page: the session with the hub, the fleet, the repository that is open. Everything the page
// reaches outside (the hub, the storage of the tab, the clock) comes in through `Env`, so the unit tests run it
// without a browser. Reads are single-flight: a read that is under way is not asked again, an older answer never
// replaces a newer one. What is new comes by the event stream of the hub: nothing asks on a timer.

import { StreamSession } from 'appkit-ui/session';
import type { SseMessage } from 'appkit-ui/sse';
import type { Fleet, RepoDoc } from './contract';
import { findingsOf, type Finding } from './model';
import type { PaletteItem } from 'appkit-ui/palette';
import { ApiError, type About, type HubSource } from './source';

/** Where the session token of the tab is kept: the tab only (`sessionStorage`), never longer. */
export const TOKEN_KEY = 'testing-desktop.session';

/** How long the page waits before it asks a hub that did not answer, or an engine that was not ready, again. */
export const RETRY_MS = 3000;

/** How often a pairing code is tried again when the hub says that there were too many attempts. */
export const EXCHANGE_RETRIES = 3;

export type Phase = 'starting' | 'needs_pairing' | 'offline' | 'unavailable' | 'ready' | 'failed';

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

/** The short word of a failure, never more of its text than its message. */
function messageOf(error: unknown): string {
  if (error instanceof ApiError) return `${error.code}: ${error.message}`;
  return error instanceof Error ? error.message : String(error);
}

export class App {
  phase = $state<Phase>('starting');
  /** Why the engine is not ready, as the hub says it (a short code). */
  problem = $state<string | null>(null);
  /** The pairing code of the address was refused (used or too old). */
  pairRefused = $state(false);
  /** The hub took no more pairing attempts for now; the code was not looked at. */
  pairLimited = $state(false);
  failure = $state<string | null>(null);

  fleet = $state<Fleet | null>(null);
  private etag: string | null = null;
  onlyRed = $state(false);
  query = $state('');

  /** The repository that is open, by its key; its document once it is read. */
  openKey = $state<string | null>(null);
  repo = $state<RepoDoc | null>(null);
  repoFailure = $state<string | null>(null);
  /** The findings of the open repository (`null` until read); a failure of this read never takes the repository. */
  findings = $state<Finding[] | null>(null);
  findingsFailure = $state<string | null>(null);

  /** What this build and its engine are (the help menu); `null` until read, and when the hub does not say. */
  about = $state<About | null>(null);
  paletteOpen = $state(false);

  /** The event stream of the hub (the kit's `StreamSession`). */
  readonly stream = new StreamSession();

  private stopped = false;
  private reading: Promise<void> | null = null;
  private repoAbort: AbortController | null = null;

  constructor(private readonly env: Env) {}

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

  openPalette(): void {
    this.paletteOpen = true;
  }

  closePalette(): void {
    this.paletteOpen = false;
  }

  /** The choice in the palette: an action runs, a repository opens. */
  async choose(item: PaletteItem): Promise<void> {
    this.paletteOpen = false;
    if (item.kind === 'action') await item.action.run();
    else await this.open(item.id);
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

  /** The fleet, and the repository that is open, which stays on the page while it is read. */
  private async readAgain(): Promise<void> {
    // A read under way may have been answered before this change: it is waited for, not joined.
    if (this.reading) await this.reading;
    await this.refresh();
    const key = this.openKey;
    if (key === null || this.stopped) return;
    const abort = new AbortController();
    this.repoAbort?.abort();
    this.repoAbort = abort;
    try {
      const doc = await this.env.hub.repo(key, abort.signal);
      if (abort.signal.aborted || this.openKey !== key) return;
      this.repo = doc;
      this.repoFailure = null;
    } catch (error) {
      if (abort.signal.aborted || this.openKey !== key) return;
      // What was read stays: a failure of this read only replaces a repository the page does not hold yet.
      if (this.repo === null) this.repoFailure = messageOf(error);
    }
    await this.readFindings(key, abort.signal);
  }

  /** The findings of the repository `key`, which is open; an answer for another one, or an aborted read, is dropped. */
  private async readFindings(key: string, signal: AbortSignal): Promise<void> {
    try {
      const doc = await this.env.hub.finding(key, signal);
      if (signal.aborted || this.openKey !== key) return;
      this.findings = findingsOf(doc);
      this.findingsFailure = null;
    } catch (error) {
      if (signal.aborted || this.openKey !== key) return;
      // What was read stays, as with the repository.
      if (this.findings === null) this.findingsFailure = messageOf(error);
    }
  }

  /** What the person asks for: the fleet, and the repository that is open with its findings. */
  reload(): Promise<void> {
    return this.readAgain();
  }

  /** Read the fleet again; a read under way is joined, not doubled. */
  refresh(): Promise<void> {
    this.reading ??= this.readFleet().finally(() => {
      this.reading = null;
    });
    return this.reading;
  }

  private async readFleet(): Promise<void> {
    try {
      const result = await this.env.hub.fleet(this.etag);
      if (this.stopped || !this.env.hub.hasToken) return;
      if (result.kind === 'fresh') {
        this.fleet = result.doc;
        this.etag = result.etag;
      }
      this.failure = null;
      this.phase = 'ready';
    } catch (error) {
      if (this.stopped || !this.env.hub.hasToken) return;
      this.failure = messageOf(error);
      // A fleet that was read stays on the page; without one the page says why there is none.
      if (this.fleet === null) this.phase = 'failed';
    }
  }

  /** Open a repository by its key of the fleet; an older read of another one is dropped. */
  async open(key: string): Promise<void> {
    this.repoAbort?.abort();
    const abort = new AbortController();
    this.repoAbort = abort;
    this.openKey = key;
    this.repo = null;
    this.repoFailure = null;
    this.findings = null;
    this.findingsFailure = null;
    const findings = this.readFindings(key, abort.signal);
    try {
      const doc = await this.env.hub.repo(key, abort.signal);
      if (abort.signal.aborted || this.openKey !== key) return;
      this.repo = doc;
    } catch (error) {
      if (abort.signal.aborted || this.openKey !== key) return;
      this.repoFailure = messageOf(error);
    }
    await findings;
  }

  close(): void {
    this.repoAbort?.abort();
    this.openKey = null;
    this.repo = null;
    this.repoFailure = null;
    this.findings = null;
    this.findingsFailure = null;
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
