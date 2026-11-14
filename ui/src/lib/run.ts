// A run through the engine, as the hub holds it (`GET /api/testing/run`, `launch` of testing-hub): the suite of a
// repository, its spec files of one name, or the sheets of a task. The page starts one and follows it until it ends;
// its result is where every run of the engine leaves it, and the page reads it with the repository again.

/** The kind of a run; `other` is what a paired device is told of a run it may not see. */
export type RunWhat = 'suite' | 'file' | 'task' | 'other';

/**
 * The verdict of an ended run, from the engine's exit code; `not_written` from its document of a task's sheets too:
 * no sheet is red, but the task could not be written (the engine's exit code is that of a red sheet).
 */
export type RunVerdict = 'green' | 'red' | 'not_written' | 'refused' | 'failed' | 'stopped';

export interface RunView {
  id: number | null;
  key: string | null;
  what: RunWhat;
  file: string | null;
  task: string | null;
  running: boolean;
  verdict: RunVerdict | null;
  exit: number | null;
  /** The end of what the engine wrote, with the folders of the PC taken out. */
  output: string;
  /**
   * What became of the task's block, when the hub has a vault of the tasks and the engine said it: its word
   * (`changed`, `unchanged`, `failed`, `missing`, ...) and whether the block stands as the run left it.
   */
  block: { outcome: string; written: boolean } | null;
}

export interface RunState {
  /** Whether the hub can start a run (it has an engine). */
  available: boolean;
  /** The run that is or was last; `null` before the first. */
  run: RunView | null;
}

const WHATS: readonly RunWhat[] = ['suite', 'file', 'task', 'other'];
const VERDICTS: readonly RunVerdict[] = ['green', 'red', 'not_written', 'refused', 'failed', 'stopped'];

const text = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const int = (v: unknown): number | null => (typeof v === 'number' && Number.isInteger(v) ? v : null);

function blockFrom(v: unknown): RunView['block'] {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  const b = v as Record<string, unknown>;
  const outcome = text(b['outcome']);
  // The hub sends a plain word; anything else is not shown as one.
  if (outcome === null || !/^[a-z_]{1,32}$/.test(outcome)) return null;
  return { outcome, written: b['written'] === true };
}

/** The state document of the hub as the page shows it; what is not of its form is left out. */
export function runFrom(doc: unknown): RunState {
  const obj = typeof doc === 'object' && doc !== null && !Array.isArray(doc) ? (doc as Record<string, unknown>) : {};
  const r = obj['run'];
  if (typeof r !== 'object' || r === null || Array.isArray(r)) return { available: obj['available'] === true, run: null };
  const run = r as Record<string, unknown>;
  const what = WHATS.find((w) => w === run['what']) ?? 'other';
  const verdict = VERDICTS.find((v) => v === run['verdict']) ?? null;
  return {
    available: obj['available'] === true,
    run: {
      id: int(run['id']),
      key: text(run['key']),
      what,
      file: text(run['file']),
      task: text(run['task']),
      running: run['running'] === true,
      verdict: run['running'] === true ? null : verdict,
      exit: int(run['exit']),
      output: text(run['output']) ?? '',
      block: run['running'] === true ? null : blockFrom(run['block']),
    },
  };
}

/** The longest file text the hub takes (`launch::MAX_FILE_TEXT`). */
export const FILE_TEXT_MAX = 100;

/** Why a file text would be refused by the hub (`launch::is_file_text`), or `null` when it is good. */
export function fileTextProblem(value: string): 'empty' | 'too_long' | 'not_plain' | null {
  const t = value.trim();
  if (t === '') return 'empty';
  if (t.length > FILE_TEXT_MAX) return 'too_long';
  if (/^[-./]/.test(t) || t.includes('..') || !/^[A-Za-z0-9_.\-/]+$/.test(t)) return 'not_plain';
  return null;
}
