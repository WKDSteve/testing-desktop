// The port the hub of the browser tests listens on. The global setup takes a free one for each run (`ensureE2ePort`) and
// hands it to the workers in `TESTING_E2E_PORT`, so that runs on one machine (the self-hosted runners of the sibling
// apps, a terminal, another session) never take the same port; a given `TESTING_E2E_PORT` is kept. 17698 is only what a
// process reads that was not started by the setup.

import { freePort } from 'appkit-web/e2e-node';

export const DEFAULT_E2E_PORT = 17698;

/** The port from the environment: a whole number from 1024 to 65535, else the default; a wrong value is an error. */
export function e2ePort(env: Record<string, string | undefined>): number {
  const raw = env['TESTING_E2E_PORT'];
  if (raw === undefined || raw === '') return DEFAULT_E2E_PORT;
  const port = Number(raw);
  if (!/^\d+$/.test(raw) || port < 1024 || port > 65535) {
    throw new Error(`TESTING_E2E_PORT must be a port from 1024 to 65535, got ${JSON.stringify(raw)}`);
  }
  return port;
}

/**
 * The port of this run: `TESTING_E2E_PORT` when it is given (checked as `e2ePort` does), else a free one, written to
 * `env` so that the processes started after it (the workers) read the same.
 */
export async function ensureE2ePort(env: Record<string, string | undefined>): Promise<number> {
  const given = env['TESTING_E2E_PORT'];
  if (given !== undefined && given !== '') return e2ePort(env);
  const port = await freePort();
  env['TESTING_E2E_PORT'] = String(port);
  return port;
}
