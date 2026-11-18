// The hub binary the browser tests start: the debug build of this repository, where cargo put it.

import { isAbsolute, join, resolve } from 'node:path';

/**
 * `<target>/debug/testing-hub(.exe)`. The target folder is `CARGO_TARGET_DIR` when it is set (relative to the
 * repository, the folder `cargo build` runs in), else `<repo>/target`: a worktree builds into a short folder of its own
 * (the rule for parallel sessions), and without this the tests found no binary there, or an old one under `target`.
 */
export function hubExe(repo: string, env: NodeJS.ProcessEnv = process.env, platform: NodeJS.Platform = process.platform): string {
  const given = env['CARGO_TARGET_DIR'];
  const target = given ? (isAbsolute(given) ? given : resolve(repo, given)) : join(repo, 'target');
  return join(target, 'debug', platform === 'win32' ? 'testing-hub.exe' : 'testing-hub');
}
