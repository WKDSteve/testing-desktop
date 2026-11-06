// What the views show of the documents of testing.nvim: pure functions of the documents, no state, no transport.
// Text of the engine (names, messages, outputs) is foreign: it is only ever shown as text, with the characters that
// hide or reorder what is read made visible (`shown`), and a link is built only to a CI run of GitHub (`runLink`).

import { markHidden } from 'appkit-ui/text';
import type { Case, Cell, Fleet, RemoteState, Row } from './contract';
import type { Key } from './i18n';

/** The three systems of the CI, in the order of the columns. */
export const SYSTEMS = ['ubuntu', 'windows', 'macos'] as const;
export type System = (typeof SYSTEMS)[number];

/** How a result reads at a glance. `unknown` is not green: nothing was read, or nothing could be. */
export type Tone = 'green' | 'red' | 'running' | 'waiting' | 'skipped' | 'unknown';

/** A row of the overview. */
export interface View {
  key: string;
  name: string;
  local: { tone: Tone; verdict: string | null; failed: number; ts: string | null } | null;
  ci: Record<System, { tone: Tone; state: RemoteState | null; link: string | null }>;
  run: { tone: Tone; state: RemoteState | null; link: string | null } | null;
  red: boolean;
}

const RED_STATES: readonly RemoteState[] = ['failure', 'rejected', 'cancelled', 'no_runner'];

/** The tone of a state of the CI adapter. */
export function remoteTone(state: RemoteState | null | undefined): Tone {
  switch (state) {
    case 'success':
      return 'green';
    case 'failure':
    case 'rejected':
    case 'cancelled':
    case 'no_runner':
      return 'red';
    case 'running':
    case 'queued':
      return 'running';
    case 'waiting_runner':
      return 'waiting';
    case 'skipped':
    case 'no_run':
    case 'no_github':
      return 'skipped';
    default:
      return 'unknown';
  }
}

/** The tone of a local verdict; `green-partial` is not a full green and reads as unknown. */
export function verdictTone(verdict: string | null | undefined): Tone {
  if (verdict === 'green') return 'green';
  if (verdict === 'red' || verdict === 'error' || verdict === 'infra') return 'red';
  return 'unknown';
}

// A run or a job of a repository of the maintainer, and nothing else: owner fixed, repository a plain name, numbers.
const RUN_LINK = /^https:\/\/github\.com\/WKDSteve\/[A-Za-z0-9._-]{1,100}\/actions\/runs\/[0-9]{1,20}(?:\/job\/[0-9]{1,20})?$/;

/** The address of a CI run, when it is one; anything else is shown as text, never as a link. */
export function runLink(url: string | null | undefined): string | null {
  return typeof url === 'string' && RUN_LINK.test(url) ? url : null;
}

function cell(c: Cell | undefined): View['ci'][System] {
  if (!c) return { tone: 'unknown', state: null, link: null };
  return { tone: remoteTone(c.state), state: c.state, link: runLink(c.url) };
}

/** The failed cases of a local summary: failures, errors, time-outs and crashes. */
function failedOf(row: Row): number {
  const s = row.local?.summary;
  return s ? s.fail + s.error + s.timeout + s.crash : 0;
}

/** One row of the fleet as the overview shows it. */
export function viewOf(row: Row): View {
  const remote = row.ci_remote;
  const local = row.local
    ? { tone: verdictTone(row.local.verdict), verdict: row.local.verdict, failed: failedOf(row), ts: row.local.ts }
    : null;
  const ci = {
    ubuntu: cell(remote?.os.ubuntu ?? undefined),
    windows: cell(remote?.os.windows ?? undefined),
    macos: cell(remote?.os.macos ?? undefined),
  };
  const run = remote ? { tone: remoteTone(remote.state), state: remote.state, link: runLink(remote.run?.url) } : null;
  const red =
    local?.tone === 'red' ||
    verdictTone(row.ci_local?.verdict) === 'red' ||
    (remote !== null && RED_STATES.includes(remote.state)) ||
    SYSTEMS.some((s) => ci[s].tone === 'red');
  return { key: row.key, name: row.name, local, ci, run, red };
}

/** The sentence that explains one cell of the overview: a key of the words and what fills it. */
export interface Tip {
  key: Key;
  params: Record<string, string | number>;
}

/** What the cell of the local run of a repository means. */
export function localTip(view: View): Tip {
  const repo = markHidden(view.name);
  const local = view.local;
  if (!local) return { key: 'tip.local.none', params: { repo } };
  if (local.failed > 0) return { key: 'tip.local.failed', params: { repo, n: local.failed } };
  if (local.tone === 'green') return { key: 'tip.local.green', params: { repo } };
  if (local.tone === 'red') return { key: 'tip.local.red', params: { repo } };
  if (local.verdict === 'green-partial') return { key: 'tip.local.partial', params: { repo } };
  return { key: 'tip.local.unknown', params: { repo } };
}

/** The states a sentence exists for; a state the page does not know reads as unknown. */
const TIP_STATES: readonly RemoteState[] = [
  'failure',
  'rejected',
  'cancelled',
  'no_runner',
  'unknown',
  'waiting_runner',
  'queued',
  'running',
  'success',
  'skipped',
  'no_run',
  'no_github',
];

/** The states of a whole repository that explain a system without a job of its own. */
const WHOLE_STATES: readonly RemoteState[] = ['rejected', 'skipped', 'no_run', 'no_github', 'unknown'];

/** What the cell of the CI of a repository on one system means. */
export function ciTip(view: View, system: System): Tip {
  const params = { repo: markHidden(view.name), system };
  if (view.run === null) return { key: 'tip.ci.not_asked', params };
  const state = view.ci[system].state;
  if (state === null) {
    const whole = view.run.state;
    if (whole !== null && WHOLE_STATES.includes(whole)) return { key: `tip.ci.${whole}`, params };
    return { key: 'tip.ci.no_job', params };
  }
  return { key: TIP_STATES.includes(state) ? `tip.ci.${state}` : 'tip.ci.unknown', params };
}

/** The rows of the fleet: red first, then by name; with `onlyRed` the red ones only; `query` a part of the name. */
export function rowsOf(fleet: Fleet, options: { onlyRed?: boolean; query?: string } = {}): View[] {
  const query = (options.query ?? '').trim().toLowerCase();
  return fleet.repos
    .map(viewOf)
    .filter((v) => !options.onlyRed || v.red)
    .filter((v) => query === '' || v.name.toLowerCase().includes(query))
    .sort((a, b) => Number(b.red) - Number(a.red) || a.name.localeCompare(b.name));
}

/** What the fleet says about itself that the page must not hide: why less was released, what was left out. */
export interface FleetNotes {
  /** The data classes could not be used (`classes.problem`): an empty fleet is not a green one. */
  classesProblem: string | null;
  excluded: number;
  /** The adapter of the CI was asked and failed. */
  remoteError: string | null;
  /** The local results were not read (the workstation). */
  localRead: boolean;
}

export function notesOf(fleet: Fleet): FleetNotes {
  return {
    classesProblem: fleet.classes?.problem ?? null,
    excluded: fleet.excluded,
    remoteError: fleet.remote.error,
    localRead: fleet.local_read !== false,
  };
}

/** The tone of a run of the log. A verdict the page does not know is unknown, never green. */
export function runTone(verdict: string): Tone {
  if (verdict === 'green-partial') return 'green';
  return verdictTone(verdict);
}

/** The share of green among the runs with a result, in whole percent; `null`: there is none to say anything about. */
export function greenPercent(green: number, red: number): number | null {
  return green + red > 0 ? Math.round((green / (green + red)) * 100) : null;
}

/** How long ago a time of a document was, for a page that shows an old state; `null`: it is no time, or in the future. */
export function ageOf(iso: string, now: number): { unit: 'now' | 'minutes' | 'hours' | 'days'; n: number } | null {
  const then = Date.parse(iso);
  if (Number.isNaN(then) || then > now + 60_000) return null;
  const minutes = Math.max(0, Math.floor((now - then) / 60_000));
  if (minutes < 1) return { unit: 'now', n: 0 };
  if (minutes < 60) return { unit: 'minutes', n: minutes };
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return { unit: 'hours', n: hours };
  return { unit: 'days', n: Math.floor(hours / 24) };
}

/** Foreign text as it may be shown: the hidden characters made visible; `null` stays `null`. */
export function shown(text: string | null | undefined): string | null {
  return typeof text === 'string' ? markHidden(text) : null;
}

/** The title of a case: its `describe` path and its name. */
export function caseTitle(c: Case): string {
  return markHidden([...c.describe, c.name ?? c.id].join(' › '));
}

/** One finding of the last run, as the page shows it: every field is text of the engine or `null`, nothing else. */
export interface Finding {
  fp: string;
  kind: string;
  title: string;
  excerpt: string;
  where: { file: string; line: number | null } | null;
  repro: string | null;
  rule: string | null;
  /** The CI run it came from, when there is one; `link` only for a run of GitHub (`runLink`). */
  run: { source: string; os: string | null; link: string | null } | null;
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function text(value: unknown): string | null {
  return typeof value === 'string' ? value : null;
}

/**
 * The findings of a `findings` document. The engine's documents are foreign: a finding that is not an object or has
 * no `fp` is left out, a field of another type is `null`, and nothing of a finding is ever read as markup or as a link
 * (except a CI run, by `runLink`).
 */
export function findingsOf(doc: { findings?: unknown }): Finding[] {
  if (!Array.isArray(doc.findings)) return [];
  const out: Finding[] = [];
  for (const raw of doc.findings as unknown[]) {
    const f = record(raw);
    const fp = f ? text(f['fp']) : null;
    if (!f || !fp) continue;
    const where = record(f['where']);
    const file = where ? text(where['file']) : null;
    const line = where && typeof where['line'] === 'number' && Number.isFinite(where['line']) ? where['line'] : null;
    const run = record(f['run']);
    const source = run ? text(run['source']) : null;
    out.push({
      fp,
      kind: text(f['kind']) ?? 'unknown',
      title: text(f['title']) ?? '',
      excerpt: text(f['excerpt']) ?? '',
      where: file !== null ? { file, line } : null,
      repro: text(f['repro']),
      rule: text(f['rule']),
      run: source !== null && run ? { source, os: text(run['os']), link: runLink(text(run['url'])) } : null,
    });
  }
  return out;
}

/** The red cases of a repository: those that failed in any way, in the order of the document. */
export function redCases(cases: readonly Case[]): Case[] {
  return cases.filter((c) => c.status !== 'pass' && c.status !== 'skip' && c.status !== 'xfail');
}
