// The page on an export folder: what it reads is `Reader`'s, the source is the folder. Reading again replaces the export
// only when all of the new one is good; a failure leaves the page on the export it holds and says so.

import type { DirHandleLike } from 'appkit-web/folder';
import { Reader } from '../lib/reader.svelte';
import { noticeOf, type Notice } from './entry';
import { FolderSource, readExport, type Snapshot } from './folder-source';

export class WebReader extends Reader {
  /** When the export that is shown was written. */
  generatedAt = $state<string | null>(null);
  /** The folder changed and could not be read: the page shows the export before. */
  stale = $state<Notice | null>(null);

  private readonly folder: FolderSource;
  private readonly handle: DirHandleLike;
  private readonly read: (handle: DirHandleLike) => Promise<Snapshot>;

  constructor(handle: DirHandleLike, first: Snapshot, read: (handle: DirHandleLike) => Promise<Snapshot> = readExport) {
    const folder = new FolderSource(first);
    super(folder);
    this.folder = folder;
    this.handle = handle;
    this.read = read;
    this.generatedAt = folder.generatedAt;
  }

  /** Show what was read when the page came in. */
  start(): Promise<void> {
    return this.refresh();
  }

  stop(): void {
    this.stopped = true;
    this.repoAbort?.abort();
  }

  /** Read the folder again, and the page with it. */
  override async reload(): Promise<void> {
    try {
      this.folder.replace(await this.read(this.handle));
    } catch (error) {
      if (!this.stopped) this.stale = noticeOf(error);
      return;
    }
    if (this.stopped) return;
    this.stale = null;
    this.generatedAt = this.folder.generatedAt;
    await this.readAgain();
  }
}
