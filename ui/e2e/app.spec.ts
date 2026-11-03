import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { expectAccessible, violations, watchViolations } from './a11y';
import { STATE } from './global';
import { origin, pairingLink } from './hub';

// The page against the real hub and the real engine (e2e/hub.ts): the fleet of the tests' repositories, red ones first,
// a repository with its red cases, hostile output as text, the pairing, and nothing the page must not do (a request
// to anywhere but the hub, a broken policy, an error in the console, a read that repeats itself).

test.use({ locale: 'en-US' });

const state = (): { config: string; token: string } => JSON.parse(readFileSync(STATE, 'utf8')) as { config: string; token: string };

/** Everything the page writes to the console as an error, and every request that leaves for elsewhere. */
async function watch(page: Page): Promise<{ errors: string[]; foreign: string[]; fleetReads: () => number }> {
  const errors: string[] = [];
  const foreign: string[] = [];
  let reads = 0;
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('request', (r) => {
    const url = new URL(r.url());
    if (url.origin !== origin()) foreign.push(r.url());
    if (url.pathname === '/api/testing/fleet') reads += 1;
  });
  await watchViolations(page);
  return { errors, foreign, fleetReads: () => reads };
}

async function openPaired(page: Page): Promise<void> {
  await page.addInitScript((t) => sessionStorage.setItem('testing-desktop.session', t), state().token);
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
}

test('the overview shows the released repositories, red first, and leaves the private one out', async ({ page }) => {
  const seen = await watch(page);
  await openPaired(page);
  const names = await page.getByRole('rowheader').allInnerTexts();
  expect(names).toEqual(['alpha.nvim', 'hostile.nvim', 'beta.nvim']);
  // The icon in the bar is really drawn: the policy of the page lets it through.
  await expect.poll(() => page.locator('.brand img').evaluate((i: HTMLImageElement) => i.naturalWidth)).toBeGreaterThan(0);
  await expect(page.getByText('secret.nvim')).toHaveCount(0);
  await expect(page.getByText(/1 repositories are not released/)).toBeVisible();
  await expect(page.getByRole('row', { name: /alpha\.nvim/ })).toContainText('1 failed');
  await expect(page.getByRole('row', { name: /beta\.nvim/ })).toContainText('green');
  // The CI was not asked by this hub: it says so instead of showing a state.
  await expect(page.getByRole('row', { name: /beta\.nvim/ })).toContainText('not asked');

  // A read of the fleet is one read: nothing asks again by itself.
  const before = seen.fleetReads();
  await page.waitForTimeout(1000);
  expect(seen.fleetReads()).toBe(before);
  expect(before).toBe(1);

  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

test('the filter shows the red ones, the search a part of the name', async ({ page }) => {
  await openPaired(page);
  await page.getByLabel('Only red').check();
  expect(await page.getByRole('rowheader').allInnerTexts()).toEqual(['alpha.nvim', 'hostile.nvim']);
  await page.getByLabel('Only red').uncheck();
  await page.getByRole('searchbox', { name: 'Find a repository' }).fill('BET');
  expect(await page.getByRole('rowheader').allInnerTexts()).toEqual(['beta.nvim']);
});

test('a repository shows its red cases with what was expected and what came', async ({ page }) => {
  const seen = await watch(page);
  await openPaired(page);
  await page.getByRole('button', { name: 'alpha.nvim' }).click();
  await expect(page.getByRole('heading', { name: 'alpha.nvim' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Red cases' })).toBeVisible();
  const item = page.getByRole('listitem').filter({ hasText: 'alpha › adds' });
  await expect(item).toBeVisible();
  await expect(item).toContainText('Expected');
  await expect(item).toContainText('3');
  await expect(item).toContainText('Actual');
  await expect(item).toContainText('2');
  await page.getByRole('button', { name: /All repositories/ }).click();
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
  expect(seen.errors).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

test('hostile output of a test is shown as text: no markup runs, reordering characters are made visible', async ({ page }) => {
  const seen = await watch(page);
  await openPaired(page);
  await page.getByRole('button', { name: 'hostile.nvim' }).click();
  await expect(page.getByRole('heading', { name: 'Red cases' })).toBeVisible();
  const detail = page.locator('main');
  await expect(detail).toContainText('<img src=x onerror=window.__xss=1>');
  await expect(detail).toContainText('<svg onload=window.__xss=3>');
  await expect(detail).toContainText('⟦U+202E⟧');
  expect(await page.evaluate(() => (window as unknown as { __xss?: number }).__xss)).toBeUndefined();
  // No link is made of a text that is not a CI run.
  await expect(page.locator('a[href^="javascript:"]')).toHaveCount(0);
  await expect(page.locator('main img, main svg:not([aria-hidden])')).toHaveCount(0);
  // Two cases of one name (one id twice) are both there, and the page did not stop over them.
  await expect(page.locator('ol.cases > li').filter({ hasText: '› twice' })).toHaveCount(2);
  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

for (const scheme of ['light', 'dark'] as const) {
  test(`the overview and a repository are accessible in ${scheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await openPaired(page);
    await expectAccessible(page, `overview (${scheme})`);
    await page.getByRole('button', { name: 'alpha.nvim' }).click();
    await expect(page.getByRole('heading', { name: 'Red cases' })).toBeVisible();
    await expectAccessible(page, `repository (${scheme})`);
  });
}

test('a tab without a session asks for the pairing, and a pairing link brings the fleet', async ({ page }) => {
  const seen = await watch(page);
  await page.goto('/');
  await expect(page.getByRole('status')).toContainText('not paired');
  const link = pairingLink(state().config);
  await page.goto(link);
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
  // The code left the address at once.
  expect(new URL(page.url()).hash).toBe('');
  expect(await page.evaluate(() => sessionStorage.getItem('testing-desktop.session'))).toMatch(/^[0-9a-f]+$/);
  expect(await page.evaluate(() => localStorage.length)).toBe(0);
  expect(seen.errors).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

test('a used pairing link is refused and says so', async ({ page }) => {
  const link = pairingLink(state().config);
  const code = link.split('#pair=')[1] ?? '';
  // Spent before the page sees it.
  await fetch(`${origin()}/auth/exchange`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Origin: origin() },
    body: JSON.stringify({ code }),
  });
  await page.goto(link);
  await expect(page.getByRole('status')).toContainText('used already');
});
