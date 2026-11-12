// `test` and `expect` of the e2e tests: Playwright's, with a browser that is started once more when it closed while it
// started (./launch.ts). The options of the config (`channel`, `headless`, `launchOptions`) are used as Playwright
// itself uses them.

import { test as base } from '@playwright/test';
import { launchWithRetry } from './launch';

export { expect, type Page } from '@playwright/test';

export const test = base.extend({
  browser: [
    async ({ playwright, browserName, headless, channel, launchOptions }, use) => {
      const browser = await launchWithRetry(
        () => playwright[browserName].launch({ ...launchOptions, headless, channel }),
        (line) => console.warn(line),
      );
      await use(browser);
      await browser.close();
    },
    { scope: 'worker' },
  ],
});
