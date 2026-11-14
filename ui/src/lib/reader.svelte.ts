// What the page reads, whatever it reads from: the fleet, the repository that is open with its findings, the filter and
// the palette. The views know this and nothing of a transport. `App` (app.svelte.ts) reads the hub with a session and an
// event stream; `WebReader` (web/web-reader.svelte.ts) reads a folder. Reads are single-flight: a read that is under way
// is not asked again, an older answer never replaces a newer one.

import type { PaletteItem } from 'appkit-ui/palette';
import type { Fleet, RepoDoc } from './contract';
import { findingsOf, type Finding } from './model';
import { ApiError, type About, type TestingSource } from './source';

export type Phase = 'starting' | 'reading' | 'needs_pairing' | 'offline' | 'unavailable' | 'ready' | 'failed';

/** The short word of a failure, never more of its text than its message. */
export function messageOf(error: unknown): string {
  if (error instanceof ApiError) return `${error.code}: ${error.message}`;
  return error instanceof Error ? error.message : String(error);
}

export class Reader {
  phase = $state<Phase>('starting');
  failure = $state<string | null>(null);

  fleet = $state<Fleet | null>(null);
  protected etag: string | null = null;
  onlyRed = $state(false);
  query = $state('');

  /** The repository that is open, by its key; its document once it is read. */
  openKey = $state<string | null>(null);
  repo = $state<RepoDoc | null>(null);
  repoFailure = $state<string | null>(null);
  /** The findings of the open repository (`null` until read); a failure of this read never takes the repository. */
  findings = $state<Finding[] | null>(null);
  findingsFailure = $state<string | null>(null);
  /** The test sheets of the open repository, by name (`null` until read); only a source with the capability `step`. */
  sheets = $state<string[] | null>(null);
  sheetsFailure = $state<string | null>(null);
  /** The page shows the stepped run instead of the fleet or a repository (the hub only). */
  stepView = $state(false);
  /** The page shows the release assistant for the runner guard instead (the hub only). */
  releaseView = $state(false);

  /** What this build and its engine are (the help menu); `null` until read, and when the source does not say. */
  about = $state<About | null>(null);
  paletteOpen = $state(false);

  protected stopped = false;
  protected reading: Promise<void> | null = null;
  protected repoAbort: AbortController | null = null;

  constructor(protected readonly source: TestingSource) {}

  /** Whether this source has findings at all: without, the views leave them out. */
  get hasFindings(): boolean {
    return this.source.capabilities.findings;
  }

  /** Whether this source can run a test sheet step by step: without, the views offer none. */
  get hasStep(): boolean {
    return this.source.capabilities.step;
  }

  /** Show the stepped run (`App` has one; a folder has none, so this does nothing). */
  showStep(): void {}

  /** Leave the stepped run for the fleet or the open repository; the run goes on. */
  hideStep(): void {
    this.stepView = false;
  }

  /** Show the release assistant for the runner guard (`App` has one; a folder has none, so this does nothing). */
  showRelease(_repo?: string): void {}

  /** Leave the release assistant for the fleet or the open repository. */
  hideRelease(): void {
    this.releaseView = false;
  }

  /** Run a sheet of the open repository step by step (`App`); a folder cannot. */
  stepSheet(_name: string): Promise<void> {
    return Promise.resolve();
  }

  /** Whether an answer that comes in now still counts. */
  protected get live(): boolean {
    return !this.stopped;
  }

  /** The fleet, and the repository that is open, which stays on the page while it is read. */
  protected async readAgain(): Promise<void> {
    // A read under way may have been answered before this change: it is waited for, not joined.
    if (this.reading) await this.reading;
    await this.refresh();
    const key = this.openKey;
    if (key === null || this.stopped) return;
    const abort = new AbortController();
    this.repoAbort?.abort();
    this.repoAbort = abort;
    try {
      const doc = await this.source.repo(key, abort.signal);
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

  /** The sheets of the repository `key`, which is open; an answer for another one, or an aborted read, is dropped. */
  private async readSheets(key: string, signal: AbortSignal): Promise<void> {
    if (!this.source.sheets) return;
    try {
      const names = await this.source.sheets(key, signal);
      if (signal.aborted || this.openKey !== key) return;
      this.sheets = names;
      this.sheetsFailure = null;
    } catch (error) {
      if (signal.aborted || this.openKey !== key) return;
      if (this.sheets === null) this.sheetsFailure = messageOf(error);
    }
  }

  /** The findings of the repository `key`, which is open; an answer for another one, or an aborted read, is dropped. */
  private async readFindings(key: string, signal: AbortSignal): Promise<void> {
    if (!this.source.finding) return;
    try {
      const doc = await this.source.finding(key, signal);
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
      const result = await this.source.fleet(this.etag);
      if (!this.live) return;
      if (result.kind === 'fresh') {
        this.fleet = result.doc;
        this.etag = result.etag;
      }
      this.failure = null;
      this.phase = 'ready';
    } catch (error) {
      if (!this.live) return;
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
    this.sheets = null;
    this.sheetsFailure = null;
    const findings = this.readFindings(key, abort.signal);
    const sheets = this.readSheets(key, abort.signal);
    try {
      const doc = await this.source.repo(key, abort.signal);
      if (abort.signal.aborted || this.openKey !== key) return;
      this.repo = doc;
    } catch (error) {
      if (abort.signal.aborted || this.openKey !== key) return;
      this.repoFailure = messageOf(error);
    }
    await findings;
    await sheets;
  }

  close(): void {
    this.repoAbort?.abort();
    this.openKey = null;
    this.repo = null;
    this.repoFailure = null;
    this.findings = null;
    this.findingsFailure = null;
    this.sheets = null;
    this.sheetsFailure = null;
    this.stepView = false;
    this.releaseView = false;
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
}
