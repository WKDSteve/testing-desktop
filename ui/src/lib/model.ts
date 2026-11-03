// What the views show of the documents of testing.nvim: pure functions of the documents, no state, no transport.
// Text of the engine (names, messages, outputs) is foreign: it is only ever shown as text, with the characters that
// hide or reorder what is read made visible (`shown`), and a link is built only to a CI run of GitHub (`runLink`).

import { markHidden } from 'appkit-ui/text';
import type { Case, Cell, Fleet, RemoteState, Row } from './contract';

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

const RED_STATES: readonly RemoteState[] = ['failure', 'rejected', 'cancelled'];

/** The tone of a state of the CI adapter. */
export function remoteTone(state: RemoteState | null | undefined): Tone {
  switch (state) {
    case 'success':
      return 'green';
    case 'failure':
    case 'rejected':
    case 'cancelled':
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

/** Foreign text as it may be shown: the hidden characters made visible; `null` stays `null`. */
export function shown(text: string | null | undefined): string | null {
  return typeof text === 'string' ? markHidden(text) : null;
}

/** The title of a case: its `describe` path and its name. */
export function caseTitle(c: Case): string {
  return markHidden([...c.describe, c.name ?? c.id].join(' › '));
}

/** The red cases of a repository: those that failed in any way, in the order of the document. */
export function redCases(cases: readonly Case[]): Case[] {
  return cases.filter((c) => c.status !== 'pass' && c.status !== 'skip' && c.status !== 'xfail');
}
