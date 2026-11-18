// The hub binary the browser tests start: the debug build of this repository, where cargo put it.

import { execFileSync } from 'node:child_process';
import { isAbsolute, join, resolve } from 'node:path';

/** Where cargo builds `repo`, as it says itself, or null when it cannot be asked. */
export type TargetOf = (repo: string, env: NodeJS.ProcessEnv) => string | null;

/** `target_directory` of the output of `cargo metadata`, or null for anything else. */
export function targetDirOf(metadata: string): string | null {
  try {
    const doc: unknown = JSON.parse(metadata);
    const dir = doc !== null && typeof doc === 'object' ? (doc as Record<string, unknown>)['target_directory'] : undefined;
    return typeof dir === 'string' && dir !== '' ? dir : null;
  } catch {
    return null;
  }
}

/**
 * The target folder cargo uses for `repo` (`cargo metadata --no-deps`): it reads `CARGO_TARGET_DIR`,
 * `CARGO_BUILD_TARGET_DIR` and `build.target-dir` of every `.cargo/config.toml` that applies, as `cargo build` does.
 */
export const cargoTargetDir: TargetOf = (repo, env) => {
  try {
    const out = execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps', '--offline'], {
      cwd: repo,
      env,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
      timeout: 60_000,
      windowsHide: true,
    });
    return targetDirOf(out);
  } catch {
    return null;
  }
};

/**
 * `<target>/debug/testing-hub(.exe)`. The target folder is the one cargo names (`targetOf`); without cargo it is
 * `CARGO_TARGET_DIR` when that is set (relative to the repository, the folder `cargo build` runs in), else
 * `<repo>/target`: a worktree builds into a folder of its own (the rule for parallel sessions), and without this the
 * tests found no binary there, or an old one under `target`.
 */
export function hubExe(
  repo: string,
  env: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform,
  targetOf: TargetOf = cargoTargetDir,
): string {
  const given = env['CARGO_TARGET_DIR'];
  const target =
    targetOf(repo, env) ?? (given ? (isAbsolute(given) ? given : resolve(repo, given)) : join(repo, 'target'));
  return join(target, 'debug', platform === 'win32' ? 'testing-hub.exe' : 'testing-hub');
}
