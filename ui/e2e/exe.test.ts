// The hub binary of the browser tests, without a hub: where cargo put it.

import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { removeTree, tempRoot } from './cleanup';
import { cargoTargetDir, hubExe, targetDirOf } from './exe';

const repo = resolve('/work/testing-desktop');
/** No cargo to ask: the folder comes from the environment alone. */
const noCargo = (): null => null;

describe('the hub binary of the browser tests', () => {
  it('is the debug build under target without CARGO_TARGET_DIR', () => {
    expect(hubExe(repo, {}, 'linux', noCargo)).toBe(join(repo, 'target', 'debug', 'testing-hub'));
    expect(hubExe(repo, { CARGO_TARGET_DIR: '' }, 'win32', noCargo)).toBe(join(repo, 'target', 'debug', 'testing-hub.exe'));
  });

  it('is under CARGO_TARGET_DIR when it is set', () => {
    const own = resolve('/t/td-k26');
    expect(hubExe(repo, { CARGO_TARGET_DIR: own }, 'win32', noCargo)).toBe(join(own, 'debug', 'testing-hub.exe'));
  });

  it('takes a relative CARGO_TARGET_DIR from the repository', () => {
    expect(hubExe(repo, { CARGO_TARGET_DIR: 'target-k26' }, 'linux', noCargo)).toBe(join(repo, 'target-k26', 'debug', 'testing-hub'));
  });

  it('is where cargo says it builds, before the environment', () => {
    const said = resolve('/t/from-cargo');
    expect(hubExe(repo, { CARGO_TARGET_DIR: 'ignored' }, 'linux', () => said)).toBe(join(said, 'debug', 'testing-hub'));
  });

  it('reads target_directory of cargo metadata, and nothing else', () => {
    expect(targetDirOf('{"packages":[],"target_directory":"E:\\\\t\\\\x"}')).toBe('E:\\t\\x');
    expect(targetDirOf('{"target_directory":""}')).toBeNull();
    expect(targetDirOf('{"target_directory":3}')).toBeNull();
    expect(targetDirOf('not json')).toBeNull();
    expect(targetDirOf('null')).toBeNull();
  });

  it('follows build.target-dir of .cargo/config.toml and CARGO_BUILD_TARGET_DIR, as cargo build does', async () => {
    const dir = tempRoot('exe-target');
    try {
      writeFileSync(join(dir, 'Cargo.toml'), '[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2021"\n');
      mkdirSync(join(dir, 'src'));
      writeFileSync(join(dir, 'src', 'lib.rs'), '');
      mkdirSync(join(dir, '.cargo'));
      writeFileSync(join(dir, '.cargo', 'config.toml'), '[build]\ntarget-dir = "from-config"\n');
      const env = { ...process.env };
      delete env['CARGO_TARGET_DIR'];
      delete env['CARGO_BUILD_TARGET_DIR'];
      const asked = cargoTargetDir(dir, env);
      if (asked === null) return; // no cargo on this machine: the cases above hold the rest
      expect(hubExe(dir, env, 'linux')).toBe(join(dir, 'from-config', 'debug', 'testing-hub'));
      const own = join(dir, 'from-env');
      expect(hubExe(dir, { ...env, CARGO_BUILD_TARGET_DIR: own }, 'linux')).toBe(join(own, 'debug', 'testing-hub'));
    } finally {
      await removeTree(dir);
    }
  }, 60_000);
});
