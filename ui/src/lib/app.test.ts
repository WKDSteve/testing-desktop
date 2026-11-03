import { describe, expect, it } from 'vitest';
import fleetJson from '../../../crates/testing-hub/tests/fixtures/golden/fleet.json';
import repoJson from '../../../crates/testing-hub/tests/fixtures/golden/repo.json';
import { App, TOKEN_KEY, type StorageLike } from './app.svelte';
import type { Fleet, RepoDoc } from './contract';
import { ApiError, HubSource } from './source';

const FLEET = JSON.stringify(fleetJson);
const REPO = JSON.stringify(repoJson);

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

interface Seen {
  path: string;
  auth: string | null;
  ifNoneMatch: string | null;
}

/** A hub in the test: answers by path, keeps what it was asked. */
function fakeHub(answer: (path: string, seen: Seen[]) => Response | Promise<Response>) {
  const seen: Seen[] = [];
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const headers = new Headers(init?.headers);
    const path = String(input);
    const s = { path, auth: headers.get('Authorization'), ifNoneMatch: headers.get('If-None-Match') };
    seen.push(s);
    return answer(path, seen);
  }) as typeof fetch;
  return { hub: new HubSource(fetchFn), seen };
}

const json = (body: string, status = 200, headers: Record<string, string> = {}) =>
  new Response(body, { status, headers: { 'Content-Type': 'application/json', ...headers } });
const hello = (ready = true) =>
  json(JSON.stringify({ instance: 'i', version: '0', engine: { ready, problem: ready ? null : 'no_engine' } }));

function standard(path: string): Response {
  if (path === '/auth/exchange') return json(JSON.stringify({ token: 't0ken' }));
  if (path === '/api/testing/hello') return hello();
  if (path === '/api/testing/fleet') return json(FLEET, 200, { ETag: '"sha256:e4259770b57246bc"' });
  if (path.startsWith('/api/testing/repo?key=')) return json(REPO);
  return json('{"error":{"code":"nope","message":"nope"}}', 404);
}

const now = () => Promise.resolve();

describe('the session', () => {
  it('trades the code for a token kept in the tab, then reads the fleet with it', async () => {
    const { hub, seen } = fakeHub(standard);
    const session = new Memory();
    const app = new App({ hub, session, sleep: now });
    await app.start('ab'.repeat(16));
    expect(app.phase).toBe('ready');
    expect(session.getItem(TOKEN_KEY)).toBe('t0ken');
    expect((app.fleet as Fleet).repos.length).toBe((fleetJson as unknown as Fleet).repos.length);
    const fleetRead = seen.find((s) => s.path === '/api/testing/fleet');
    expect(fleetRead?.auth).toBe('Bearer t0ken');
  });

  it('asks for a pairing without a code and without a token, and tells a refused code apart', async () => {
    const { hub } = fakeHub(standard);
    const app = new App({ hub, session: new Memory(), sleep: now });
    await app.start(null);
    expect(app.phase).toBe('needs_pairing');
    const refused = fakeHub((p) => (p === '/auth/exchange' ? json('{"error":{"code":"x","message":"x"}}', 401) : standard(p)));
    const again = new App({ hub: refused.hub, session: new Memory(), sleep: now });
    await again.start('ab'.repeat(16));
    expect(again.phase).toBe('needs_pairing');
    expect(again.pairRefused).toBe(true);
  });

  it('waits for an engine that is not ready, and stops when told', async () => {
    let asked = 0;
    const { hub } = fakeHub((p) => {
      if (p === '/api/testing/hello') {
        asked += 1;
        return hello(asked > 2);
      }
      return standard(p);
    });
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub, session, sleep: now });
    await app.start(null);
    expect(asked).toBe(3);
    expect(app.phase).toBe('ready');
  });

  it('goes back to the pairing when the hub says the session is gone', async () => {
    let gone = false;
    const { hub } = fakeHub((p) => (gone && p === '/api/testing/fleet' ? json('{"error":{"code":"unauthorized","message":"x"}}', 401) : standard(p)));
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub, session, sleep: now });
    await app.start(null);
    gone = true;
    await app.refresh();
    expect(app.phase).toBe('needs_pairing');
    expect(session.getItem(TOKEN_KEY)).toBeNull();
    expect(app.fleet).toBeNull();
  });
});

describe('the reads', () => {
  it('sends the tag it holds and keeps the fleet on a 304', async () => {
    const { hub, seen } = fakeHub((p, s) =>
      p === '/api/testing/fleet' && s.filter((x) => x.path === p).length > 1 ? new Response(null, { status: 304 }) : standard(p),
    );
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub, session, sleep: now });
    await app.start(null);
    const held = app.fleet;
    await app.refresh();
    expect(app.fleet).toBe(held);
    expect(seen.filter((s) => s.path === '/api/testing/fleet').at(-1)?.ifNoneMatch).toBe('"sha256:e4259770b57246bc"');
  });

  it('joins a read of the fleet that is under way instead of asking twice', async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    const { hub, seen } = fakeHub(async (p) => {
      if (p === '/api/testing/fleet') await gate;
      return standard(p);
    });
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub, session, sleep: now });
    const started = app.start(null);
    await new Promise((r) => setTimeout(r, 0));
    const a = app.refresh();
    const b = app.refresh();
    release();
    await Promise.all([started, a, b]);
    expect(seen.filter((s) => s.path === '/api/testing/fleet').length).toBe(1);
  });

  it('opens a repository by its key, escaped in the query', async () => {
    const { hub, seen } = fakeHub(standard);
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub, session, sleep: now });
    await app.start(null);
    const key = (repoJson as unknown as RepoDoc).repo.key;
    await app.open(key);
    expect(app.repo?.repo.key).toBe(key);
    expect(seen.at(-1)?.path).toBe(`/api/testing/repo?key=${encodeURIComponent(key)}`);
  });

  it('keeps the answer of the repository opened last, not of one opened before', async () => {
    let releaseFirst: () => void = () => {};
    const first = new Promise<void>((r) => (releaseFirst = r));
    const { hub } = fakeHub(async (p) => {
      if (p.includes('first.nvim')) await first;
      return standard(p);
    });
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub, session, sleep: now });
    await app.start(null);
    const slow = app.open('first.nvim-000000000000');
    await app.open('beta.nvim-ac6d3a4ece7f');
    releaseFirst();
    await slow;
    expect(app.openKey).toBe('beta.nvim-ac6d3a4ece7f');
    expect(app.repo).not.toBeNull();
  });

  it('says why the fleet could not be read when there is none to show', async () => {
    const { hub } = fakeHub((p) =>
      p === '/api/testing/fleet' ? json('{"error":{"code":"engine_failed","message":"the engine could not answer"}}', 502) : standard(p),
    );
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub, session, sleep: now });
    await app.start(null);
    expect(app.phase).toBe('failed');
    expect(app.failure).toContain('engine_failed');
    expect(new ApiError(1, 'a', 'b')).toBeInstanceOf(Error);
  });
});
