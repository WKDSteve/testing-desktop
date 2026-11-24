import { expect, test, type Page } from '@playwright/test';
import { exportFiles, fakePicker, openExport } from './page';

// A page in the background looks at nothing: the folder is not read every 30 seconds for a tab nobody sees, and the
// clock of the export's age stands still. When the page comes back it looks once.

test.use({ locale: 'en-US' });

const listings = (page: Page) => page.evaluate(() => (window as unknown as { __listings: { n: number } }).__listings.n);
const hide = (page: Page, hidden: boolean) =>
  page.evaluate((h) => {
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => h });
    document.dispatchEvent(new Event('visibilitychange'));
  }, hidden);

test('a tab in the background does not read the folder, and reads it once when it is back', async ({ page }) => {
  await page.clock.install();
  await fakePicker(page, exportFiles());
  await openExport(page);
  // In front: the timer looks every 30 seconds.
  const start = await listings(page);
  await page.clock.fastForward(31_000);
  await expect.poll(() => listings(page)).toBeGreaterThan(start);

  await hide(page, true);
  const quiet = await listings(page);
  await page.clock.fastForward(125_000);
  await page.waitForTimeout(200);
  expect(await listings(page)).toBe(quiet);

  await hide(page, false);
  await expect.poll(() => listings(page)).toBeGreaterThan(quiet);
});
