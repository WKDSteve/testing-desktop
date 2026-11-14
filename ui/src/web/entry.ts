// The way in of the web variant: choose a folder or take one used before, get the permission back after a restart, read
// the export. A small state machine without a DOM; the page (WebEntry.svelte) draws its state and calls its methods from
// clicks. A click handler awaits nothing before it asks: the browser asks for a permission only inside a click.

import { oneLine, pickerSupport, type DirHandle, type DirHandleLike, type PickerWindow, type RecentFolders } from 'appkit-web/folder';
import { inputFolderSupport, pickFolderByInput, type InputDocument } from 'appkit-web/folder-input';
import { grantAccess, permissionOf, pickFolder } from './folder';
import { ExportError, type ExportProblem, type Snapshot } from './folder-source';
import { RecordingError, type Recording, type RecordingProblem } from './recording';

export type Access = 'granted' | 'prompt' | 'denied' | 'gone';

export interface RecentView {
  id: string;
  name: string;
  access: Access;
}

/** Why the last attempt did not lead into an export. `detail` is a path, a word of the contract or a line of the browser. */
export interface Notice {
  code: 'cancelled' | 'empty' | 'no_paths' | 'blocked' | 'failed' | 'denied' | 'gone' | ExportProblem | RecordingProblem;
  detail?: string;
}

export type EntryState =
  | { step: 'starting' }
  | { step: 'unsupported'; reason: 'missing' | 'insecure' }
  /** `readOnce`: this browser has no folder picker (Firefox, Safari), the folder comes through an input and cannot be looked at again. */
  | { step: 'choose'; recent: RecentView[]; notice: Notice | null; readOnce: boolean }
  | { step: 'opening'; name: string }
  | { step: 'open'; name: string; handle: DirHandleLike; snapshot: Snapshot; readOnce: boolean }
  /** The folder is the recording of a stepped run (`events.jsonl`): it is played back, read once. */
  | { step: 'replay'; name: string; recording: Recording };

export interface EntryDeps {
  win: PickerWindow;
  recent: RecentFolders;
  read(handle: DirHandleLike): Promise<Snapshot>;
  /** The recording of a stepped run in the folder; `null`: the folder is none (it is read as an export). */
  recording?(handle: DirHandleLike): Promise<Recording | null>;
  now(): number;
}

export class Entry {
  private readonly deps: EntryDeps;
  private current: EntryState = { step: 'starting' };
  private readonly listeners = new Set<(state: EntryState) => void>();
  private readOnce = false;
  // A dialog of the input is open: a second click must not open another one.
  private picking = false;

  constructor(deps: EntryDeps) {
    this.deps = deps;
  }

  get state(): EntryState {
    return this.current;
  }

  subscribe(listener: (state: EntryState) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private set(state: EntryState): void {
    this.current = state;
    for (const listener of [...this.listeners]) listener(state);
  }

  /** The first look: can this page choose a folder, and which ones were used before. */
  async start(notice: Notice | null = null): Promise<void> {
    const support = pickerSupport(this.deps.win);
    if (support === 'missing' && inputFolderSupport(this.deps.win.document)) {
      this.readOnce = true;
      this.set({ step: 'choose', recent: [], notice, readOnce: true });
      return;
    }
    this.readOnce = false;
    if (support !== 'ok') {
      this.set({ step: 'unsupported', reason: support });
      return;
    }
    const recent: RecentView[] = [];
    for (const entry of await this.deps.recent.list()) {
      recent.push({ id: entry.id, name: entry.name, access: await permissionOf(entry.handle) });
    }
    this.set({ step: 'choose', recent, notice, readOnce: false });
  }

  /** From the click on "Choose folder". */
  async pick(): Promise<void> {
    if (this.readOnce) {
      if (this.picking) return;
      this.picking = true;
      try {
        return await this.pickByInput();
      } finally {
        this.picking = false;
      }
    }
    const result = await pickFolder(this.deps.win);
    if (result.kind === 'cancelled') return this.start({ code: 'cancelled' });
    if (result.kind === 'blocked') return this.start({ code: 'blocked', detail: result.message });
    if (result.kind === 'failed') return this.start({ code: 'failed', detail: result.message });
    await this.enter(result.handle);
  }

  private async pickByInput(): Promise<void> {
    const result = await pickFolderByInput(this.deps.win.document as InputDocument);
    if (result.kind === 'cancelled') return this.start({ code: 'cancelled' });
    if (result.kind === 'empty') return this.start({ code: 'empty' });
    if (result.kind === 'no_paths') return this.start({ code: 'no_paths' });
    if (result.kind === 'failed') return this.start({ code: 'failed', detail: result.message });
    await this.enter(result.folder);
  }

  /** From the click on a folder used before. Asks for the permission first when the browser dropped it. */
  async openRecent(id: string): Promise<void> {
    const entry = (await this.deps.recent.list()).find((e) => e.id === id);
    if (!entry) return this.start();
    let access = await permissionOf(entry.handle);
    if (access === 'prompt') access = await grantAccess(entry.handle);
    if (access === 'gone') return this.start({ code: 'gone' });
    if (access !== 'granted') return this.start({ code: 'denied' });
    await this.enter(entry.handle);
  }

  async forget(id: string): Promise<void> {
    await this.deps.recent.forget(id);
    await this.start();
  }

  private async enter(handle: DirHandleLike): Promise<void> {
    this.set({ step: 'opening', name: handle.name });
    try {
      const recording = this.deps.recording ? await this.deps.recording(handle) : null;
      const snapshot = recording === null ? await this.deps.read(handle) : null;
      if (!this.readOnce) {
        // The list of folders is a convenience: a browser that cannot keep it must not keep the person out of an export.
        try {
          await this.deps.recent.remember(handle as DirHandle, this.deps.now());
        } catch {
          // the next start simply does not list this folder
        }
      }
      if (recording !== null) this.set({ step: 'replay', name: handle.name, recording });
      else if (snapshot !== null) this.set({ step: 'open', name: handle.name, handle, snapshot, readOnce: this.readOnce });
    } catch (error) {
      return this.start(noticeOf(error));
    }
  }
}

/** What a failed read of a folder says. */
export function noticeOf(error: unknown): Notice {
  if (error instanceof ExportError || error instanceof RecordingError) return { code: error.code, detail: error.detail };
  const name = typeof error === 'object' && error !== null ? (error as { name?: unknown }).name : undefined;
  if (name === 'NotFoundError') return { code: 'gone' };
  if (name === 'NotAllowedError' || name === 'SecurityError') return { code: 'denied' };
  return { code: 'failed', detail: oneLine(error) };
}
