// The hub binary of the browser tests, without a hub: where cargo put it.

import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { hubExe } from './exe';

const repo = resolve('/work/testing-desktop');

describe('the hub binary of the browser tests', () => {
  it('is the debug build under target without CARGO_TARGET_DIR', () => {
    expect(hubExe(repo, {}, 'linux')).toBe(join(repo, 'target', 'debug', 'testing-hub'));
    expect(hubExe(repo, { CARGO_TARGET_DIR: '' }, 'win32')).toBe(join(repo, 'target', 'debug', 'testing-hub.exe'));
  });

  it('is under CARGO_TARGET_DIR when it is set', () => {
    const own = resolve('/t/td-k26');
    expect(hubExe(repo, { CARGO_TARGET_DIR: own }, 'win32')).toBe(join(own, 'debug', 'testing-hub.exe'));
  });

  it('takes a relative CARGO_TARGET_DIR from the repository', () => {
    expect(hubExe(repo, { CARGO_TARGET_DIR: 'target-k26' }, 'linux')).toBe(join(repo, 'target-k26', 'debug', 'testing-hub'));
  });
});
