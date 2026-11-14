// The page on the hub: the session with it and its event stream. What it reads is `Reader`'s. Everything the page reaches
// outside (the hub, the storage of the tab, the clock) comes in through `Env`, so the unit tests run it without a browser.
// What is new comes by the event stream of the hub: nothing asks on a timer.

import { StreamSession } from 'appkit-ui/session';
import type { SseMessage } from 'appkit-ui/sse';
import { messageOf, Reader } from './reader.svelte';
import { ApiError, type HubSource, type StepCommand } from './source';
import { commitsFrom, emptyWizard, isFullSha, outcomeFrom, reposFrom, reviewFrom, type Wizard } from './release';
import { fileTextProblem, runFrom, type RunView } from './run';
import { emptyStep, fromDocument, fromStream, isStale, noteProblem, STREAM_EVENT, type StepState } from './step';

export type { Phase } from './reader.svelte';

/** Where the session token of the tab is kept: the tab only (`sessionStorage`), never longer. */
export const TOKEN_KEY = 'testing-desktop.session';

/** How long the page waits before it asks a hub that did not answer, or an engine that was not ready, again. */
export const RETRY_MS = 3000;

/** How often the page asks where a run through the engine stands while it runs (the hub streams no run). */
export const RUN_POLL_MS = 2000;

/** How often the page asks what became of a release while it runs (the hub answers its start at once). */
export const RELEASE_POLL_MS = 1000;

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

  /** The run through the engine that is or was last (`lib/run.ts`); `null` before the first. */
  run = $state<RunView | null>(null);
  /** Why the last start of a run was refused; `null` once one went through. */
  runFailure = $state<string | null>(null);
  private following = false;

  /** The release assistant for the runner guard (`lib/release.ts`): where a person stands in its steps. */
  release = $state<Wizard>(emptyWizard());
  // The step asked last: an answer to an earlier one (the person went on or back meanwhile) is dropped.
  private releaseAsked = 0;

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
    this.releaseView = false;
    this.paletteOpen = false;
    if (this.step.session === null) void this.readStep();
  }

  /** Run a sheet of the open repository step by step: the page shows the run as it starts. */
  override async stepSheet(name: string): Promise<void> {
    const key = this.openKey;
    if (key === null) return;
    this.releaseView = false;
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

  /** Open a repository; the page also learns of a run through the engine under way, and follows it. */
  override async open(key: string): Promise<void> {
    // Quietly: a hub without the route, or one that does not answer it now, leaves the page as it is.
    const read = (this.hasStep ? this.readRun(true) : Promise.resolve()).then(() => {
      if (this.run?.running) void this.followRun();
    });
    await super.open(key);
    await read;
  }

  /** Read where the run through the engine stands. A failure leaves what the page holds (and says so, unless quiet). */
  async readRun(quiet = false): Promise<void> {
    try {
      const doc = await this.env.hub.runState();
      if (this.live) this.run = runFrom(doc).run;
    } catch (error) {
      if (this.live && !quiet) this.runFailure = messageOf(error);
    }
  }

  /**
   * Run the suite of the open repository, or its spec files whose name holds `file`, through the engine; the page
   * follows the run until it ends and then reads the repository again. A file text the hub would refuse is not sent.
   */
  async runStart(file?: string): Promise<void> {
    const key = this.openKey;
    if (key === null) return;
    const text = file?.trim();
    if (text !== undefined && fileTextProblem(text) !== null) return;
    this.runFailure = null;
    try {
      const doc = await this.env.hub.runStart(key, text);
      if (!this.live) return;
      this.run = runFrom(doc).run;
      void this.followRun();
    } catch (error) {
      if (this.live) this.runFailure = messageOf(error);
    }
  }

  /** Ask where the run stands until it ended; then the repository is read again. One follower at a time. */
  private async followRun(): Promise<void> {
    if (this.following) return;
    this.following = true;
    try {
      while (this.live && this.run?.running) {
        await this.env.sleep(RUN_POLL_MS);
        if (!this.live) return;
        await this.readRun();
      }
      if (this.live) await this.readAgain();
    } finally {
      this.following = false;
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

  /**
   * Open the release assistant at its first step; with `repo` (the repository of a red CI run) at its commits, when
   * the assistant offers that repository.
   */
  override showRelease(repo?: string): void {
    this.releaseView = true;
    this.stepView = false;
    this.paletteOpen = false;
    this.release = emptyWizard();
    void this.releaseStart(repo);
  }

  /** One step of the assistant: `ask` asks the hub, `take` folds a good answer into the state. */
  private async releaseStep(ask: () => Promise<unknown>, take: (doc: unknown) => void): Promise<void> {
    const mine = ++this.releaseAsked;
    this.release = { ...this.release, busy: true, failure: null };
    try {
      const doc = await ask();
      if (!this.live || mine !== this.releaseAsked) return;
      this.release = { ...this.release, busy: false };
      take(doc);
    } catch (error) {
      if (!this.live || mine !== this.releaseAsked) return;
      this.release = { ...this.release, busy: false, failure: messageOf(error) };
    }
  }

  /** What the engine says when a step did not go through; `null` when it did. */
  private static releaseProblem(read: { exit: number | null; problem: string | null }): string | null {
    return read.exit === 0 ? null : (read.problem ?? `exit ${read.exit ?? '?'}`);
  }

  private async releaseStart(repo?: string): Promise<void> {
    await this.releaseStep(
      () => this.env.hub.releaseRepos(),
      (doc) => {
        const read = reposFrom(doc);
        this.release = { ...this.release, repos: read.repos, failure: App.releaseProblem(read) };
      },
    );
    const wanted = repo?.toLowerCase();
    const offered = wanted ? this.release.repos?.find((r) => r.name.toLowerCase() === wanted) : undefined;
    if (offered && this.release.stage === 'repo') await this.releaseChooseRepo(offered.name);
  }

  /** Step 1 is done: this repository. Its commits are read. */
  async releaseChooseRepo(name: string): Promise<void> {
    if (this.release.busy || !this.release.repos?.some((r) => r.name === name)) return;
    this.release = { ...this.release, stage: 'commit', repo: name, commits: null, sha: null, review: null, looked: false, done: null, doneProblem: null };
    await this.releaseStep(
      () => this.env.hub.releaseCommits(name),
      (doc) => {
        const read = commitsFrom(doc);
        this.release = { ...this.release, commits: read.commits, failure: App.releaseProblem(read) };
      },
    );
  }

  /** Step 2 is done: this commit, by its full id. Its review aid is read; only what the hub answers for it counts. */
  async releaseChooseCommit(sha: string): Promise<void> {
    const repo = this.release.repo;
    if (this.release.busy || repo === null || !isFullSha(sha)) return;
    this.release = { ...this.release, stage: 'review', sha, review: null, looked: false, done: null, doneProblem: null };
    await this.releaseStep(
      () => this.env.hub.releaseShow(repo, sha),
      (doc) => {
        const read = reviewFrom(doc);
        const review = read.review?.sha === sha ? read.review : null;
        this.release = { ...this.release, review, failure: review ? null : (App.releaseProblem(read) ?? 'the hub gave no review aid for this commit') };
      },
    );
  }

  /** The person says whether they looked at what the commit changes. */
  releaseLooked(looked: boolean): void {
    if (this.release.stage === 'review' && this.release.review !== null) this.release = { ...this.release, looked };
  }

  /** Step 3 is done: on to the question, only with a review aid that was looked at. */
  releaseToConfirm(): void {
    const w = this.release;
    if (w.busy || w.stage !== 'review' || w.review === null || !w.looked) return;
    this.release = { ...w, stage: 'confirm', failure: null };
  }

  /** One step back; what was chosen in the step that is left is forgotten. */
  releaseBack(): void {
    const w = this.release;
    if (w.busy) return;
    if (w.stage === 'commit') this.release = { ...w, stage: 'repo', repo: null, commits: null, failure: null };
    else if (w.stage === 'review') this.release = { ...w, stage: 'commit', sha: null, review: null, looked: false, failure: null };
    else if (w.stage === 'confirm') this.release = { ...w, stage: 'review', failure: null };
  }

  /**
   * The release: the commit of the review aid on the page, after the question. The hub hands it to the engine only
   * when it handed out that aid last; what the engine did is the last step.
   */
  async releaseDo(): Promise<void> {
    const w = this.release;
    if (w.busy || w.stage !== 'confirm' || w.repo === null || w.sha === null || w.review === null || w.review.sha !== w.sha || !w.looked) return;
    const { repo, sha } = w;
    await this.releaseStep(
      async () => {
        await this.env.hub.releaseCommit(repo, sha);
        // The hub started it and answers at once; the release runs on there, whatever becomes of this page.
        for (;;) {
          const read = outcomeFrom(await this.env.hub.releaseOutcome());
          if (read.state !== 'pending') return read;
          await this.env.sleep(RELEASE_POLL_MS);
          if (!this.live) return read;
        }
      },
      (answer) => {
        const read = answer as ReturnType<typeof outcomeFrom>;
        // Only the outcome of the release this page started: another one's (a second tab) is not this one's result.
        const mine = read.repo?.toLowerCase() === repo.toLowerCase() && read.sha === sha;
        if (read.state === 'done' && mine) {
          this.release = { ...this.release, stage: 'done', done: read.done, doneProblem: App.releaseProblem(read) };
        } else {
          const why = read.state === 'failed' && mine ? read.error : 'the hub does not say what became of this release';
          this.release = { ...this.release, stage: 'done', done: null, doneProblem: why };
        }
      },
    );
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
    this.run = null;
    this.runFailure = null;
    this.release = emptyWizard();
    this.releaseAsked += 1;
    this.close();
  }
}
