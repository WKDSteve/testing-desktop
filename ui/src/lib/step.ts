// The model of a stepped run of a test sheet, as the page shows it: the events of `testing-sheet-session/1` (as the
// hub passes them on: no path, a picture is a yes) folded into one state, the same way for the state the hub holds
// and for the lines that come on the stream. The words and the division follow the panel of testing.nvim
// (`testing.sheet.panel`), so that both surfaces read the same. Nothing here touches a transport or the DOM.

import { markHidden } from 'appkit-ui/text';

/** The contract the hub passes on; a higher one is refused. */
export const SCHEMA = 'testing-sheet-session/1';

/** The longest note, in bytes, as testing.nvim and the hub take it. */
export const NOTE_MAX_BYTES = 1000;

/** The event of the hub's stream that carries a line of the run. */
export const STREAM_EVENT = 'sheet-step';

export interface SheetStepDoc {
  n: number;
  action: string;
  value: string;
  note: string | null;
}

/** The whole sheet, from `start`. */
export interface SheetDoc {
  id: string;
  title: string;
  engine: string;
  app: string | null;
  task: string | null;
  sizes: string[];
  steps: SheetStepDoc[];
  breaks: number[];
  judge: string[];
  manual: string[];
}

/** What a step did (`step-after`). */
export interface Mark {
  status: 'pass' | 'fail';
  ms: number | null;
  reason: string | null;
  failure: string | null;
  /** The target after the step, to be asked of the hub by size and step. */
  image: boolean;
  /** The file of a `shot` step, likewise. */
  shot: boolean;
  /** The child's screen as text, one string per row (rpc sheets). */
  screen: string[] | null;
}

export interface SizeState {
  size: string;
  state: 'open' | 'failed' | 'skip';
  reason: string | null;
}

export interface Finished {
  status: string;
  ok: boolean;
  verdict: boolean;
  failures: string[];
  skipped: string | null;
  ms: number | null;
}

export type StepStatus = 'pending' | 'running' | 'next' | 'pass' | 'fail' | 'skipped';

export interface StepState {
  /** The run of the hub this state is of; `null` before any. */
  session: number | null;
  /** The last line taken, of that run. */
  seq: number;
  /** Whether the hub can run a sheet at all. */
  available: boolean;
  running: boolean;
  key: string | null;
  sheet: string | null;
  doc: SheetDoc | null;
  sizes: SizeState[];
  /** The size that runs or ran last. */
  size: string | null;
  /** The step that runs or runs next. */
  at: number | null;
  /** The step is being run right now. */
  stepping: boolean;
  /** Why the run waits: `start`, `step`, `break`, `until`, `failed`; `null` while it runs. */
  paused: string | null;
  /** The question the run asks, with its step. */
  question: { size: string; n: number; text: string } | null;
  /** What each step of each size did. */
  marks: Record<string, Record<number, Mark>>;
  /** The notes taken in the run (`note`), per size and step, in order. */
  notes: Record<string, Record<number, string[]>>;
  /** The breakpoints, the page's own, as the panel keeps them (the run holds them too). */
  breaks: number[];
  finished: Finished | null;
  /** The hub's note when the child ended without an `end`: its exit code and what it said. */
  exit: { code: number | null; detail: string } | null;
  /** The last `error` of the run: a command it could not do. */
  error: string | null;
  /** The outcome of the task's block (`--tasks`), if any. */
  task: { id: string; outcome: string; message: string | null } | null;
}

export function emptyStep(): StepState {
  return {
    session: null,
    seq: 0,
    available: true,
    running: false,
    key: null,
    sheet: null,
    doc: null,
    sizes: [],
    size: null,
    at: null,
    stepping: false,
    paused: null,
    question: null,
    marks: {},
    notes: {},
    breaks: [],
    finished: null,
    exit: null,
    error: null,
    task: null,
  };
}

const str = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);
const obj = (v: unknown): Record<string, unknown> | null =>
  typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : null;

function sizeTag(v: unknown): string | null {
  const o = obj(v);
  if (!o) return null;
  const cols = num(o['cols']);
  const rows = num(o['rows']);
  return cols !== null && rows !== null ? `${cols}x${rows}` : null;
}

function docOf(e: Record<string, unknown>): SheetDoc {
  const steps: SheetStepDoc[] = (Array.isArray(e['steps']) ? e['steps'] : [])
    .map((s) => obj(s))
    .filter((s): s is Record<string, unknown> => s !== null)
    .map((s) => ({
      n: num(s['n']) ?? 0,
      action: str(s['action']) ?? '',
      value: typeof s['value'] === 'number' ? String(s['value']) : (str(s['value']) ?? ''),
      note: str(s['note']),
    }))
    .filter((s) => s.n > 0);
  return {
    id: str(e['id']) ?? '',
    title: str(e['title']) ?? '',
    engine: str(e['engine']) ?? '',
    app: str(e['app']),
    task: str(e['task']),
    sizes: (Array.isArray(e['sizes']) ? e['sizes'] : []).map(sizeTag).filter((s): s is string => s !== null),
    steps,
    breaks: (Array.isArray(e['breaks']) ? e['breaks'] : []).map(num).filter((n): n is number => n !== null),
    judge: strings(e['judge']),
    manual: strings(e['manual']),
  };
}

function markOf(e: Record<string, unknown>): Mark {
  return {
    status: e['status'] === 'fail' ? 'fail' : 'pass',
    ms: num(e['ms']),
    reason: str(e['reason']),
    failure: str(e['failure']),
    image: e['image'] === true,
    shot: e['shot'] === true,
    screen: Array.isArray(e['screen']) ? strings(e['screen']) : null,
  };
}

/**
 * One event of the run folded into the state. Unknown events and fields are for a newer reader and change nothing.
 * The event is the hub's `event` object, either from its state document or from a line of the stream.
 */
export function reduce(state: StepState, event: unknown): StepState {
  const e = obj(event);
  if (!e) return state;
  const name = str(e['event']);
  // The hub's own note has no schema; a line of the child must speak the contract.
  if (name !== 'exit' && e['schema'] !== SCHEMA) return state;
  switch (name) {
    case 'start': {
      const doc = docOf(e);
      return { ...state, doc, breaks: doc.breaks, finished: null, exit: null, error: null, task: null };
    }
    case 'size': {
      const size = str(e['size']);
      if (!size) return state;
      const entry: SizeState = {
        size,
        state: e['state'] === 'failed' ? 'failed' : e['state'] === 'skip' ? 'skip' : 'open',
        reason: str(e['reason']),
      };
      const sizes = [...state.sizes.filter((s) => s.size !== size), entry];
      const marks = { ...state.marks, [size]: state.marks[size] ?? {} };
      return { ...state, sizes, marks, size: entry.state === 'open' ? size : state.size };
    }
    case 'step-before':
      return { ...state, size: str(e['size']) ?? state.size, at: num(e['n']), paused: null, stepping: true, question: null };
    case 'paused':
      return { ...state, size: str(e['size']) ?? state.size, at: num(e['n']), paused: str(e['reason']) ?? 'step', stepping: false };
    case 'ask': {
      const size = str(e['size']) ?? state.size ?? '';
      const n = num(e['n']) ?? 0;
      return { ...state, size, at: n, paused: 'ask', stepping: false, question: { size, n, text: str(e['text']) ?? '' } };
    }
    case 'step-after': {
      const size = str(e['size']) ?? state.size ?? '';
      const n = num(e['n']);
      if (n === null) return state;
      const marks = { ...state.marks, [size]: { ...(state.marks[size] ?? {}), [n]: markOf(e) } };
      return { ...state, marks, stepping: false, question: null };
    }
    case 'note': {
      const size = str(e['size']) ?? state.size ?? '';
      const n = num(e['n']);
      const text = str(e['text']);
      if (n === null || text === null) return state;
      const bySize = state.notes[size] ?? {};
      const notes = { ...state.notes, [size]: { ...bySize, [n]: [...(bySize[n] ?? []), text] } };
      return { ...state, notes };
    }
    case 'error':
      return { ...state, error: str(e['message']) ?? '' };
    case 'task':
      return { ...state, task: { id: str(e['id']) ?? '', outcome: str(e['outcome']) ?? '', message: str(e['message']) } };
    case 'end':
      return {
        ...state,
        finished: {
          status: str(e['status']) ?? '',
          ok: e['ok'] === true,
          verdict: e['verdict'] === true,
          failures: strings(e['failures']),
          skipped: str(e['skipped']),
          ms: num(e['ms']),
        },
        paused: null,
        stepping: false,
        question: null,
        running: false,
      };
    case 'exit':
      return { ...state, exit: { code: num(e['code']), detail: str(e['detail']) ?? '' }, paused: null, stepping: false, question: null, running: false };
    default:
      return state;
  }
}

/** The state document of the hub (`GET sheet-step`) folded into a fresh state. */
export function fromDocument(doc: unknown): StepState {
  const d = obj(doc);
  const base = emptyStep();
  if (!d) return base;
  base.available = d['available'] !== false;
  base.session = num(d['session']);
  base.seq = num(d['seq']) ?? 0;
  base.running = d['running'] === true;
  base.key = str(d['key']);
  base.sheet = str(d['sheet']);
  let state = base;
  for (const event of Array.isArray(d['events']) ? d['events'] : []) state = reduce(state, event);
  // The document says whether it runs; an `end` among the events says the same.
  state.running = base.running && state.finished === null && state.exit === null;
  return state;
}

/** A line of the stream (`data` of a `sheet-step` event) taken into the state: only a later line of the same run. */
export function fromStream(state: StepState, data: string): StepState {
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    return state;
  }
  const line = obj(parsed);
  if (!line) return state;
  const session = num(line['session']);
  const seq = num(line['seq']);
  if (session === null || seq === null) return state;
  // Another run than the one the page holds, or a later line of a run it never read (it came while the run was under
  // way): it missed the start and reads the state again (the caller).
  const missed = state.session === null ? seq > 1 : session !== state.session;
  if (missed) return { ...state, session, seq: 0, stale: true } as StepState;
  if (seq <= state.seq) return state;
  const next = reduce(state, line['event']);
  return { ...next, session, seq, running: next.finished === null && next.exit === null };
}

/** Whether a line of another run came: the page reads the state of the hub again. */
export function isStale(state: StepState): boolean {
  return (state as { stale?: boolean }).stale === true;
}

/** The status of a step, as the list shows it. */
export function statusOf(state: StepState, n: number): StepStatus {
  const size = state.size;
  const mark = size ? state.marks[size]?.[n] : undefined;
  if (mark) return mark.status;
  if (state.finished || state.exit) {
    // A step the run never reached: after a failed step, a skip, or an abort.
    return 'skipped';
  }
  if (n === state.at) return state.stepping ? 'running' : 'next';
  return 'pending';
}

/** The marks of the size that runs or ran last. */
export function marksOf(state: StepState): Record<number, Mark> {
  return state.size ? (state.marks[state.size] ?? {}) : {};
}

/** The notes of a step in the size that runs or ran last. */
export function notesOf(state: StepState, n: number): string[] {
  return state.size ? (state.notes[state.size]?.[n] ?? []) : [];
}

/**
 * The step whose tile is shown: the one chosen, else the last one that did something or got a note, else the one that
 * runs next.
 */
export function tileStep(state: StepState, chosen: number | null): number | null {
  const marks = marksOf(state);
  const notes = state.size ? (state.notes[state.size] ?? {}) : {};
  if (chosen !== null && (marks[chosen] || notes[chosen] || chosen === state.at)) return chosen;
  const done = [...Object.keys(marks), ...Object.keys(notes)]
    .map(Number)
    .filter((n) => Number.isFinite(n));
  if (done.length > 0) return Math.max(...done);
  return state.at;
}

/** One line of a value, as the panel shows it: whitespace folded, cut, hidden characters marked. */
export function oneLine(value: string | number | null | undefined, max: number): string {
  const text = String(value ?? '').replace(/\s+/g, ' ');
  const chars = [...text];
  const cut = chars.length > max ? `${chars.slice(0, max - 3).join('')}...` : text;
  return markHidden(cut);
}

/** The rows of a screen without the empty tail, as the panel shows them. */
export function screenRows(rows: string[]): string[] {
  const out = rows.map((r) => markHidden(r.replace(/\s+$/, '')));
  while (out.length > 0 && /^\s*~?\s*$/.test(out[out.length - 1] ?? '')) out.pop();
  return out;
}

/** What the second line says: where the run stands, in the words of the panel. */
export type StatusLine =
  | { kind: 'finished'; status: string; ms: number; skipped: string | null }
  | { kind: 'exit'; code: number | null; detail: string }
  | { kind: 'asks'; text: string }
  | { kind: 'paused_failed'; n: number; size: string }
  | { kind: 'paused_before'; n: number; size: string }
  | { kind: 'running'; n: number; size: string }
  | { kind: 'starting' }
  | { kind: 'none' };

export function statusLine(state: StepState): StatusLine {
  if (state.finished) {
    return { kind: 'finished', status: state.finished.status, ms: state.finished.ms ?? 0, skipped: state.finished.skipped };
  }
  if (state.exit) return { kind: 'exit', code: state.exit.code, detail: state.exit.detail };
  if (state.question) return { kind: 'asks', text: state.question.text };
  if (state.paused && state.at !== null) {
    const size = state.size ?? '';
    return state.paused === 'failed' ? { kind: 'paused_failed', n: state.at, size } : { kind: 'paused_before', n: state.at, size };
  }
  if (state.stepping && state.at !== null) return { kind: 'running', n: state.at, size: state.size ?? '' };
  if (state.session === null) return { kind: 'none' };
  return { kind: 'starting' };
}
