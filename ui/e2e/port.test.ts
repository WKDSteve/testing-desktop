// The port of the browser tests, without a hub: a run takes a free one and hands it on, a given one is kept and checked.

import { describe, expect, it } from 'vitest';
import { DEFAULT_E2E_PORT, e2ePort, ensureE2ePort } from './port';

describe('the port of a browser test run', () => {
  it('is the default for a process the setup did not start', () => {
    expect(e2ePort({})).toBe(DEFAULT_E2E_PORT);
    expect(e2ePort({ TESTING_E2E_PORT: '' })).toBe(DEFAULT_E2E_PORT);
  });

  it('is what the environment says', () => {
    expect(e2ePort({ TESTING_E2E_PORT: '20123' })).toBe(20123);
  });

  it.each(['80', '65536', '12a', '-20000', '1e4', '2000.5'])('refuses %s, it does not fall back', async (raw) => {
    expect(() => e2ePort({ TESTING_E2E_PORT: raw })).toThrow(/TESTING_E2E_PORT/);
    await expect(ensureE2ePort({ TESTING_E2E_PORT: raw })).rejects.toThrow(/TESTING_E2E_PORT/);
  });

  it('takes a free one and writes it down for the workers', async () => {
    const env: Record<string, string | undefined> = {};
    const port = await ensureE2ePort(env);
    expect(port).toBeGreaterThanOrEqual(1024);
    expect(env['TESTING_E2E_PORT']).toBe(String(port));
    expect(e2ePort(env)).toBe(port);
  });

  it('keeps one that is given', async () => {
    const env = { TESTING_E2E_PORT: '20123' };
    expect(await ensureE2ePort(env)).toBe(20123);
    expect(env.TESTING_E2E_PORT).toBe('20123');
  });
});
