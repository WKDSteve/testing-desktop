import { describe, expect, it } from 'vitest';
import fleetJson from '../../../crates/testing-hub/tests/fixtures/golden/fleet.json';
import repoJson from '../../../crates/testing-hub/tests/fixtures/golden/repo.json';
import { App, RUN_POLL_MS, TOKEN_KEY, type StorageLike } from './app.svelte';
import { fileTextProblem, runFrom } from './run';
import { HubSource } from './source';

// A run through the engine on the page: a start is a POST with exactly the body the hub takes, the page follows the
// run until it ends and then reads the repository again; a file text the hub would refuse is not sent.

const KEY = 'alpha.nvim-356bf237c7fc';

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

function hub() {
  const posts: { path: string; body: unknown }[] = [];
  let repoReads = 0;
  let polls = 0;
  let run: Record<string, unknown> | null = null;
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    if (path === '/api/testing/events') return new Promise<Response>(() => {});
    if (path === '/api/testing/hello') return json({ instance: 'i', version: '0', engine: { ready: true, problem: null } });
    if (path === '/api/testing/fleet') return json(fleetJson);
    if (path.startsWith('/api/testing/repo?key=')) {
      repoReads += 1;
      return json(repoJson);
    }
    if (path.startsWith('/api/testing/finding?key=')) return json({ schema: 'testing-export/1', kind: 'findings', findings: [] });
    if (path.startsWith('/api/testing/sheet-step/sheets?key=')) return json({ schema: 1, kind: 'hub.sheets', sheets: [] });
    if (path === '/api/testing/run' && init?.method !== 'POST') {
      polls += 1;
      // The run ends at the second look.
      if (run && polls >= 2) run = { ...run, running: false, exit: 1, verdict: 'red', output: '<path>/x_spec.lua:3: red' };
      return json({ schema: 1, kind: 'hub.run', available: true, run });
    }
    if (path === '/api/testing/run/start' && init?.method === 'POST') {
      const body = JSON.parse(String(init.body)) as { key: string; file?: string };
      posts.push({ path, body });
      if (run?.['running']) return json({ error: { code: 'busy', message: 'a run (suite) of x is under way' } }, 409);
      run = { id: 1, key: body.key, what: body.file ? 'file' : 'suite', file: body.file ?? null, running: true, exit: null, verdict: null, output: '' };
      polls = 0;
      return json({ schema: 1, kind: 'hub.run', available: true, run }, 202);
    }
    return json({ error: { code: 'nope', message: 'nope' } }, 404);
  }) as typeof fetch;
  return { fetchFn, posts, repoReads: () => repoReads, setRun: (r: Record<string, unknown> | null) => (run = r) };
}

async function opened() {
  const h = hub();
  const session = new Memory();
  session.setItem(TOKEN_KEY, 't');
  const sleeps: number[] = [];
  const app = new App({
    hub: new HubSource(h.fetchFn),
    session,
    sleep: (ms) => {
      sleeps.push(ms);
      return Promise.resolve();
    },
  });
  await app.start(null);
  await app.open(KEY);
  return { ...h, app, sleeps };
}

describe('a run through the engine on the page', () => {
  it('starts the suite of the open repository and follows it to its verdict', async () => {
    const { app, posts, repoReads, sleeps } = await opened();
    const before = repoReads();
    await app.runStart();
    expect(posts).toEqual([{ path: '/api/testing/run/start', body: { key: KEY } }]);
    // The follower runs on its own; let it finish.
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(app.run?.running).toBe(false);
    expect(app.run?.verdict).toBe('red');
    expect(app.run?.output).toBe('<path>/x_spec.lua:3: red');
    expect(sleeps.every((ms) => ms === RUN_POLL_MS)).toBe(true);
    expect(repoReads()).toBeGreaterThan(before);
  });

  it('sends a file text only when the hub would take it, and trimmed', async () => {
    const { app, posts } = await opened();
    await app.runStart('../etc');
    await app.runStart('--trust');
    await app.runStart('   ');
    expect(posts).toEqual([]);
    await app.runStart('  config ');
    expect(posts).toEqual([{ path: '/api/testing/run/start', body: { key: KEY, file: 'config' } }]);
  });

  it('says why a start was refused', async () => {
    const { app, setRun } = await opened();
    setRun({ id: 9, key: 'other-0123456789ab', what: 'suite', running: true });
    await app.runStart();
    expect(app.runFailure).toContain('under way');
  });

  it('learns of a run under way when a repository is opened', async () => {
    const h = hub();
    h.setRun({ id: 3, key: KEY, what: 'task', task: 'alpha.nvim/try-it', running: true });
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub: new HubSource(h.fetchFn), session, sleep: () => Promise.resolve() });
    await app.start(null);
    await app.open(KEY);
    expect(app.run?.what).toBe('task');
    expect(app.run?.task).toBe('alpha.nvim/try-it');
  });
});

describe('the state of a run', () => {
  it('takes what is of its form and leaves out the rest', () => {
    expect(runFrom(null)).toEqual({ available: false, run: null });
    expect(runFrom({ available: true, run: null })).toEqual({ available: true, run: null });
    const s = runFrom({ available: true, run: { id: 2, key: 'k', what: 'weird', running: false, exit: 0, verdict: 'green', output: 7 } });
    expect(s.run).toEqual({ id: 2, key: 'k', what: 'other', file: null, task: null, running: false, verdict: 'green', exit: 0, output: '', block: null });
    // What became of the task's block: the engine's word and whether it stands; nothing that is not a plain word.
    const block = (b: unknown, running = false) => runFrom({ run: { what: 'task', running, block: b } }).run?.block;
    expect(block({ outcome: 'conflict', written: false })).toEqual({ outcome: 'conflict', written: false });
    expect(block({ outcome: 'changed', written: true })).toEqual({ outcome: 'changed', written: true });
    expect(block({ outcome: 'changed', written: 'yes' })).toEqual({ outcome: 'changed', written: false });
    for (const bad of [null, 'changed', [], { outcome: '<b>' }, { outcome: 7 }, { written: true }]) expect(block(bad)).toBeNull();
    expect(block({ outcome: 'changed', written: true }, true)).toBeNull();
    // A verdict of a run that still runs is none.
    expect(runFrom({ run: { running: true, verdict: 'green' } }).run?.verdict).toBeNull();
    expect(runFrom({ run: { running: false, verdict: 'purple' } }).run?.verdict).toBeNull();
  });

  it('checks a file text as the hub does', () => {
    expect(fileTextProblem('config')).toBeNull();
    expect(fileTextProblem('sub/dir_spec.lua')).toBeNull();
    expect(fileTextProblem('')).toBe('empty');
    expect(fileTextProblem('x'.repeat(101))).toBe('too_long');
    for (const bad of ['-x', '.x', '/x', 'a/../b', 'a b', 'a;b', 'ä']) expect(fileTextProblem(bad), bad).toBe('not_plain');
  });
});
