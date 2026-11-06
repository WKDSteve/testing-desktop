import { describe, expect, it } from 'vitest';
import fleetJson from '../../../crates/testing-hub/tests/fixtures/golden/fleet.json';
import findingsJson from '../../../crates/testing-hub/tests/fixtures/golden/findings.json';
import repoJson from '../../../crates/testing-hub/tests/fixtures/golden/repo.json';
import { paletteItems } from 'appkit-ui/palette';
import { App, TOKEN_KEY, type StorageLike } from './app.svelte';
import { textsOf } from './i18n';
import { paletteActions, paletteEntries } from './paletteActions';
import { HubSource } from './source';

const t = textsOf('en');

class Memory implements StorageLike {
  map = new Map<string, string>([[TOKEN_KEY, 't']]);
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

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });

/** A page that is started on a hub that answers the fleet, a repository, its findings and `about`. */
async function started(about: Response | null = null): Promise<App> {
  const fetchFn = (async (input: RequestInfo | URL) => {
    const path = String(input);
    if (path === '/api/testing/events') return new Promise<Response>(() => {});
    if (path === '/api/testing/hello') return json({ instance: 'i', version: '0', engine: { ready: true, problem: null } });
    if (path === '/api/testing/fleet') return json(fleetJson, 200);
    if (path.startsWith('/api/testing/repo?key=')) return json(repoJson);
    if (path.startsWith('/api/testing/finding?key=')) return json(findingsJson);
    if (path === '/api/testing/about' && about) return about;
    return json({ error: { code: 'nope', message: 'nope' } }, 404);
  }) as typeof fetch;
  const app = new App({ hub: new HubSource(fetchFn), session: new Memory(), sleep: () => Promise.resolve() });
  await app.start(null);
  return app;
}

describe('the palette', () => {
  it('offers the actions of the state: the overview only when a repository is open, the stepped run while it is not shown', async () => {
    const app = await started();
    expect(paletteActions(app, t).map((a) => a.key)).toEqual(['only_red', 'go.step', 'reload']);
    await app.open(app.fleet!.repos[0]!.key);
    expect(paletteActions(app, t).map((a) => a.key)).toEqual(['go.overview', 'only_red', 'go.step', 'reload']);
    app.showStep();
    expect(paletteActions(app, t).map((a) => a.key)).toEqual(['go.overview', 'only_red', 'reload']);
    app.stop();
  });

  it('names the filter by what choosing it does', async () => {
    const app = await started();
    expect(paletteActions(app, t)[0]?.label).toBe('Show only red repositories');
    app.onlyRed = true;
    expect(paletteActions(app, t)[0]?.label).toBe('Show all repositories');
    app.stop();
  });

  it('finds a repository by a part of its name and opens it; an action runs and the box closes', async () => {
    const app = await started();
    const first = app.fleet!.repos[0]!;
    const items = paletteItems(first.name.slice(0, 4), paletteActions(app, t), paletteEntries(app), {}, 0);
    const entry = items.find((i) => i.kind === 'entry' && i.id === first.key);
    expect(entry).toBeDefined();
    app.openPalette();
    await app.choose(entry!);
    expect(app.paletteOpen).toBe(false);
    expect(app.openKey).toBe(first.key);

    app.openPalette();
    const only = paletteItems('only red', paletteActions(app, t), [], {}, 0).find((i) => i.kind === 'action' && i.key.endsWith('only_red'));
    await app.choose(only!);
    expect(app.onlyRed).toBe(true);
    app.stop();
  });
});

describe('reading again', () => {
  it('reads the repository that is open and its findings, not only the fleet', async () => {
    const asked: string[] = [];
    const fetchFn = (async (input: RequestInfo | URL) => {
      const path = String(input);
      asked.push(path);
      if (path === '/api/testing/events') return new Promise<Response>(() => {});
      if (path === '/api/testing/hello') return json({ instance: 'i', version: '0', engine: { ready: true, problem: null } });
      if (path === '/api/testing/fleet') return json(fleetJson);
      if (path.startsWith('/api/testing/repo?key=')) return json(repoJson);
      if (path.startsWith('/api/testing/finding?key=')) return json(findingsJson);
      return json({ error: { code: 'nope', message: 'nope' } }, 404);
    }) as typeof fetch;
    const app = new App({ hub: new HubSource(fetchFn), session: new Memory(), sleep: () => Promise.resolve() });
    await app.start(null);
    const key = app.fleet!.repos[0]!.key;
    await app.open(key);
    asked.length = 0;
    const action = paletteActions(app, t).find((a) => a.key === 'reload');
    await action!.run();
    expect(asked).toEqual(expect.arrayContaining(['/api/testing/fleet', `/api/testing/repo?key=${key}`, `/api/testing/finding?key=${key}`]));
    app.stop();
  });

  it('a start closes a palette that was open', async () => {
    const app = await started();
    app.openPalette();
    app.stop();
    await app.start(null);
    expect(app.paletteOpen).toBe(false);
    app.stop();
  });
});

describe('about', () => {
  it('is read once the page is ready, and a hub that cannot say leaves it empty', async () => {
    const none = await started();
    await Promise.resolve();
    expect(none.about).toBeNull();
    none.stop();

    const said = await started(
      json({ app: 'testing-desktop', version: '1', engine: { version: '0.1', git: 'abc', nvim: '0.12' }, reads: { state_dir: 'default', areas: 'default' }, ci: 'no_token' }),
    );
    await expect.poll(() => said.about?.engine?.git).toBe('abc');
    expect(said.about?.reads.stateDir).toBe('default');
    expect(said.about?.ci).toBe('no_token');
    said.stop();
  });
});
