// The app "hub" of the test sheets (.testing.lua): a testing-hub of the run on an empty machine, started the way
// ui/e2e/hub.ts starts one (the real binary, the real engine), with nothing of the person's settings, repositories or
// runs. Prints the pairing link on its stdout once the engine answers, and ends the hub when its stdin closes.
//   node tools/sheet-hub.mjs <folder of the run> <port>
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const [folder, port] = process.argv.slice(2);
if (!folder || !/^\d+$/.test(port ?? '')) {
  console.error('usage: node tools/sheet-hub.mjs <folder of the run> <port>');
  process.exit(2);
}

const exe = resolve('target', 'debug', process.platform === 'win32' ? 'testing-hub.exe' : 'testing-hub');
const engine = [process.env.TESTING_ENGINE_DIR, resolve('.deps', 'testing.nvim'), join(process.env.REPOS_DIR ?? '', 'testing.nvim')].find(
  (dir) => dir && existsSync(join(dir, 'scripts', 'testing.lua')),
);
if (!engine) {
  console.error('no testing.nvim checkout: set TESTING_ENGINE_DIR');
  process.exit(2);
}

// An empty machine: no repository, no run, no area.
const config = join(folder, 'config');
const state = join(folder, 'state');
const repos = join(folder, 'repos');
for (const dir of [config, state, repos]) mkdirSync(dir, { recursive: true });
writeFileSync(join(folder, 'areas.json'), JSON.stringify({ schema: 1, areas: {} }));
writeFileSync(
  join(config, 'config.json'),
  JSON.stringify({ vault: repos, engine_dir: engine, state_dir: state, areas: join(folder, 'areas.json'), ask_github: false }),
);

const env = { ...process.env, TESTING_DESKTOP_CONFIG_DIR: config, TESTING_DESKTOP_PORT: port, TESTING_RUNNER_ROOT: join(folder, 'runners') };
for (const name of ['TESTING_DESKTOP_STATE_DIR', 'TESTING_DESKTOP_AREAS', 'TESTING_REPOS_ROOT']) delete env[name];

const hub = spawn(exe, ['serve'], { env, stdio: 'ignore' });
const stop = () => {
  hub.kill();
  process.exit(0);
};
process.stdin.on('end', stop);
process.stdin.resume();
hub.on('exit', (code) => process.exit(code ?? 1));

const deadline = Date.now() + 30_000;
for (;;) {
  try {
    const answer = await fetch(`http://127.0.0.1:${port}/api/testing/hello`);
    if (answer.ok && (await answer.json()).engine.ready) break;
  } catch {
    // not up yet
  }
  if (Date.now() > deadline) {
    console.error('the hub did not get ready');
    hub.kill();
    process.exit(1);
  }
  await new Promise((resolveWait) => setTimeout(resolveWait, 200));
}
const link = spawnSync(exe, ['link'], { env, encoding: 'utf8' }).stdout.trim();
console.log(link);
