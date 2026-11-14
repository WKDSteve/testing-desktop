import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { DirHandle, DirHandleLike, FileHandleLike, PickerWindow, RecentFolders } from 'appkit-web/folder';
import { describe, expect, it } from 'vitest';
import { lastPosition, stateAt } from '../src/lib/replay';
import { Entry, noticeOf } from '../src/web/entry';
import { ExportError } from '../src/web/folder-source';
import { EVENTS, MAX_EVENTS_BYTES, MAX_LINES, MAX_PICTURE_BYTES, pictureOf, readRecording, recordingFile, RecordingError } from '../src/web/recording';

// The recording a real run of testing.nvim left (`testing sheets step`), byte for byte.
const RECORDED = readFileSync(join(__dirname, '..', '..', 'crates', 'testing-hub', 'tests', 'fixtures', 'recording', EVENTS), 'utf8');
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

type Tree = Record<string, string | Uint8Array | { size: number }>;

/** A folder over a map of names, as the browser hands one out. `{ size }` is a file that only says how big it is. */
function folder(tree: Tree, name = 'demo.greeting'): DirHandleLike {
  const dir = (prefix: string, dirName: string): DirHandleLike => ({
    kind: 'directory',
    name: dirName,
    async *values() {
      const seen = new Set<string>();
      for (const [path, content] of Object.entries(tree)) {
        if (!path.startsWith(prefix)) continue;
        const rest = path.slice(prefix.length);
        const head = rest.split('/')[0] as string;
        if (seen.has(head)) continue;
        seen.add(head);
        if (rest.includes('/')) {
          yield dir(`${prefix}${head}/`, head);
          continue;
        }
        const bytes = typeof content === 'string' ? new TextEncoder().encode(content) : content instanceof Uint8Array ? content : new Uint8Array(0);
        const size = content instanceof Uint8Array || typeof content === 'string' ? bytes.length : content.size;
        const file: FileHandleLike = {
          kind: 'file',
          name: head,
          getFile: () => Promise.resolve({ size, lastModified: 1, arrayBuffer: () => Promise.resolve(bytes.slice().buffer) }),
        };
        yield file;
      }
    },
  });
  return dir('', name);
}

const lines = () => RECORDED.split('\n').filter((l) => l.trim() !== '');
/** The recording with one line changed. */
function withLine(match: (e: Record<string, unknown>) => boolean, change: (e: Record<string, unknown>) => void): string {
  return (
    lines()
      .map((line) => {
        const e = JSON.parse(line) as Record<string, unknown>;
        if (match(e)) change(e);
        return JSON.stringify(e);
      })
      .join('\n') + '\n'
  );
}
const after = (n: number) => (e: Record<string, unknown>) => e['event'] === 'step-after' && e['n'] === n;

async function refusal(tree: Tree): Promise<RecordingError> {
  try {
    await readRecording(folder(tree));
  } catch (error) {
    if (error instanceof RecordingError) return error;
    throw error;
  }
  throw new Error('the recording was taken');
}

describe('the recording of a stepped run', () => {
  it('is the folder with events.jsonl at its top', async () => {
    expect(await recordingFile(folder({ [EVENTS]: RECORDED }))).not.toBeNull();
    expect(await recordingFile(folder({ 'notes.json': '{}' }))).toBeNull();
    expect(await recordingFile(folder({ [`inner/${EVENTS}`]: RECORDED }))).toBeNull();
  });

  it('reads what a real run of testing.nvim left, line for line', async () => {
    const recording = await readRecording(folder({ [EVENTS]: RECORDED, 'notes.json': '{"root":"C:/somewhere"}' }));
    expect(recording.events).toHaveLength(lines().length);
    expect((recording.events[0] as { event: string }).event).toBe('start');
    const end = stateAt(recording.events, lastPosition(recording.events));
    expect(end.doc?.title).toBe('The greeting buffer');
    expect(end.finished?.status).toBe('fail');
    expect(recording.pictures.size).toBe(0);
  });

  it('takes a last line that was not ended as half written, and a recording without an end as cut off', async () => {
    const whole = lines();
    const cut = `${whole.slice(0, 8).join('\n')}\n${(whole[8] as string).slice(0, 40)}`;
    const recording = await readRecording(folder({ [EVENTS]: cut }));
    expect(recording.events).toHaveLength(8);
    // A whole last line without its line end counts.
    expect((await readRecording(folder({ [EVENTS]: whole.join('\n') }))).events).toHaveLength(whole.length);
    // Windows line ends are line ends.
    expect((await readRecording(folder({ [EVENTS]: whole.join('\r\n') + '\r\n' }))).events).toHaveLength(whole.length);
  });

  it('refuses a line that is no event of the contract, and says where and which rule, never the text', async () => {
    const broken = withLine(after(2), (e) => {
      e['status'] = 'SECRET-WORD';
    });
    const error = await refusal({ [EVENTS]: broken });
    expect(error.code).toBe('rec_invalid');
    expect(error.detail).toMatch(/^events\.jsonl:\d+\/status \(enum\)$/);
    expect(error.detail).not.toContain('SECRET-WORD');

    expect((await refusal({ [EVENTS]: `${lines()[0]}\nnot json\n${lines()[2]}\n` })).detail).toBe('events.jsonl:2: not JSON');
    expect((await refusal({ [EVENTS]: `${lines()[0]}\n[1,2]\n${lines()[2]}\n` })).code).toBe('rec_invalid');
    expect((await refusal({ [EVENTS]: lines().slice(1).join('\n') + '\n' })).detail).toContain('the first line is not the start');
    expect((await refusal({ [EVENTS]: new Uint8Array([0xff, 0xfe, 0x00]) })).detail).toBe('events.jsonl: not UTF-8');
  });

  it('refuses a recording of a newer contract by name', async () => {
    const newer = withLine(
      () => true,
      (e) => {
        e['schema'] = 'testing-sheet-session/2';
      },
    );
    expect(await refusal({ [EVENTS]: newer })).toMatchObject({ code: 'rec_too_new', detail: 'testing-sheet-session/2' });
  });

  it('refuses an empty folder, an empty file and one that is too large or too long', async () => {
    expect((await refusal({ 'notes.json': '{}' })).code).toBe('rec_empty');
    expect((await refusal({ [EVENTS]: '\n\n' })).code).toBe('rec_empty');
    expect((await refusal({ [EVENTS]: { size: MAX_EVENTS_BYTES + 1 } })).code).toBe('rec_too_large');
    const note = lines().find((l) => l.includes('"event":"note"')) as string;
    const many = `${lines()[0]}\n${`${note}\n`.repeat(MAX_LINES)}`;
    expect((await refusal({ [EVENTS]: many })).code).toBe('rec_too_large');
  });

  it('hands out a picture only when a line named a file of the folder, a PNG or a JPEG, by its plain name', async () => {
    const named = (image: unknown, shot?: unknown) =>
      withLine(after(1), (e) => {
        e['image'] = image;
        if (shot !== undefined) e['shot'] = shot;
      });
    const good = await readRecording(folder({ [EVENTS]: named('80x24-1.png', 'shot.JPG'), '80x24-1.png': PNG, 'shot.JPG': PNG, 'other.png': PNG }));
    const first = good.events.find((e) => after(1)(e as Record<string, unknown>)) as Record<string, unknown>;
    expect([first['image'], first['shot']]).toEqual([true, true]);
    expect([...good.pictures.keys()].sort()).toEqual(['80x24/1/image', '80x24/1/shot']);
    const blob = await pictureOf(good, '80x24', 1, 'image');
    expect([blob.type, blob.size]).toEqual(['image/png', PNG.length]);
    expect((await pictureOf(good, '80x24', 1, 'shot')).type).toBe('image/jpeg');
    // A step no line named a picture for has none, whatever lies in the folder.
    await expect(pictureOf(good, '80x24', 2, 'image')).rejects.toThrow('no picture');

    // A name that is a way out, another kind of file, or a file that is not there: no picture, and the page is told so.
    for (const bad of ['../secret.png', 'sub/x.png', 'C:\\x.png', 'notes.json', '.hidden.png', 'missing.png', '']) {
      const tree: Tree = { [EVENTS]: named(bad), 'notes.json': '{}', 'sub/x.png': PNG, '.hidden.png': PNG };
      const recording = await readRecording(folder(tree));
      const event = recording.events.find((e) => after(1)(e as Record<string, unknown>)) as Record<string, unknown>;
      expect(event['image'], bad).toBe(false);
      expect(recording.pictures.size, bad).toBe(0);
    }
  });

  it('does not read a picture larger than the hub would serve', async () => {
    const named = withLine(after(1), (e) => {
      e['image'] = 'big.png';
    });
    const recording = await readRecording(folder({ [EVENTS]: named, 'big.png': { size: MAX_PICTURE_BYTES + 1 } }));
    await expect(pictureOf(recording, '80x24', 1, 'image')).rejects.toThrow('too large');
  });
});

describe('the way in with a recording', () => {
  function entryOf(handle: DirHandleLike, recording: (h: DirHandleLike) => ReturnType<typeof readRecording> | Promise<null>) {
    const win: PickerWindow = { isSecureContext: true, showDirectoryPicker: () => Promise.resolve(handle as DirHandle) };
    const recent = { list: () => Promise.resolve([]), remember: () => Promise.resolve(), forget: () => Promise.resolve() } as unknown as RecentFolders;
    const exports: string[] = [];
    const entry = new Entry({
      win,
      recent,
      read: (h) => {
        exports.push(h.name);
        return Promise.reject(new ExportError('no_manifest', 'testing-export.json'));
      },
      recording,
      now: () => 0,
    });
    return { entry, exports };
  }

  it('plays a folder that is a recording back, and does not read it as an export', async () => {
    const handle = folder({ [EVENTS]: RECORDED });
    const { entry, exports } = entryOf(handle, (h) => readRecording(h));
    await entry.start();
    await entry.pick();
    expect(entry.state).toMatchObject({ step: 'replay', name: 'demo.greeting' });
    expect(exports).toEqual([]);
  });

  it('reads a folder that is no recording as an export, as before', async () => {
    const { entry, exports } = entryOf(folder({ 'x.txt': 'x' }, 'elsewhere'), () => Promise.resolve(null));
    await entry.start();
    await entry.pick();
    expect(exports).toEqual(['elsewhere']);
    expect(entry.state).toMatchObject({ step: 'choose', notice: { code: 'no_manifest' } });
  });

  it('stays on the way in when the recording is no good, and says why', async () => {
    const { entry } = entryOf(folder({ [EVENTS]: 'not json\n' }), (h) => readRecording(h));
    await entry.start();
    await entry.pick();
    expect(entry.state).toMatchObject({ step: 'choose', notice: { code: 'rec_invalid', detail: 'events.jsonl:1: not JSON' } });
    expect(noticeOf(new RecordingError('rec_too_new', 'testing-sheet-session/2'))).toEqual({ code: 'rec_too_new', detail: 'testing-sheet-session/2' });
  });
});
