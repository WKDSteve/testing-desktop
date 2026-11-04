import { writeFileSync } from 'node:fs';
import { failAfterCleanup, removeTree, tempFile } from './cleanup';
import { sessionToken, startHub, type Running } from './hub';
import { ensureE2ePort } from './port';

// One file for one run: the workers inherit the variable from here.
process.env['TESTING_DESKTOP_E2E_STATE'] ??= tempFile(`e2e-state-${process.pid}.json`);
export const STATE = process.env['TESTING_DESKTOP_E2E_STATE'];

let running: (Running & { root: string }) | null = null;

/** The end of a run: the hub and its folder, then the state file. Both are tried; the first error is thrown. */
export async function teardown(hub: Running | null, state: string): Promise<void> {
  let first: unknown;
  try {
    await hub?.stop();
  } catch (error) {
    first = error;
  }
  try {
    await removeTree(state);
  } catch (error) {
    first ??= error;
  }
  if (first !== undefined) throw first;
}

export default async function setup(): Promise<() => Promise<void>> {
  try {
    // A free port for this run, before the hub starts: the workers are started after the setup and inherit it.
    await ensureE2ePort(process.env);
    running = await startHub();
    const token = await sessionToken(running.config);
    writeFileSync(STATE, JSON.stringify({ config: running.config, token, root: running.root }));
  } catch (error) {
    // Playwright runs no teardown when the setup fails: what was made so far goes here (or the error says it stayed).
    const made = running;
    running = null;
    return failAfterCleanup(error, () => teardown(made, STATE));
  }
  return () => teardown(running, STATE);
}
