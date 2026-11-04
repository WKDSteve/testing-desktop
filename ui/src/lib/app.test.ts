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
function fakeHub(answer: (path: string, seen: Seen[]) => Response | Promise<Response>, stream?: () => Response) {
  const seen: Seen[] = [];
  const streamAuth: (string | null)[] = [];
  const fetchFn = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const headers = new Headers(init?.headers);
    const path = String(input);
    // The event stream stays open and says nothing, until a test gives it something to say.
    if (path === '/api/testing/events') {
      streamAuth.push(headers.get('Authorization'));
      return new Promise<Response>((resolve, reject) => {
        if (stream) resolve(stream());
        init?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
      });
    }
    const s = { path, auth: headers.get('Authorization'), ifNoneMatch: headers.get('If-None-Match') };
    seen.push(s);
    return answer(path, seen);
  }) as typeof fetch;
  return { hub: new HubSource(fetchFn), seen, streamAuth };
}

/** A stream that says these messages and then stays open. */
function saying(text: string): () => Response {
  return () =>
    new Response(
      new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(text));
        },
      }),
      { status: 200, headers: { 'Content-Type': 'text/event-stream; charset=utf-8' } },
    );
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

describe('the stream', () => {
  const started = async (answer: (path: string, seen: Seen[]) => Response = standard, stream?: () => Response) => {
    const fake = fakeHub(answer, stream);
    const session = new Memory();
    session.setItem(TOKEN_KEY, 't');
    const app = new App({ hub: fake.hub, session, sleep: now });
    await app.start(null);
    return { ...fake, app };
  };
  const fleetReads = (seen: Seen[]) => seen.filter((s) => s.path === '/api/testing/fleet');
  const repoReads = (seen: Seen[]) => seen.filter((s) => s.path.startsWith('/api/testing/repo?key='));
  const event = (name: string, data: unknown = {}) => ({ event: name, data: JSON.stringify(data) });
  const REV = (fleetJson as unknown as Fleet).rev;

  it('is read with the token of the session, after the fleet, and asks nothing by itself', async () => {
    const { app, seen, streamAuth } = await started();
    await Promise.resolve();
    expect(streamAuth).toEqual(['Bearer t']);
    expect(fleetReads(seen).length).toBe(1);
    app.stop();
  });

  it('an event of the fleet reads it again, with the tag of the one held', async () => {
    const { app, seen } = await started();
    await app.onMessage(event('fleet', { seq: 1 }));
    const reads = fleetReads(seen);
    expect(reads.length).toBe(2);
    expect(reads[1]?.ifNoneMatch).toBe('"sha256:e4259770b57246bc"');
    expect(repoReads(seen).length).toBe(0);
    app.stop();
  });

  it('the repository that is open is read again and stays on the page meanwhile', async () => {
    let release: (r: Response) => void = () => {};
    let held = false;
    const { app, seen } = await started((p) => {
      if (held && p.startsWith('/api/testing/repo?key=')) {
        return new Promise<Response>((resolve) => (release = resolve)) as unknown as Response;
      }
      return standard(p);
    });
    await app.open('beta.nvim-ac6d3a4ece7f');
    const before = app.repo;
    expect(before).not.toBeNull();
    held = true;
    const reading = app.onMessage(event('fleet'));
    await new Promise((resolve) => setTimeout(resolve));
    expect(app.repo).toBe(before);
    release(json(JSON.stringify({ ...(repoJson as object), cases_red: 7 })));
    await reading;
    expect((app.repo as RepoDoc).cases_red).toBe(7);
    expect(repoReads(seen).length).toBe(2);
    app.stop();
  });

  it('a read of an open repository that fails keeps what the page shows', async () => {
    let fail = false;
    const { app } = await started((p) => (fail && p.startsWith('/api/testing/repo?key=') ? json('{"error":{"code":"x","message":"x"}}', 500) : standard(p)));
    await app.open('beta.nvim-ac6d3a4ece7f');
    fail = true;
    await app.onMessage(event('fleet'));
    expect(app.repo).not.toBeNull();
    expect(app.repoFailure).toBeNull();
    app.stop();
  });

  it('a repository that was closed meanwhile is not opened again by an event', async () => {
    const { app, seen } = await started();
    await app.open('beta.nvim-ac6d3a4ece7f');
    app.close();
    await app.onMessage(event('fleet'));
    expect(app.repo).toBeNull();
    expect(repoReads(seen).length).toBe(1);
    app.stop();
  });

  it('resync reads everything again', async () => {
    const { app, seen } = await started();
    await app.onMessage(event('resync'));
    expect(fleetReads(seen).length).toBe(2);
    app.stop();
  });

  it('hello reads again only when the stream stands at another revision', async () => {
    const { app, seen } = await started();
    await app.onMessage(event('hello', { rev: REV, seq: 0 }));
    expect(fleetReads(seen).length).toBe(1);
    await app.onMessage(event('hello', { seq: 0 }));
    expect(fleetReads(seen).length).toBe(1);
    await app.onMessage(event('hello', { rev: 'rev:other', seq: 0 }));
    expect(fleetReads(seen).length).toBe(2);
    await app.onMessage({ event: 'hello', data: 'not json' });
    expect(fleetReads(seen).length).toBe(3);
    app.stop();
  });

  it('what the hub says over the wire arrives', async () => {
    const { app, seen } = await started(standard, saying('event: fleet\ndata: {"seq":1}\n\n'));
    await expect.poll(() => fleetReads(seen).length).toBe(2);
    app.stop();
  });

  it('a stream the hub refuses with 401 takes the session', async () => {
    const refused = () => json('{"error":{"code":"unauthorized","message":"x"}}', 401);
    const { app } = await started(standard, refused);
    await expect.poll(() => app.phase).toBe('needs_pairing');
  });

  it('stops when the page stops', async () => {
    const { app, streamAuth } = await started();
    await Promise.resolve();
    app.stop();
    expect(app.stream.stopped).toBe(true);
    await new Promise((resolve) => setTimeout(resolve));
    expect(streamAuth.length).toBe(1);
  });
});

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
