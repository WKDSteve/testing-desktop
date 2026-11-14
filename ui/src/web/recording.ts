// The recording of a stepped run as a source of the page: the folder testing.nvim keeps beside the pictures of a run
// (`testing sheets step`: `events.jsonl`, every event of `testing-sheet-session/1` as one line, and the pictures by
// their file names). Every line is checked against the contract before anything of the recording is used; a picture is
// read only when its tile is shown, and only a file of the folder a line of the recording named.
//
// Nothing here reaches the network, and it holds no more than the lines in memory.

import type { DirHandleLike, FileHandleLike } from 'appkit-web/folder';
import validate from './validate-session.generated.js';

export const EVENTS = 'events.jsonl';
/** The version of the contract this page knows. */
export const SCHEMA_VERSION = 1;
/** The whole recording: a line holds at most the screen of one step. */
export const MAX_EVENTS_BYTES = 16 * 1024 * 1024;
/** More lines than a sheet can have events (the hub keeps as many of a run). */
export const MAX_LINES = 20_000;
/** A picture of a step, as the hub serves at most. */
export const MAX_PICTURE_BYTES = 16 * 1024 * 1024;

export type RecordingProblem = 'rec_too_large' | 'rec_too_new' | 'rec_invalid' | 'rec_empty';

/** Why a recording cannot be used. `detail` is a line number, a rule or a word of the contract, never the text of a line. */
export class RecordingError extends Error {
  readonly code: RecordingProblem;
  readonly detail: string;

  constructor(code: RecordingProblem, detail: string) {
    super(`${code}: ${detail}`);
    this.name = 'RecordingError';
    this.code = code;
    this.detail = detail;
  }
}

/** One recording, read whole. */
export interface Recording {
  /** The events in the form the hub passes them on: no path, a picture is a yes. */
  events: unknown[];
  /** The files of the pictures the recording names, by `<size>/<n>/<kind>`. */
  pictures: Map<string, FileHandleLike>;
}

/** The name of a picture as the recording may name it: a file of the folder, a PNG or a JPEG, no way out. */
const PICTURE_NAME = /^[A-Za-z0-9_][A-Za-z0-9._-]{0,199}\.(png|jpe?g)$/i;

/** The file of the recording in a folder; `null`: the folder is no recording. */
export async function recordingFile(chosen: DirHandleLike): Promise<FileHandleLike | null> {
  for await (const entry of chosen.values()) {
    if (entry.kind === 'file' && entry.name === EVENTS) return entry;
  }
  return null;
}

/** The version of the contract a line names; `null`: it names none. */
function versionOf(doc: unknown): number | null {
  const schema = typeof doc === 'object' && doc !== null ? (doc as { schema?: unknown }).schema : null;
  const match = typeof schema === 'string' ? /^testing-sheet-session\/(\d+)$/.exec(schema) : null;
  return match ? Number(match[1]) : null;
}

/** One line, checked: a newer contract first, then the contract. The recording leaves `root` out of `start`. */
function checked(doc: unknown, line: number): Record<string, unknown> {
  const version = versionOf(doc);
  if (version !== null && version > SCHEMA_VERSION) throw new RecordingError('rec_too_new', `testing-sheet-session/${version}`);
  const isStart = typeof doc === 'object' && doc !== null && !Array.isArray(doc) && (doc as { event?: unknown }).event === 'start';
  const subject = isStart && !('root' in (doc as object)) ? { ...(doc as object), root: '' } : doc;
  if (!validate(subject)) {
    const first = validate.errors?.[0];
    // Where and which rule: the message of the check could quote a value of the line.
    throw new RecordingError('rec_invalid', `${EVENTS}:${line}${first?.instancePath ?? ''} (${first?.keyword ?? 'contract'})`);
  }
  return doc as Record<string, unknown>;
}

/** The text of the recording as lines; a last line that was not ended is half written and left out. */
function linesOf(text: string): string[] {
  const lines = text.split('\n');
  const tail = lines.pop() ?? '';
  if (tail.trim() !== '') {
    try {
      JSON.parse(tail);
      lines.push(tail);
    } catch {
      // the run was cut off in the middle of this line
    }
  }
  return lines;
}

/** Read one recording whole, or refuse it. */
export async function readRecording(chosen: DirHandleLike): Promise<Recording> {
  const files = new Map<string, FileHandleLike>();
  for await (const entry of chosen.values()) {
    if (entry.kind === 'file') files.set(entry.name, entry);
  }
  const file = files.get(EVENTS);
  if (file === undefined) throw new RecordingError('rec_empty', EVENTS);
  const blob = await file.getFile();
  if (blob.size > MAX_EVENTS_BYTES) throw new RecordingError('rec_too_large', EVENTS);
  let text: string;
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(await blob.arrayBuffer());
  } catch {
    throw new RecordingError('rec_invalid', `${EVENTS}: not UTF-8`);
  }
  const events: unknown[] = [];
  const pictures = new Map<string, FileHandleLike>();
  for (const [index, raw] of linesOf(text).entries()) {
    const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw;
    if (line.trim() === '') continue;
    if (events.length >= MAX_LINES) throw new RecordingError('rec_too_large', `${EVENTS}: more than ${MAX_LINES} lines`);
    let doc: unknown;
    try {
      doc = JSON.parse(line);
    } catch {
      throw new RecordingError('rec_invalid', `${EVENTS}:${index + 1}: not JSON`);
    }
    const event = { ...checked(doc, index + 1) };
    if (event['event'] === 'step-after') {
      // As the hub does: the page learns that there is a picture, and asks for it by size and step.
      for (const kind of ['image', 'shot'] as const) {
        const name = event[kind];
        const found = typeof name === 'string' && PICTURE_NAME.test(name) ? files.get(name) : undefined;
        if (found) pictures.set(`${String(event['size'])}/${String(event['n'])}/${kind}`, found);
        if (kind in event) event[kind] = found !== undefined;
      }
    }
    events.push(event);
  }
  const first = events[0] as { event?: unknown } | undefined;
  if (first === undefined) throw new RecordingError('rec_empty', EVENTS);
  if (first.event !== 'start') throw new RecordingError('rec_invalid', `${EVENTS}:1: the first line is not the start of a run`);
  return { events, pictures };
}

/** The picture of a step of the recording, as the tile asks for it. */
export async function pictureOf(recording: Recording, size: string, n: number, kind: 'image' | 'shot'): Promise<Blob> {
  const file = recording.pictures.get(`${size}/${n}/${kind}`);
  if (file === undefined) throw new Error('the recording has no picture of this step');
  const data = await file.getFile();
  if (data.size > MAX_PICTURE_BYTES) throw new Error('the picture is too large');
  const type = /\.png$/i.test(file.name) ? 'image/png' : 'image/jpeg';
  return new Blob([await data.arrayBuffer()], { type });
}
