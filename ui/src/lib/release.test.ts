import { describe, expect, it } from 'vitest';
import {
  commitsFrom,
  doneFrom,
  filesFirstFlagged,
  isFullSha,
  isRepoName,
  mayNeedRelease,
  outcomeFrom,
  releaseCommand,
  reposFrom,
  reviewFrom,
  warningsOf,
  type Review,
} from './release';

// The answers of the hub's release assistant, folded: only what has the form the page can act on is taken, and a
// command is only ever made of a plain repository and a full commit id.

const SHA = '2222222222222222222222222222222222222222';
const PREV = '1111111111111111111111111111111111111111';

function review(over: Partial<Review> = {}): Review {
  return {
    sha: SHA,
    already: false,
    prev: PREV,
    compared: true,
    problem: null,
    status: 'ahead',
    totalCommits: 1,
    commits: [{ sha: SHA, author: 'Alice', title: 'feat' }],
    files: [
      { path: 'lua/a.lua', previous: null, status: 'modified', flag: null },
      { path: '.github/workflows/ci.yml', previous: null, status: 'modified', flag: 'workflow' },
      { path: 'README.md', previous: null, status: 'modified', flag: null },
      { path: 'scripts/x.sh', previous: 'x.sh', status: 'renamed', flag: 'script' },
    ],
    filesTotal: 4,
    truncated: false,
    ...over,
  };
}

describe('the answers of the release assistant', () => {
  it('a commit id is the full one and a repository is owner and name', () => {
    expect(isFullSha(SHA)).toBe(true);
    for (const bad of ['', '2222222', SHA.slice(1), `${SHA}2`, SHA.toUpperCase().replace(/2/g, 'A'), 'main', '--yes', ` ${SHA.slice(1)}`]) {
      expect(isFullSha(bad), bad).toBe(false);
    }
    for (const ok of ['WKDSteve/testing.nvim', 'o/r', 'a-b/c_d.e']) expect(isRepoName(ok), ok).toBe(true);
    for (const bad of ['', 'o', 'o/', '/r', 'o/r/x', '-o/r', 'o/-r', 'o/..', '../r', 'o/r --yes', 'o/r;x', 'o\\r', 'ö/r', 'o/r\n']) {
      expect(isRepoName(bad), JSON.stringify(bad)).toBe(false);
    }
  });

  it('takes the repositories that have a plain name, with their state', () => {
    const read = reposFrom({
      exit: 0,
      problem: null,
      repos: [
        { name: 'o/red', listed: false, mode: null, reviewed: 0, ci: 'failure', red: true },
        { name: 'o/listed', listed: true, mode: 'auto', reviewed: 3, ci: null, red: false },
        { name: 'not a repo', listed: true },
        'x',
      ],
    });
    expect(read.exit).toBe(0);
    expect(read.repos).toEqual([
      { name: 'o/red', listed: false, mode: null, reviewed: 0, ci: 'failure', red: true },
      { name: 'o/listed', listed: true, mode: 'auto', reviewed: 3, ci: null, red: false },
    ]);
    expect(reposFrom(null)).toEqual({ exit: null, problem: null, repos: [] });
  });

  it('takes only commits with a full id: nothing else can be released', () => {
    const read = commitsFrom({
      exit: 0,
      commits: [
        { sha: SHA, title: 'feat', author: 'Alice', date: '2026-11-07T10:00:00Z', released: false },
        { sha: 'abc1234', title: 'short' },
        { title: 'none' },
      ],
    });
    expect(read.commits).toEqual([{ sha: SHA, title: 'feat', author: 'Alice', date: '2026-11-07T10:00:00Z', released: false }]);
    expect(commitsFrom({ exit: 3, problem: 'HTTP 404' })).toEqual({ exit: 3, problem: 'HTTP 404', commits: [] });
  });

  it('a review aid is one of a full commit id, or none', () => {
    const read = reviewFrom({
      exit: 0,
      show: {
        sha: SHA,
        already: false,
        prev: PREV,
        compared: true,
        status: 'ahead',
        total_commits: 2,
        commits: [{ sha: SHA, author: 'Alice', title: 'feat' }],
        files: [{ path: 'a', status: 'added', flag: 'script' }],
        files_total: 1,
        truncated: false,
      },
    });
    expect(read.review).toMatchObject({ sha: SHA, prev: PREV, compared: true, totalCommits: 2, filesTotal: 1 });
    expect(read.review?.files).toEqual([{ path: 'a', previous: null, status: 'added', flag: 'script' }]);
    expect(reviewFrom({ exit: 3, problem: 'no such commit', show: null }).review).toBeNull();
    expect(reviewFrom({ exit: 0, show: { sha: 'abc1234' } }).review).toBeNull();
    expect(reviewFrom({ exit: 0 }).review).toBeNull();
  });

  it('says what of a review aid must not be overlooked', () => {
    expect(warningsOf(review())).toEqual(['flagged']);
    expect(warningsOf(review({ files: [] }))).toEqual([]);
    expect(warningsOf(review({ prev: null, compared: false, files: [] }))).toEqual(['no_prev']);
    expect(warningsOf(review({ compared: false, problem: 'HTTP 500', files: [] }))).toEqual(['not_compared']);
    expect(warningsOf(review({ status: 'diverged', truncated: true }))).toEqual(['not_ahead', 'truncated', 'flagged']);
    // A commit on the list was not compared: there is nothing to warn of.
    expect(warningsOf(review({ already: true, compared: false, prev: PREV }))).toEqual([]);
  });

  it('lists the files that deserve a close look first, each group in its order', () => {
    expect(filesFirstFlagged(review()).map((f) => f.path)).toEqual(['.github/workflows/ci.yml', 'scripts/x.sh', 'lua/a.lua', 'README.md']);
  });

  it('reads what a release did, also one whose re-run failed', () => {
    const read = doneFrom({
      exit: 3,
      problem: 're-run 71 failed: HTTP 403',
      release: { already: false, recorded: true, reruns: [{ id: 71, name: 'ci', was: 'failure', ok: false, problem: 'HTTP 403' }] },
    });
    expect(read.exit).toBe(3);
    expect(read.done).toEqual({ already: false, recorded: true, reruns: [{ id: 71, name: 'ci', was: 'failure', ok: false, problem: 'HTTP 403' }] });
    expect(doneFrom({ exit: 2, problem: 'usage', release: null }).done).toBeNull();
  });

  it('reads where a release stands: pending, done with what it did, failed with why, or nothing', () => {
    expect(outcomeFrom({ state: 'pending', repo: 'o/r', sha: SHA })).toMatchObject({ state: 'pending', repo: 'o/r', sha: SHA, done: null, error: null });
    const done = outcomeFrom({ state: 'done', repo: 'o/r', sha: SHA, exit: 0, problem: null, release: { already: true, recorded: true, reruns: [] } });
    expect(done).toMatchObject({ state: 'done', exit: 0, done: { already: true, recorded: true, reruns: [] } });
    expect(outcomeFrom({ state: 'failed', error: { code: 'engine_spawn', message: 'the engine could not be started' } }).error).toBe('engine_spawn: the engine could not be started');
    for (const doc of [null, {}, { state: 'none' }, { state: 'released' }, { state: 7 }]) expect(outcomeFrom(doc).state).toBe('none');
    // A document that says done without what was done is no release.
    expect(outcomeFrom({ state: 'done', exit: 0 }).done).toBeNull();
  });

  it('makes the command of a terminal only of a plain repository and a full commit id', () => {
    expect(releaseCommand('WKDSteve/lib.nvim', SHA)).toBe(`testing ci release WKDSteve/lib.nvim ${SHA}`);
    for (const [repo, sha] of [
      ['WKDSteve/lib.nvim', 'abc1234'],
      ['WKDSteve/lib.nvim', null],
      [null, SHA],
      ['o/r; rm -rf ~', SHA],
      ['o/r && x', SHA],
      ['o/$(x)', SHA],
      ['o/r', `${SHA.slice(0, 39)};`],
      ['--yes/x', SHA],
    ] as [string | null, string | null][]) {
      expect(releaseCommand(repo, sha), `${repo} ${sha}`).toBeNull();
    }
  });

  it('a refused job can stand behind a run that failed or was cancelled, and behind no other state', () => {
    expect(['failure', 'cancelled'].map(mayNeedRelease)).toEqual([true, true]);
    for (const state of ['rejected', 'no_runner', 'success', 'running', 'queued', 'unknown', null, undefined]) {
      expect(mayNeedRelease(state), String(state)).toBe(false);
    }
  });
});
