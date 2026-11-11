// The page on the hub: the session with it and its event stream. What it reads is `Reader`'s. Everything the page reaches
// outside (the hub, the storage of the tab, the clock) comes in through `Env`, so the unit tests run it without a browser.
// What is new comes by the event stream of the hub: nothing asks on a timer.

import { StreamSession } from 'appkit-ui/session';
import type { SseMessage } from 'appkit-ui/sse';
import { messageOf, Reader } from './reader.svelte';
import { ApiError, type HubSource, type StepCommand } from './source';
import { emptyStep, fromDocument, fromStream, isStale, noteProblem, STREAM_EVENT, type StepState } from './step';

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

  /** The stepped run of a sheet, as the hub holds it (`lib/step.ts`); the stream keeps it current. */
  step = $state<StepState>(emptyStep());
  /** Why the last start or command of the run was refused; `null` once one went through. */
  stepFailure = $state<string | null>(null);
  /** The step whose tile is shown, chosen in the list; `null`: the last one that did something. */
  stepChosen = $state<number | null>(null);

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
    // The hub answers and has an engine: what is left is the first read of the fleet, which asks GitHub and can take
    // most of a minute when the hub has only just started. The page says so instead of "connecting".
    this.phase = 'reading';
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
    if (message.event === STREAM_EVENT) {
      const next = fromStream(this.step, message.data);
      // A line of a run the page does not hold: its state is read whole.
      if (isStale(next)) await this.readStep();
      else this.step = next;
      return;
    }
    if (message.event === 'fleet' || message.event === 'resync') {
      // An event of this hub names no repository: it says that the fleet changed.
      await this.readAgain();
      // Too slow for the lines of a run: its state is read whole.
      if (message.event === 'resync') await this.readStep();
      return;
    }
    if (message.event === 'hello') {
      // The stream was down for a while: what a run did meanwhile is in its state.
      if (this.step.session !== null) await this.readStep();
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

  /** The state of the run as the hub holds it; a state older than what the stream brought is not taken. */
  private takeStep(doc: unknown): void {
    const next = fromDocument(doc);
    if (next.session === this.step.session && next.seq < this.step.seq) return;
    // The breakpoints of this page hold while it follows the run; a page that comes later reads them from the hub.
    if (next.session === this.step.session) next.breaks = this.step.breaks;
    this.step = next;
  }

  /** Read where the run stands. A failure leaves what the page holds. */
  async readStep(): Promise<void> {
    try {
      const doc = await this.env.hub.stepState();
      if (this.live) this.takeStep(doc);
    } catch (error) {
      if (this.live) this.stepFailure = messageOf(error);
    }
  }

  override showStep(): void {
    this.stepView = true;
    this.paletteOpen = false;
    if (this.step.session === null) void this.readStep();
  }

  /** Run a sheet of the open repository step by step: the page shows the run as it starts. */
  override async stepSheet(name: string): Promise<void> {
    const key = this.openKey;
    if (key === null) return;
    this.stepView = true;
    this.stepFailure = null;
    this.stepChosen = null;
    try {
      const doc = await this.env.hub.stepStart(key, name);
      if (this.live) this.takeStep(doc);
    } catch (error) {
      if (this.live) this.stepFailure = messageOf(error);
    }
  }

  /** One command to the run; what the hub answers is where the run stands now. */
  async stepCommand(command: StepCommand): Promise<void> {
    this.stepFailure = null;
    try {
      const doc = await this.env.hub.stepCommand(command);
      if (this.live) this.takeStep(doc);
    } catch (error) {
      if (this.live) this.stepFailure = messageOf(error);
    }
  }

  /** Run this step; to a question this is a no. */
  stepNext(): Promise<void> {
    if (this.step.question) return this.stepCommand({ command: 'answer', yes: false });
    return this.step.paused === null ? Promise.resolve() : this.stepCommand({ command: 'next' });
  }

  /** Yes to the question; nothing without one. */
  stepYes(): Promise<void> {
    return this.step.question ? this.stepCommand({ command: 'answer', yes: true }) : Promise.resolve();
  }

  /** The answer to the question, with a note. */
  stepAnswer(yes: boolean, text: string): Promise<void> {
    const note = text.trim();
    return this.stepCommand(note ? { command: 'answer', yes, text: note } : { command: 'answer', yes });
  }

  /**
   * A note on the step where the run stands, at any time while it runs (testing.nvim takes it at once). An empty note
   * or one over `NOTE_MAX_BYTES` (`noteProblem`) is not sent: false, so that the page keeps what was typed.
   */
  stepNote(text: string): Promise<boolean> {
    if (!this.step.running || noteProblem(text) !== null) return Promise.resolve(false);
    return this.stepCommand({ command: 'note', text: text.trim() }).then(() => this.stepFailure === null);
  }

  /** Run on from a stop; nothing while a step runs or a question waits. */
  stepContinue(): Promise<void> {
    if (this.step.paused === null || this.step.question) return Promise.resolve();
    return this.stepCommand({ command: 'continue' });
  }

  /** Run on up to the chosen step; nothing when it is not a later one. */
  stepUntil(): Promise<void> {
    const n = this.stepChosen;
    if (n === null || n <= (this.step.at ?? 0) || this.step.paused === null || this.step.question) return Promise.resolve();
    return this.stepCommand({ command: 'until', n });
  }

  /** A breakpoint on the step, or none: the page keeps it, and the run holds it in every size. */
  stepToggleBreak(n: number | null = this.stepChosen): Promise<void> {
    if (n === null) return Promise.resolve();
    const set = !this.step.breaks.includes(n);
    this.step = { ...this.step, breaks: set ? [...this.step.breaks, n].sort((a, b) => a - b) : this.step.breaks.filter((b) => b !== n) };
    return this.stepCommand({ command: set ? 'break' : 'clear', n });
  }

  stepAbort(): Promise<void> {
    return this.stepCommand({ command: 'abort' });
  }

  /** The picture of a step of the run, for the tile. */
  stepImage(size: string, n: number, kind: 'image' | 'shot', signal?: AbortSignal): Promise<Blob> {
    return this.env.hub.stepImage(size, n, kind, signal);
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
    this.step = emptyStep();
    this.stepFailure = null;
    this.close();
  }
}
