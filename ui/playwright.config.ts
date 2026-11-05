import { defineConfig } from '@playwright/test';
import { e2ePort } from './e2e/port';

// The system's Chrome is used (`channel`), so nothing is downloaded. One worker: the tests share one hub and
// pair it one after the other.
export default defineConfig({
  testDir: './e2e',
  testMatch: /.*\.spec\.ts/,
  globalSetup: './e2e/global.ts',
  workers: 1,
  fullyParallel: false,
  timeout: 60_000,
  // Every answer of the hub that is not the snapshot starts a Neovim (about 2 s, more on a loaded machine), and an event
  // of the stream can restart a read: the 5 s of Playwright's default fail on load, not on a fault.
  expect: { timeout: 15_000 },
  reporter: 'list',
  use: {
    channel: 'chrome',
    // The free port the global setup took for this run (e2e/port.ts), read again by each worker.
    baseURL: `http://127.0.0.1:${e2ePort(process.env)}`,
    trace: 'retain-on-failure',
    screenshot: 'off',
    video: 'off',
  },
});
