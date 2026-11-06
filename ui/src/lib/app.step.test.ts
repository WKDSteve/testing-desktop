import { describe, expect, it } from 'vitest';
import fleetJson from '../../../crates/testing-hub/tests/fixtures/golden/fleet.json';
import repoJson from '../../../crates/testing-hub/tests/fixtures/golden/repo.json';
import { App, TOKEN_KEY, type StorageLike } from './app.svelte';
import { HubSource } from './source';

// The stepped run on the page: what the hub answers and what the stream brings become one state; a start and a
// command are POSTs with exactly the body the hub takes; a line of another run reads the state whole.

const SCHEMA = 'testing-sheet-session/1';
const ev = (event: string, fields: Record<string, unknown> = {}) => ({ schema: SCHEMA, event, ...fields });
const START = ev('start', {
  id: 'ui.float',
  title: ':UI option float',
  engine: 'rpc',
  sizes: [{ rows: 24, cols: 80 }],
  steps: [
    { n: 1, action: 'keys', value: ':UI<CR>' },
    { n: 2, action: 'expect', value: 'x' },
  ],
  headed: false,
  delay_ms: 0,
  breaks: [],
});

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

interface Post {
  path: string;
  body: unknown;
}

/** A hub with a stepped run: its state document is what the tests make it; every POST is kept. */
function hub() {
  const posts: Post[] = [];
  let state: Record<string, unknown> = { schema: 1, kind: 'hub.sheet_step', available: true, session: null, running: false, seq: 0, key: null, sheet: null, events: [] };
  let stateReads = 0;
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    if (path === '/api/testing/events') return new Promise<Response>(() => {});
    if (path === '/api/testing/hello') return json({ instance: 'i', version: '0', engine: { ready: true, problem: null } });
    if (path === '/api/testing/fleet') return json(fleetJson);
    if (path.startsWith('/api/testing/repo?key=')) return json(repoJson);
    if (path.startsWith('/api/testing/finding?key=')) return json({ schema: 'testing-export/1', kind: 'findings', findings: [] });
    if (path.startsWith('/api/testing/sheet-step/sheets?key=')) return json({ schema: 1, kind: 'hub.sheets', sheets: ['float', 'smoke'] });
    if (path === '/api/testing/sheet-step') {
      stateReads += 1;
      return json(state);
    }
    if (init?.method === 'POST') {
      posts.push({ path, body: JSON.parse(String(init.body)) });
      if (path === '/api/testing/sheet-step/start') {
        const body = JSON.parse(String(init.body)) as { sheet: string };
        if (body.sheet === 'refused') return json({ error: { code: 'refused', message: 'refused on the workstation', exit: 2 } }, 422);
        state = { ...state, session: 7, running: true, seq: 2, key: 'alpha.nvim-356bf237c7fc', sheet: body.sheet, events: [START, ev('size', { size: '80x24', state: 'open' })] };
        return json(state);
      }
      if (path === '/api/testing/sheet-step/command') return json(state);
    }
    return json({ error: { code: 'nope', message: 'nope' } }, 404);
  }) as typeof fetch;
  return { fetchFn, posts, stateReads: () => stateReads, setState: (s: Record<string, unknown>) => (state = s) };
}

async function started() {
  const h = hub();
  const session = new Memory();
  session.setItem(TOKEN_KEY, 't');
  const app = new App({ hub: new HubSource(h.fetchFn), session, sleep: () => Promise.resolve() });
  await app.start(null);
  return { ...h, app };
}

const line = (session: number, seq: number, event: unknown) => ({ event: 'sheet-step', data: JSON.stringify({ session, seq, event }) });

describe('the stepped run on the page', () => {
  it('lists the sheets of the open repository and starts one with its key and name', async () => {
    const { app, posts } = await started();
    expect(app.hasStep).toBe(true);
    await app.open(app.fleet!.repos[0]!.key);
    expect(app.sheets).toEqual(['float', 'smoke']);
    await app.stepSheet('float');
    expect(posts).toEqual([{ path: '/api/testing/sheet-step/start', body: { key: app.fleet!.repos[0]!.key, sheet: 'float' } }]);
    expect(app.stepView).toBe(true);
    expect(app.step.session).toBe(7);
    expect(app.step.running).toBe(true);
    expect(app.step.doc?.title).toBe(':UI option float');
    expect(app.step.sheet).toBe('float');
    expect(app.stepFailure).toBeNull();
    app.stop();
  });

  it('says why a start was refused, and the words of the hub are shown as text', async () => {
    const { app } = await started();
    await app.open(app.fleet!.repos[0]!.key);
    await app.stepSheet('refused');
    expect(app.stepFailure).toBe('refused: refused on the workstation');
    expect(app.step.session).toBeNull();
    app.stop();
  });

  it('takes the lines of the stream after the state it holds, and sends the commands the hub takes', async () => {
    const { app, posts } = await started();
    await app.open(app.fleet!.repos[0]!.key);
    await app.stepSheet('float');
    posts.length = 0;
    // A line the state already holds changes nothing; a later one does.
    await app.onMessage(line(7, 2, ev('size', { size: '80x24', state: 'open' })));
    expect(app.step.seq).toBe(2);
    await app.onMessage(line(7, 3, ev('paused', { size: '80x24', n: 1, reason: 'start' })));
    expect(app.step.paused).toBe('start');
    expect(app.step.at).toBe(1);
    await app.stepNext();
    expect(posts.at(-1)?.body).toEqual({ command: 'next' });
    const asked = posts.length;
    await app.onMessage(line(7, 4, ev('step-before', { size: '80x24', n: 1, total: 2, action: 'keys', value: ':UI<CR>' })));
    // While the step runs a key sends nothing: the command would wait in the pipe and run the next step unasked.
    await app.stepNext();
    await app.stepContinue();
    expect(posts.length).toBe(asked);
    await app.onMessage(line(7, 5, ev('step-after', { size: '80x24', n: 1, status: 'pass', ms: 3, image: true })));
    await app.onMessage(line(7, 6, ev('paused', { size: '80x24', n: 2, reason: 'step' })));
    expect(app.step.marks['80x24']?.[1]?.image).toBe(true);
    app.stepChosen = 3;
    await app.stepUntil();
    await app.stepToggleBreak(2);
    expect(app.step.breaks).toEqual([2]);
    await app.stepToggleBreak(2);
    expect(app.step.breaks).toEqual([]);
    await app.stepContinue();
    await app.stepAbort();
    expect(posts.map((p) => p.body)).toEqual([
      { command: 'next' },
      { command: 'until', n: 3 },
      { command: 'break', n: 2 },
      { command: 'clear', n: 2 },
      { command: 'continue' },
      { command: 'abort' },
    ]);
    // The answer of the hub to a command is an older state than the stream brought: it is not taken.
    expect(app.step.seq).toBe(6);
    expect(app.step.marks['80x24']?.[1]).toBeDefined();
    app.stop();
  });

  it('answers a question with yes or no and a note; next is a no', async () => {
    const { app, posts } = await started();
    await app.open(app.fleet!.repos[0]!.key);
    await app.stepSheet('float');
    posts.length = 0;
    await app.onMessage(line(7, 3, ev('ask', { size: '80x24', n: 1, text: 'Does it sound right?' })));
    expect(app.step.question?.text).toBe('Does it sound right?');
    await app.stepYes();
    await app.stepAnswer(false, '  the sound stutters ');
    await app.stepNext();
    expect(posts.map((p) => p.body)).toEqual([
      { command: 'answer', yes: true },
      { command: 'answer', yes: false, text: 'the sound stutters' },
      { command: 'answer', yes: false },
    ]);
    // Without a question, y does nothing.
    await app.onMessage(line(7, 4, ev('step-after', { size: '80x24', n: 1, status: 'pass', ms: 1 })));
    await app.stepYes();
    expect(posts.length).toBe(3);
    app.stop();
  });

  it('reads the state whole when a line of another run comes, or the stream says resync', async () => {
    const { app, stateReads, setState } = await started();
    await app.open(app.fleet!.repos[0]!.key);
    await app.stepSheet('float');
    const before = stateReads();
    setState({ schema: 1, kind: 'hub.sheet_step', available: true, session: 8, running: true, seq: 1, key: 'k-0123456789ab', sheet: 'smoke', events: [START] });
    await app.onMessage(line(8, 1, START));
    expect(stateReads()).toBe(before + 1);
    expect(app.step.session).toBe(8);
    expect(app.step.sheet).toBe('smoke');
    await app.onMessage({ event: 'resync', data: '{}' });
    expect(stateReads()).toBe(before + 2);
    // A hello of a stream that came back reads it too.
    await app.onMessage({ event: 'hello', data: JSON.stringify({ rev: app.fleet?.rev }) });
    expect(stateReads()).toBe(before + 3);
    app.stop();
  });

  it('shows the run on request and reads its state when it holds none', async () => {
    const { app, stateReads } = await started();
    expect(app.stepView).toBe(false);
    app.showStep();
    await Promise.resolve();
    expect(app.stepView).toBe(true);
    expect(stateReads()).toBe(1);
    app.hideStep();
    expect(app.stepView).toBe(false);
    // Leaving for the overview leaves the run too.
    app.showStep();
    app.close();
    expect(app.stepView).toBe(false);
    app.stop();
  });
});
