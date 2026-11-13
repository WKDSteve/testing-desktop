import { readFileSync } from 'node:fs';
import { expect, test, type Page } from './fixture';
import { expectAccessible, violations, watchViolations } from './a11y';
import { STATE } from './global';
import { origin } from './hub';

// A run through the engine from the page, against the real hub and the real engine (e2e/hub.ts): the spec files of
// alpha.nvim run through testing.nvim's command line, the page follows the run to its verdict and shows the end of
// what the engine wrote. The page asks nothing of anywhere but the hub, breaks no policy and writes no error to the
// console.

test.use({ locale: 'en-US' });

const state = (): { token: string } => JSON.parse(readFileSync(STATE, 'utf8')) as { token: string };

async function watch(page: Page): Promise<{ errors: string[]; foreign: string[] }> {
  const errors: string[] = [];
  const foreign: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('request', (r) => {
    if (new URL(r.url()).origin !== origin()) foreign.push(r.url());
  });
  await watchViolations(page);
  return { errors, foreign };
}

test('the spec files of a repository run through the engine from the page, to their verdict', async ({ page }) => {
  const seen = await watch(page);
  await page.addInitScript((t) => sessionStorage.setItem('testing-desktop.session', t), state().token);
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
  await page.getByRole('button', { name: 'alpha.nvim' }).click();
  await expect(page.getByRole('heading', { name: 'alpha.nvim' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Run', exact: true })).toBeVisible();

  // A file text the hub would refuse is said, not sent.
  const field = page.getByLabel('Spec files whose name holds');
  await field.fill('--trust');
  await expect(page.getByText('Letters, digits and _ - . / only')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Run these files' })).toBeDisabled();
  await expect(field).toHaveAttribute('aria-invalid', 'true');

  // The spec of alpha.nvim is red: the run says so, with the case in the end of what the engine wrote.
  await field.fill('alpha');
  await page.getByRole('button', { name: 'Run these files' }).click();
  await expect(page.getByText(/Last run, the spec files with .alpha.: red\./)).toBeVisible({ timeout: 60_000 });
  await expect(page.getByText('The end of what testing.nvim wrote')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Run the suite' })).toBeEnabled();

  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
  await expectAccessible(page, 'run');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expectAccessible(page, 'run (dark)');
});
