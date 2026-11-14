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

test("a run of a task's sheets says under its verdict what became of the task's block", async ({ page }, testInfo) => {
  const seen = await watch(page);
  // The run is the phone's to start (tasks mobil), so its state is stood in here: green sheets, and a task that
  // testing.nvim could not write. The key is the real one of alpha.nvim, taken from the fleet the page reads.
  const fleet = page.waitForResponse((r) => new URL(r.url()).pathname === '/api/testing/fleet' && r.status() === 200);
  const key = fleet.then(async (r) => {
    const doc = (await r.json()) as { repos: { name: string; key: string }[] };
    return doc.repos.find((repo) => repo.name === 'alpha.nvim')?.key ?? '';
  });
  await page.route('**/api/testing/run', async (route) => {
    await route.fulfill({
      json: {
        schema: 1,
        kind: 'hub.run',
        available: true,
        run: {
          id: 7,
          key: await key,
          what: 'task',
          task: 'alpha.nvim/try-it',
          running: false,
          exit: 1,
          verdict: 'red',
          started: 1,
          ended: 2,
          output: 'sheet try-it: pass\ntesting sheets: task alpha.nvim/try-it conflict',
          block: { written: false, outcome: 'conflict' },
        },
      },
    });
  });
  await page.addInitScript((t) => sessionStorage.setItem('testing-desktop.session', t), state().token);
  await page.goto('/');
  await page.getByRole('button', { name: 'alpha.nvim' }).click();
  await expect(page.getByText('Last run, the test sheets of alpha.nvim/try-it: red.')).toBeVisible();
  await expect(page.getByText('The result is not in the task: testing.nvim could not write it (conflict).', { exact: false })).toBeVisible();
  for (const colorScheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme });
    await expectAccessible(page, `task block (${colorScheme})`);
    await page.getByText('The result is not in the task').scrollIntoViewIfNeeded();
    await testInfo.attach(`block-${colorScheme}`, { body: await page.screenshot(), contentType: 'image/png' });
  }
  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
});
