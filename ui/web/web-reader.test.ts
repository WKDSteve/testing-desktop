import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { DirHandleLike, DirHandle, FileHandleLike, PickerWindow, RecentFolders } from 'appkit-web/folder';
import { describe, expect, it } from 'vitest';
import { Entry, noticeOf, type EntryState } from '../src/web/entry';
import { ExportError, readExport, type Snapshot } from '../src/web/folder-source';
import { WebReader } from '../src/web/web-reader.svelte';

const GOLDEN = join(__dirname, '..', '..', 'crates', 'testing-hub', 'tests', 'fixtures', 'golden-export');
const ALPHA = 'alpha.nvim-356bf237c7fc';

function folder(dir: string, name = 'export'): DirHandleLike {
  return {
    kind: 'directory',
    name,
    async *values() {
      for (const entry of readdirSync(dir)) {
        const path = join(dir, entry);
        if (statSync(path).isDirectory()) {
          yield folder(path, entry);
        } else {
          const bytes = new Uint8Array(readFileSync(path));
          const file: FileHandleLike = {
            kind: 'file',
            name: entry,
            getFile: () => Promise.resolve({ size: bytes.length, lastModified: 1, arrayBuffer: () => Promise.resolve(bytes.slice().buffer) }),
          };
          yield file;
        }
      }
    },
  };
}

const handle = folder(GOLDEN);
const good = (): Promise<Snapshot> => readExport(handle);

describe('the page on a folder', () => {
  it('shows the fleet of the export, and a repository without asking for findings', async () => {
    const reader = new WebReader(handle, await good());
    await reader.start();
    expect(reader.phase).toBe('ready');
    expect(reader.fleet?.kind).toBe('fleet');
    expect(reader.generatedAt).toBe('2026-10-31T12:00:00Z');
    expect(reader.hasFindings).toBe(false);
    await reader.open(ALPHA);
    expect(reader.repo?.kind).toBe('repo');
    expect(reader.findings).toBeNull();
    expect(reader.findingsFailure).toBeNull();
    reader.stop();
  });

  it('a read that fails leaves the export on the page and says so; the next good one clears it', async () => {
    let fail = true;
    const first = await good();
    const reader = new WebReader(handle, first, async () => {
      if (fail) throw new ExportError('hash', 'fleet.json');
      const next = await good();
      next.manifest = { ...next.manifest, generated_at: '2026-11-01T12:00:00Z' };
      return next;
    });
    await reader.start();
    await reader.open(ALPHA);
    const shown = reader.fleet;
    await reader.reload();
    expect(reader.stale).toEqual({ code: 'hash', detail: 'fleet.json' });
    expect(reader.fleet).toBe(shown);
    expect(reader.repo).not.toBeNull();
    expect(reader.generatedAt).toBe('2026-10-31T12:00:00Z');

    fail = false;
    await reader.reload();
    expect(reader.stale).toBeNull();
    expect(reader.generatedAt).toBe('2026-11-01T12:00:00Z');
    reader.stop();
  });

  it('a read that ends after the page stopped changes nothing', async () => {
    let release: (s: Snapshot) => void = () => {};
    const reader = new WebReader(handle, await good(), () => new Promise<Snapshot>((resolve) => (release = resolve)));
    await reader.start();
    const reading = reader.reload();
    reader.stop();
    const next = await good();
    next.manifest = { ...next.manifest, generated_at: '2030-01-01T00:00:00Z' };
    release(next);
    await reading;
    expect(reader.generatedAt).toBe('2026-10-31T12:00:00Z');
  });
});

describe('the way in', () => {
  function entryOf(win: PickerWindow, read = good as (h: DirHandleLike) => Promise<Snapshot>) {
    const remembered: string[] = [];
    const recent = {
      list: () => Promise.resolve([]),
      remember: (h: DirHandle) => {
        remembered.push(h.name);
        return Promise.reject(new DOMException('no clone', 'DataCloneError'));
      },
      forget: () => Promise.resolve(),
    } as unknown as RecentFolders;
    const entry = new Entry({ win, recent, read, now: () => 0 });
    const states: EntryState[] = [];
    entry.subscribe((s) => states.push(s));
    return { entry, states, remembered };
  }
  const picker = (h: unknown): PickerWindow => ({ isSecureContext: true, showDirectoryPicker: () => Promise.resolve(h as DirHandle) });

  it('asks for the read mode under the id of this page', async () => {
    const asked: unknown[] = [];
    const { entry } = entryOf({
      isSecureContext: true,
      showDirectoryPicker: (options) => {
        asked.push(options);
        return Promise.reject(Object.assign(new Error('x'), { name: 'AbortError' }));
      },
    });
    await entry.start();
    await entry.pick();
    expect(asked).toEqual([{ id: 'testing-web-export', mode: 'read' }]);
    expect(entry.state).toMatchObject({ step: 'choose', notice: { code: 'cancelled' } });
  });

  it('opens a good export, also when the browser cannot keep the folder', async () => {
    const { entry, states, remembered } = entryOf(picker(handle));
    await entry.start();
    await entry.pick();
    expect(remembered).toEqual(['export']);
    expect(states.at(-1)).toMatchObject({ step: 'open', name: 'export', readOnce: false });
  });

  it('stays on the way in when the folder is no good export, and says why', async () => {
    const { entry } = entryOf(picker(handle), () => Promise.reject(new ExportError('too_new', 'testing-export/2')));
    await entry.start();
    await entry.pick();
    expect(entry.state).toMatchObject({ step: 'choose', notice: { code: 'too_new', detail: 'testing-export/2' } });
  });

  it('says so when the browser has no way to a folder', async () => {
    const { entry } = entryOf({});
    await entry.start();
    expect(entry.state).toMatchObject({ step: 'unsupported', reason: 'missing' });
  });

  it('names what the browser says of a folder', () => {
    expect(noticeOf(new DOMException('x', 'NotFoundError'))).toEqual({ code: 'gone' });
    expect(noticeOf(new DOMException('x', 'NotAllowedError'))).toEqual({ code: 'denied' });
    expect(noticeOf(new Error('boom\nline'))).toMatchObject({ code: 'failed' });
  });
});
