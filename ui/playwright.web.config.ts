import { defineConfig } from '@playwright/test';

// The single-file page of the web variant, opened from disk. No hub, no server: `pnpm web:build` makes the file first.
// The system's Chrome is used (`channel`), as for the hub's tests, so nothing is downloaded.
export default defineConfig({
  testDir: './e2e-web',
  testMatch: /.*\.spec\.ts/,
  workers: 1,
  fullyParallel: false,
  timeout: 30_000,
  reporter: 'list',
  outputDir: 'test-results-web',
  use: {
    channel: 'chrome',
    trace: 'retain-on-failure',
    screenshot: 'off',
    video: 'off',
  },
});
