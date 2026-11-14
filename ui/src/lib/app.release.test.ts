import { describe, expect, it } from 'vitest';
import fleetJson from '../../../crates/testing-hub/tests/fixtures/golden/fleet.json';
import { App, TOKEN_KEY, type StorageLike } from './app.svelte';
import { HubSource } from './source';

// The release assistant on the page: its steps are requests to the hub with exactly the repository and the full
// commit id a person chose, the way to the release leads through the review aid and the question, and what the hub
// or the engine refuses is said and changes nothing.

const S1 = '1111111111111111111111111111111111111111';
const S2 = '2222222222222222222222222222222222222222';
const RED = 'fixture-owner/beta.nvim';

class Memory implements StorageLike {
  map = new Map<string, string>();
  getItem(k: string): string | null {
    return this.map.get(k) ?? null;
  }
  setItem(k: string, v: string): void {
    this.map.set(k, v);
  }
  removeItem(k: string): void {
    this.map.delete(k);
  }
}

interface Options {
  /** The answer to `release/show`; a function sees the sha asked. */
  show?: (sha: string) => unknown;
  /** The answer to `POST release`, with its status, when the hub refuses to start it. */
  release?: { status: number; body: unknown };
  /** What `release/outcome` says once the release ended (after one `pending`); the default is a good release. */
  outcome?: Record<string, unknown>;
  /** Hold the answer to `release/commits` until this is resolved. */
  holdCommits?: Promise<void>;
}

function hub(options: Options = {}) {
  const asked: string[] = [];
  const posts: { path: string; body: unknown }[] = [];
  let started: { repo: string; sha: string } | null = null;
  let outcomeReads = 0;
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
  const show = (sha: string) => ({
    schema: 1,
    kind: 'hub.release.show',
    exit: 0,
    problem: null,
    repo: RED,
    sha,
    show: {
      sha,
      already: false,
      prev: S1,
      compared: true,
      status: 'ahead',
      total_commits: 1,
      commits: [{ sha, author: 'Alice', title: 'feat: the newest' }],
      files: [{ path: '.github/workflows/ci.yml', status: 'modified', flag: 'workflow' }],
      files_total: 1,
      truncated: false,
    },
  });
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    if (path === '/api/testing/events') return new Promise<Response>(() => {});
    if (path === '/api/testing/hello') return json({ instance: 'i', version: '0', engine: { ready: true, problem: null } });
    if (path === '/api/testing/fleet') return json(fleetJson);
    if (path === '/api/testing/about') return json({ app: 'testing-desktop', version: '0' });
    if (path === '/api/testing/sheet-step') return json({ schema: 1, available: true, session: null, running: false, seq: 0, events: [] });
    if (init?.method === 'POST') {
      posts.push({ path, body: JSON.parse(String(init.body)) });
      if (path === '/api/testing/release') {
        if (options.release) return json(options.release.body, options.release.status);
        started = JSON.parse(String(init.body)) as { repo: string; sha: string };
        return json({ schema: 1, kind: 'hub.release.started', ...started }, 202);
      }
      return json({ error: { code: 'nope', message: 'nope' } }, 404);
    }
    asked.push(path);
    if (path === '/api/testing/release/outcome') {
      if (started === null) return json({ schema: 1, kind: 'hub.release.outcome', state: 'none' });
      outcomeReads += 1;
      // It runs for a moment: the first look finds it pending.
      if (outcomeReads === 1) return json({ schema: 1, kind: 'hub.release.outcome', state: 'pending', ...started });
      return json({
        schema: 1,
        kind: 'hub.release.outcome',
        state: 'done',
        ...started,
        exit: 0,
        problem: null,
        release: { already: false, recorded: true, reruns: [{ id: 71, name: 'ci', was: 'failure', ok: true }] },
        ...options.outcome,
      });
    }
    if (path === '/api/testing/release/repos') {
      return json({
        schema: 1,
        kind: 'hub.release.repos',
        exit: 0,
        problem: null,
        repos: [
          { name: RED, listed: false, mode: null, reviewed: 0, ci: 'failure', red: true },
          { name: 'fixture-owner/alpha.nvim', listed: true, mode: 'auto', reviewed: 2, ci: 'success', red: false },
        ],
      });
    }
    if (path.startsWith('/api/testing/release/commits?repo=')) {
      if (options.holdCommits) await options.holdCommits;
      return json({
        schema: 1,
        kind: 'hub.release.commits',
        exit: 0,
        problem: null,
        commits: [
          { sha: S2, title: 'feat: the newest', author: 'Alice', date: '2026-11-07T10:00:00Z', released: false },
          { sha: S1, title: 'fix: the one before', author: 'Bob', date: '2026-11-06T10:00:00Z', released: true },
        ],
      });
    }
    if (path.startsWith('/api/testing/release/show?')) {
      const sha = new URL(path, 'http://x').searchParams.get('sha') ?? '';
      return json((options.show ?? show)(sha));
    }
    return json({ error: { code: 'nope', message: 'nope' } }, 404);
  }) as typeof fetch;
  return { fetchFn, asked, posts };
}

async function started(options: Options = {}) {
  const h = hub(options);
  const session = new Memory();
  session.setItem(TOKEN_KEY, 't');
  const app = new App({ hub: new HubSource(h.fetchFn), session, sleep: () => Promise.resolve() });
  await app.start(null);
  const releaseAsked = () => h.asked.filter((p) => p.includes('/release'));
  return { ...h, app, releaseAsked };
}

/** Wait until the assistant is not asking any more. */
async function settled(app: App): Promise<void> {
  for (let i = 0; i < 50 && (app.release.busy || app.release.repos === null); i++) await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe('the release assistant on the page', () => {
  it('walks repository, commit, review aid, question and release, and sends exactly what was chosen', async () => {
    const { app, posts, releaseAsked } = await started();
    app.showRelease();
    expect(app.releaseView).toBe(true);
    await settled(app);
    expect(app.release.stage).toBe('repo');
    expect(app.release.repos?.map((r) => r.name)).toEqual([RED, 'fixture-owner/alpha.nvim']);

    await app.releaseChooseRepo(RED);
    expect(app.release.stage).toBe('commit');
    expect(app.release.commits?.map((c) => c.released)).toEqual([false, true]);

    await app.releaseChooseCommit(S2);
    expect(app.release.stage).toBe('review');
    expect(app.release.review?.files[0]?.flag).toBe('workflow');

    // Not on to the question before the person says they looked.
    app.releaseToConfirm();
    expect(app.release.stage).toBe('review');
    await app.releaseDo();
    expect(posts).toEqual([]);
    app.releaseLooked(true);
    app.releaseToConfirm();
    expect(app.release.stage).toBe('confirm');

    await app.releaseDo();
    expect(posts).toEqual([{ path: '/api/testing/release', body: { repo: RED, sha: S2 } }]);
    expect(app.release.stage).toBe('done');
    expect(app.release.done).toMatchObject({ recorded: true, reruns: [{ id: 71, ok: true }] });
    expect(app.release.doneProblem).toBeNull();
    // The hub answers the start at once: the page asks what became of it until it is not pending any more.
    expect(releaseAsked()).toEqual([
      '/api/testing/release/repos',
      `/api/testing/release/commits?repo=${encodeURIComponent(RED)}`,
      `/api/testing/release/show?repo=${encodeURIComponent(RED)}&sha=${S2}`,
      '/api/testing/release/outcome',
      '/api/testing/release/outcome',
    ]);
    app.stop();
  });

  it('opens at the commits of the repository of a red run, when the assistant offers it, whatever its case', async () => {
    const { app } = await started();
    app.showRelease('Fixture-Owner/Beta.nvim');
    await settled(app);
    expect(app.release.stage).toBe('commit');
    expect(app.release.repo).toBe(RED);
    app.showRelease('someone/else');
    await settled(app);
    expect(app.release.stage).toBe('repo');
    expect(app.release.repo).toBeNull();
    app.stop();
  });

  it('takes no repository the assistant did not offer and no commit id that is not the full one', async () => {
    const { app, releaseAsked } = await started();
    app.showRelease();
    await settled(app);
    await app.releaseChooseRepo('someone/else');
    expect(app.release.stage).toBe('repo');
    await app.releaseChooseRepo(RED);
    for (const sha of ['2222222', 'main', '', S2.toUpperCase().replace(/2/g, 'A')]) await app.releaseChooseCommit(sha);
    expect(app.release.stage).toBe('commit');
    expect(releaseAsked().filter((p) => p.includes('/show'))).toEqual([]);
    app.stop();
  });

  it('an aid for another commit than the one chosen is no aid, and what the engine says is shown', async () => {
    const other = '9'.repeat(40);
    const moved = await started({ show: () => ({ exit: 0, problem: null, show: { sha: other, already: false, prev: S1, compared: true, commits: [], files: [], files_total: 0, truncated: false } }) });
    moved.app.showRelease(RED);
    await settled(moved.app);
    await moved.app.releaseChooseCommit(S2);
    expect(moved.app.release.review).toBeNull();
    expect(moved.app.release.failure).toContain('no review aid');
    moved.app.releaseLooked(true);
    moved.app.releaseToConfirm();
    expect(moved.app.release.stage).toBe('review');
    moved.app.stop();

    const gone = await started({ show: () => ({ exit: 3, problem: 'o/r has no commit: HTTP 404', show: null }) });
    gone.app.showRelease(RED);
    await settled(gone.app);
    await gone.app.releaseChooseCommit(S2);
    expect(gone.app.release.failure).toBe('o/r has no commit: HTTP 404');
    gone.app.stop();
  });

  it('a release the hub refuses leaves the question open and says why', async () => {
    const { app, posts } = await started({ release: { status: 409, body: { error: { code: 'not_shown', message: 'the review aid of this commit was not read just now' } } } });
    app.showRelease(RED);
    await settled(app);
    await app.releaseChooseCommit(S2);
    app.releaseLooked(true);
    app.releaseToConfirm();
    await app.releaseDo();
    expect(posts).toHaveLength(1);
    expect(app.release.stage).toBe('confirm');
    expect(app.release.failure).toBe('not_shown: the review aid of this commit was not read just now');
    expect(app.release.done).toBeNull();
    app.stop();
  });

  it('a release that is recorded while a re-run failed says both', async () => {
    const { app } = await started({
      outcome: { exit: 3, problem: 're-run 71 failed: HTTP 403', release: { already: false, recorded: true, reruns: [{ id: 71, name: 'ci', was: 'failure', ok: false, problem: 'HTTP 403' }] } },
    });
    app.showRelease(RED);
    await settled(app);
    await app.releaseChooseCommit(S2);
    app.releaseLooked(true);
    app.releaseToConfirm();
    await app.releaseDo();
    expect(app.release.stage).toBe('done');
    expect(app.release.done?.recorded).toBe(true);
    expect(app.release.doneProblem).toBe('re-run 71 failed: HTTP 403');
    app.stop();
  });

  it('a release that gave no document, or whose outcome is another release, is not shown as released', async () => {
    const walk = async (options: Options) => {
      const s = await started(options);
      s.app.showRelease(RED);
      await settled(s.app);
      await s.app.releaseChooseCommit(S2);
      s.app.releaseLooked(true);
      s.app.releaseToConfirm();
      await s.app.releaseDo();
      return s.app;
    };
    const failed = await walk({ outcome: { state: 'failed', release: undefined, error: { code: 'engine_timeout', message: 'the engine took too long' } } });
    expect([failed.release.stage, failed.release.done]).toEqual(['done', null]);
    expect(failed.release.doneProblem).toBe('engine_timeout: the engine took too long');
    failed.stop();

    // Another tab released another commit meanwhile: its outcome is not this page's result.
    const other = await walk({ outcome: { sha: S1 } });
    expect([other.release.stage, other.release.done]).toEqual(['done', null]);
    expect(other.release.doneProblem).toContain('does not say what became of this release');
    other.stop();
  });

  it('goes back one step at a time and forgets what the step left behind chose', async () => {
    const { app } = await started();
    app.showRelease(RED);
    await settled(app);
    await app.releaseChooseCommit(S2);
    app.releaseLooked(true);
    app.releaseToConfirm();
    app.releaseBack();
    expect([app.release.stage, app.release.looked]).toEqual(['review', true]);
    app.releaseBack();
    expect([app.release.stage, app.release.sha, app.release.review, app.release.looked]).toEqual(['commit', null, null, false]);
    app.releaseBack();
    expect([app.release.stage, app.release.repo, app.release.commits]).toEqual(['repo', null, null]);
    app.stop();
  });

  it('does not take a second step while one is on its way, and drops an answer the person left behind', async () => {
    let let_go: () => void = () => {};
    const { app, releaseAsked } = await started({ holdCommits: new Promise<void>((resolve) => (let_go = resolve)) });
    app.showRelease();
    await settled(app);
    const reading = app.releaseChooseRepo(RED);
    expect(app.release.busy).toBe(true);
    // A click while the commits are read does nothing.
    await app.releaseChooseCommit(S2);
    app.releaseBack();
    expect(app.release.stage).toBe('commit');
    expect(releaseAsked().filter((p) => p.includes('/show'))).toEqual([]);
    // The person opens the assistant anew: the answer of the old step is not taken.
    app.showRelease();
    let_go();
    await reading;
    await settled(app);
    expect(app.release.stage).toBe('repo');
    expect(app.release.commits).toBeNull();
    app.stop();
  });

  it('leaves the stepped run and the assistant to one view at a time', async () => {
    const { app } = await started();
    app.showRelease();
    expect([app.releaseView, app.stepView]).toEqual([true, false]);
    app.showStep();
    expect([app.releaseView, app.stepView]).toEqual([false, true]);
    app.showRelease();
    app.hideRelease();
    expect(app.releaseView).toBe(false);
    app.stop();
  });
});
