// A real hub for the browser tests: the binary of this repository and the real engine (a real Neovim and
// testing.nvim). The repositories are made here, in the temp dir, and their results come from real runs of
// testing.nvim into a state folder of the tests' own. Nothing of the machine's own repositories or state is read:
// the hub is given that root, that state folder and a data classes file of the tests.

import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { failAfterCleanup, removeTree, stopChild, tempRoot } from './cleanup';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..', '..');
export const PORT = 17698;
export const ORIGIN = `http://127.0.0.1:${PORT}`;

const exe = join(repo, 'target', 'debug', process.platform === 'win32' ? 'testing-hub.exe' : 'testing-hub');

/** The checkout of testing.nvim the tests run: `TESTING_ENGINE_DIR`, else `$REPOS_DIR/testing.nvim`. */
function engineDir(): string {
  const dir = process.env['TESTING_ENGINE_DIR'] ?? join(process.env['REPOS_DIR'] ?? '', 'testing.nvim');
  if (!existsSync(join(dir, 'scripts', 'testing.lua'))) {
    throw new Error(`no testing.nvim checkout at ${dir}: set TESTING_ENGINE_DIR`);
  }
  return dir;
}

/** What an attacker could write into the output of a test: markup, a script link, reordering characters, length. */
// Made from their code points: the characters themselves are never in a source file (tools/check-control-chars.py).
const RLO = String.fromCodePoint(0x202e);
const PDF = String.fromCodePoint(0x202c);
export const HOSTILE_MESSAGE = `<img src=x onerror=window.__xss=1> javascript:window.__xss=2 ${RLO}txt.exe${PDF}`;

/** The repositories of the root: a spec file each, and the class each has on the "device" of the tests. */
const REPOS: { name: string; class: string; spec: string }[] = [
  {
    name: 'alpha.nvim',
    class: 'public',
    spec: ['describe("alpha", function()', '  it("adds", function()', '    assert.are.equal(3, 1 + 1)', '  end)', 'end)'].join('\n'),
  },
  {
    name: 'beta.nvim',
    class: 'internal',
    spec: ['describe("beta", function()', '  it("holds", function()', '    assert.is_true(true)', '  end)', 'end)'].join('\n'),
  },
  {
    name: 'hostile.nvim',
    class: 'public',
    spec: [
      'describe("<svg onload=window.__xss=3>", function()',
      `  it("${RLO}reversed", function()`,
      `    error(${JSON.stringify(HOSTILE_MESSAGE + '\n' + 'x'.repeat(20000))})`,
      '  end)',
      'end)',
    ].join('\n'),
  },
  {
    name: 'secret.nvim',
    class: 'private',
    spec: ['describe("secret", function()', '  it("is red", function()', '    assert.is_true(false)', '  end)', 'end)'].join('\n'),
  },
];

export interface Running {
  child: ChildProcess;
  config: string;
  /** Ends the hub, waits for it, and removes its folder in the temp dir (an error if that fails). */
  stop(): Promise<void>;
}

/** The root, the state of real runs, the data classes and the settings of the hub. */
export function prepare(): { root: string; config: string } {
  const root = tempRoot('e2e');
  try {
    return fill(root);
  } catch (error) {
    rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

function fill(root: string): { root: string; config: string } {
  const repos = join(root, 'repos');
  const state = join(root, 'state');
  mkdirSync(state, { recursive: true });
  const script = join(engineDir(), 'scripts', 'testing.lua');
  for (const r of REPOS) {
    const dir = join(repos, r.name);
    mkdirSync(join(dir, 'tests'), { recursive: true });
    writeFileSync(join(dir, '.testing.lua'), 'return {}\n');
    writeFileSync(join(dir, 'tests', `${r.name.split('.')[0]}_spec.lua`), `${r.spec}\n`);
    // A real run into the tests' state; red runs end with exit 1. `CI` trusts the root of the temp dir.
    try {
      execFileSync('nvim', ['-n', '-i', 'NONE', '--headless', '-u', 'NONE', '-l', script, 'run', '.', '--state-dir', state], {
        cwd: dir,
        env: { ...process.env, CI: 'true' },
        stdio: 'ignore',
      });
    } catch (error) {
      if ((error as { status?: number }).status !== 1) throw error;
    }
  }
  const areas: Record<string, { class: string; repo: string }> = {};
  for (const r of REPOS) areas[r.name] = { class: r.class, repo: r.name };
  writeFileSync(join(root, 'areas.json'), JSON.stringify({ schema: 1, areas }));
  const config = join(root, 'config');
  mkdirSync(config);
  writeFileSync(
    join(config, 'config.json'),
    JSON.stringify({ vault: repos, engine_dir: engineDir(), state_dir: state, areas: join(root, 'areas.json') }),
  );
  return { root, config };
}

export function env(config: string): NodeJS.ProcessEnv {
  const out: NodeJS.ProcessEnv = { ...process.env, TESTING_DESKTOP_CONFIG_DIR: config, TESTING_DESKTOP_PORT: String(PORT) };
  // The places of the settings file, not those of whoever runs the tests.
  delete out['TESTING_DESKTOP_STATE_DIR'];
  delete out['TESTING_DESKTOP_AREAS'];
  delete out['TESTING_REPOS_ROOT'];
  return out;
}

export async function startHub(): Promise<Running & { root: string }> {
  // Something already answers on the port: those tests would run against another hub.
  const taken = await fetch(`${ORIGIN}/api/testing/hello`).then(
    () => true,
    () => false,
  );
  if (taken) throw new Error(`port ${PORT} is in use: stop the hub that runs there`);
  const { root, config } = prepare();
  const child = spawn(exe, ['serve'], { env: env(config), stdio: 'ignore' });
  const stop = async (): Promise<void> => {
    await stopChild(child);
    await removeTree(root);
  };
  try {
    await waitUntilReady(config);
  } catch (error) {
    return failAfterCleanup(error, stop);
  }
  return { child, config, root, stop };
}

/** Is the binary the one of the current build of the interface? */
export async function assertFreshBinary(): Promise<void> {
  const dist = join(repo, 'ui', 'dist', 'index.html');
  if (!existsSync(dist)) throw new Error('ui/dist is not built: run `pnpm build` and `cargo build`');
  const served = await (await fetch(`${ORIGIN}/`)).text();
  if (served.trim() !== readFileSync(dist, 'utf8').trim()) {
    throw new Error('the hub binary holds an older interface: run `cargo build` after `pnpm build`');
  }
}

/** Waits until the hub answers with a ready engine, has written `hub.json` and serves the current build. */
async function waitUntilReady(config: string): Promise<void> {
  const deadline = Date.now() + 30_000;
  for (;;) {
    try {
      const r = await fetch(`${ORIGIN}/api/testing/hello`);
      if (r.ok) {
        const doc = (await r.json()) as { engine: { ready: boolean; problem: string | null } };
        if (doc.engine.ready) break;
        throw new Error(`the engine is not ready (${doc.engine.problem}): set TESTING_ENGINE_DIR`);
      }
    } catch (error) {
      if (Date.now() > deadline || (error instanceof Error && error.message.includes('engine'))) throw error;
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  while (!existsSync(join(config, 'hub.json'))) {
    if (Date.now() > deadline) throw new Error('the hub answers but wrote no hub.json');
    await new Promise((r) => setTimeout(r, 100));
  }
  await assertFreshBinary();
}

/** A pairing link, as `testing-hub open` prints it. */
export function pairingLink(config: string): string {
  const out = execFileSync(exe, ['open'], { env: env(config), encoding: 'utf8' });
  const link = out.trim().split(/\r?\n/)[0] ?? '';
  if (!link.startsWith(`${ORIGIN}/#pair=`)) throw new Error(`unexpected link: ${link}`);
  return link;
}

/** A session token, got the way the page gets one. The hub allows few exchanges a minute: the tests share one. */
export async function sessionToken(config: string): Promise<string> {
  const code = pairingLink(config).split('#pair=')[1] ?? '';
  const response = await fetch(`${ORIGIN}/auth/exchange`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Origin: ORIGIN },
    body: JSON.stringify({ code }),
  });
  if (!response.ok) throw new Error(`exchange refused: ${response.status}`);
  return ((await response.json()) as { token: string }).token;
}
