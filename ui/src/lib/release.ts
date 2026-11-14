// The release assistant for the runner guard, as the page holds it: the steps of testing.nvim's `testing ci release`
// (repository, commit, what changed, the release), each an answer of the hub (`release/*`), folded into one state.
// The page decides nothing about a release: it shows what the engine says and sends the commit a person chose and
// looked at. Nothing here touches a transport or the DOM.

export interface ReleaseRepo {
  /** `owner/name`. */
  name: string;
  /** On the list of reviewed commits of the runner guard. */
  listed: boolean;
  /** The mode of the guard for it (`auto`, `always`, `off`); `null`: not on the list. */
  mode: string | null;
  /** How many of its commits are released. */
  reviewed: number;
  /** The state of its CI in the fleet; `null`: not a repository of the fleet. */
  ci: string | null;
  /** Its CI has a run that failed: where a refused job shows. */
  red: boolean;
}

export interface ReleaseCommit {
  sha: string;
  title: string;
  author: string;
  date: string;
  released: boolean;
}

export interface ReviewFile {
  path: string;
  previous: string | null;
  status: string;
  /** Why the file deserves a close look (the workflow, a script, a lock file); `null`: nothing special. */
  flag: string | null;
}

/** The review aid of one commit: what changed since the last reviewed one. */
export interface Review {
  sha: string;
  /** On the list already: nothing was compared. */
  already: boolean;
  /** The last reviewed commit; `null`: none, the whole tree is new to the runner. */
  prev: string | null;
  compared: boolean;
  problem: string | null;
  /** How the commit lies to the last reviewed one; anything but `ahead` is a rewritten history or another branch. */
  status: string | null;
  totalCommits: number | null;
  commits: { sha: string; author: string; title: string }[];
  files: ReviewFile[];
  filesTotal: number;
  /** GitHub lists at most 300 files. */
  truncated: boolean;
}

export interface Rerun {
  id: number;
  name: string;
  was: string;
  ok: boolean;
  problem: string | null;
}

/** What a release did. */
export interface Done {
  already: boolean;
  recorded: boolean;
  reruns: Rerun[];
}

export type Stage = 'repo' | 'commit' | 'review' | 'confirm' | 'done';

export interface Wizard {
  stage: Stage;
  /** A step is on its way to the hub. */
  busy: boolean;
  /** Why the last step gave nothing: the hub's refusal, or the engine's `problem`. */
  failure: string | null;
  repos: ReleaseRepo[] | null;
  repo: string | null;
  commits: ReleaseCommit[] | null;
  sha: string | null;
  review: Review | null;
  /** The person says they looked at the changes: without it there is no way on to the release. */
  looked: boolean;
  done: Done | null;
  /** The engine's `problem` of the release (a re-run that failed, the runs that could not be read). */
  doneProblem: string | null;
  /**
   * The release was started, but the page could not learn what became of it (the hub did not answer, or answers for
   * another release): it may be on the list or not, and the page must not say either.
   */
  doneUnknown: boolean;
}

export function emptyWizard(): Wizard {
  return {
    stage: 'repo',
    busy: false,
    failure: null,
    repos: null,
    repo: null,
    commits: null,
    sha: null,
    review: null,
    looked: false,
    done: null,
    doneProblem: null,
    doneUnknown: false,
  };
}

const str = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const obj = (v: unknown): Record<string, unknown> | null =>
  typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
const list = (v: unknown): Record<string, unknown>[] =>
  (Array.isArray(v) ? v : []).map((x) => obj(x)).filter((x): x is Record<string, unknown> => x !== null);

/** A full commit id, as the hub takes one for a release: 40 hex digits, lower case. */
export function isFullSha(sha: string): boolean {
  return /^[0-9a-f]{40}$/.test(sha);
}

/** A repository as the hub takes one: `owner/name` of plain characters. */
export function isRepoName(name: string): boolean {
  return name.length <= 140 && /^[A-Za-z0-9_.][A-Za-z0-9_.-]{0,99}\/[A-Za-z0-9_.][A-Za-z0-9_.-]{0,99}$/.test(name) && !/(^|\/)\.{1,2}($|\/)/.test(name);
}

/** What every answer of a step says first: the engine's exit code and its problem. */
export interface StepHead {
  exit: number | null;
  problem: string | null;
}

function headOf(doc: unknown): StepHead {
  const d = obj(doc);
  return { exit: num(d?.['exit']), problem: str(d?.['problem']) };
}

export function reposFrom(doc: unknown): StepHead & { repos: ReleaseRepo[] } {
  const repos = list(obj(doc)?.['repos'])
    .map((r) => ({
      name: str(r['name']) ?? '',
      listed: r['listed'] === true,
      mode: str(r['mode']),
      reviewed: num(r['reviewed']) ?? 0,
      ci: str(r['ci']),
      red: r['red'] === true,
    }))
    .filter((r) => isRepoName(r.name));
  return { ...headOf(doc), repos };
}

export function commitsFrom(doc: unknown): StepHead & { commits: ReleaseCommit[] } {
  const commits = list(obj(doc)?.['commits'])
    .map((c) => ({
      sha: str(c['sha']) ?? '',
      title: str(c['title']) ?? '',
      author: str(c['author']) ?? '',
      date: str(c['date']) ?? '',
      released: c['released'] === true,
    }))
    .filter((c) => isFullSha(c.sha));
  return { ...headOf(doc), commits };
}

export function reviewFrom(doc: unknown): StepHead & { review: Review | null } {
  const show = obj(obj(doc)?.['show']);
  const sha = str(show?.['sha']);
  if (!show || sha === null || !isFullSha(sha)) return { ...headOf(doc), review: null };
  const files = list(show['files']).map((f) => ({
    path: str(f['path']) ?? '',
    previous: str(f['previous']),
    status: str(f['status']) ?? '',
    flag: str(f['flag']),
  }));
  return {
    ...headOf(doc),
    review: {
      sha,
      already: show['already'] === true,
      prev: str(show['prev']),
      compared: show['compared'] === true,
      problem: str(show['problem']),
      status: str(show['status']),
      totalCommits: num(show['total_commits']),
      commits: list(show['commits']).map((c) => ({ sha: str(c['sha']) ?? '', author: str(c['author']) ?? '', title: str(c['title']) ?? '' })),
      files,
      filesTotal: num(show['files_total']) ?? files.length,
      truncated: show['truncated'] === true,
    },
  };
}

export function doneFrom(doc: unknown): StepHead & { done: Done | null } {
  const release = obj(obj(doc)?.['release']);
  if (!release) return { ...headOf(doc), done: null };
  return {
    ...headOf(doc),
    done: {
      already: release['already'] === true,
      recorded: release['recorded'] === true,
      reruns: list(release['reruns']).map((r) => ({
        id: num(r['id']) ?? 0,
        name: str(r['name']) ?? '',
        was: str(r['was']) ?? '',
        ok: r['ok'] === true,
        problem: str(r['problem']),
      })),
    },
  };
}

/** Where the release that was started last stands (`release/outcome`). */
export type OutcomeState = 'none' | 'pending' | 'done' | 'failed';

/**
 * The outcome of a release: while it is `pending` the page asks again; `done` carries what the engine did (and its
 * `exit` and `problem`), `failed` why the step gave nothing. Anything else the hub says is `none`.
 */
export function outcomeFrom(doc: unknown): StepHead & { state: OutcomeState; repo: string | null; sha: string | null; done: Done | null; error: string | null } {
  const d = obj(doc);
  const word = str(d?.['state']);
  const state: OutcomeState = word === 'pending' || word === 'done' || word === 'failed' ? word : 'none';
  const error = obj(d?.['error']);
  return {
    ...headOf(doc),
    state,
    repo: str(d?.['repo']),
    sha: str(d?.['sha']),
    done: state === 'done' ? doneFrom(doc).done : null,
    error: state === 'failed' ? `${str(error?.['code']) ?? 'failed'}: ${str(error?.['message']) ?? ''}` : null,
  };
}

/** What of a review aid a person must not overlook, most serious first. */
export type ReviewWarning = 'not_compared' | 'not_ahead' | 'no_prev' | 'truncated' | 'flagged';

export function warningsOf(review: Review): ReviewWarning[] {
  if (review.already) return [];
  const out: ReviewWarning[] = [];
  if (review.prev === null) out.push('no_prev');
  else if (!review.compared) out.push('not_compared');
  if (review.compared && review.status !== null && review.status !== 'ahead') out.push('not_ahead');
  if (review.truncated) out.push('truncated');
  if (review.files.some((f) => f.flag !== null)) out.push('flagged');
  return out;
}

/** The files of a review aid, those that deserve a close look first, each group as GitHub listed it. */
export function filesFirstFlagged(review: Review): ReviewFile[] {
  return [...review.files.filter((f) => f.flag !== null), ...review.files.filter((f) => f.flag === null)];
}

/** The commit id as a person reads it. */
export function shortSha(sha: string): string {
  return sha.slice(0, 12);
}

/**
 * The command of a terminal that releases this commit, for a page that cannot (the web variant) or a person who would
 * rather type it; `null` when the repository or the commit id is not plain: nothing else is ever put into a command.
 */
export function releaseCommand(repo: string | null | undefined, sha: string | null | undefined): string | null {
  if (!repo || !sha || !isRepoName(repo) || !isFullSha(sha)) return null;
  return `testing ci release ${repo} ${sha}`;
}

/** The states of a CI run behind which a job the runner guard refused can stand: the run failed, or was cancelled. */
export function mayNeedRelease(state: string | null | undefined): boolean {
  return state === 'failure' || state === 'cancelled';
}
