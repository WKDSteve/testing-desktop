import { readFileSync } from 'node:fs';
import { expect, test, type Page } from './fixture';
import { expectAccessible, violations, watchViolations } from './a11y';
import { STATE } from './global';
import { origin } from './hub';

// A test sheet step by step, against the real hub and the real engine (e2e/hub.ts): the sheet of alpha.nvim runs in a
// child editor nobody sees, the page shows its steps, runs them on request and shows what each left. The page asks
// nothing of anywhere but the hub, breaks no policy and writes no error to the console.

test.use({ locale: 'en-US' });

const state = (): { config: string; token: string; root: string } =>
  JSON.parse(readFileSync(STATE, 'utf8')) as { config: string; token: string; root: string };

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

async function openRepository(page: Page): Promise<void> {
  await page.addInitScript((t) => sessionStorage.setItem('testing-desktop.session', t), state().token);
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
  await page.getByRole('button', { name: 'alpha.nvim' }).click();
  await expect(page.getByRole('heading', { name: 'alpha.nvim' })).toBeVisible();
}

test('a sheet of a repository runs step by step from the page, and its steps show what they left', async ({ page }) => {
  const seen = await watch(page);
  await openRepository(page);
  // The sheets of the repository, by name; the one of the tests is there.
  await expect(page.getByRole('heading', { name: 'Test sheets' })).toBeVisible();
  await page.getByRole('button', { name: 'Step through smoke' }).click();

  // The run: the page shows the sheet and waits before its first step.
  await expect(page.getByRole('heading', { name: /alpha smoke/ })).toBeVisible();
  await expect(page.getByRole('status')).toContainText('paused before step 1', { timeout: 30_000 });
  const steps = page.getByRole('list', { name: 'Steps' }).getByRole('listitem');
  await expect(steps).toHaveCount(2);
  await expect(steps.nth(0)).toContainText('set the value');
  await expect(steps.nth(1)).toContainText('the value holds');

  // An explanation by an AI is an offer, closed until a person opens it; nothing was asked or built before that.
  const explain = page.locator('details.explain');
  await expect(explain).not.toHaveAttribute('open', '');
  await expect(page.getByText(/^Request about step/)).toHaveCount(0);
  await explain.locator('summary').focus();
  await page.keyboard.press('Enter');
  await expect(explain).toHaveAttribute('open', '');
  // The result of a step that has not run cannot be asked for.
  await expect(page.getByRole('button', { name: 'What does its result mean?' })).toBeDisabled();
  await page.getByRole('button', { name: 'What does this step do?' }).click();
  // testing.nvim built the request (the real engine): the page shows all of it and where it goes: nowhere from here.
  await expect(page.getByRole('heading', { name: 'Request about step 1: What does this step do?' })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText('Goes to: nowhere from this page.')).toBeVisible();
  const request = page.getByLabel('The whole request, instruction and question');
  await expect(request).toContainText('You explain one step of a test sheet of testing.nvim');
  await expect(request).toContainText('Sheet alpha.smoke: alpha smoke (engine rpc)');
  await expect(request).toContainText('> 1. lua vim.g.alpha_smoke = 1  -- set the value');
  await expect(request).toContainText('It has not run yet.');
  await expect(request).toContainText('Question: What does this step do?');
  // No folder of this machine is in it.
  expect(await request.textContent()).not.toContain(state().root);
  await explain.getByRole('button', { name: 'Copy' }).click();
  await expect(explain.getByText('Copied.')).toBeVisible();
  // The run did not move.
  await expect(page.getByRole('status').first()).toContainText('paused before step 1');
  await expectAccessible(page, 'the request for an AI');

  // One step: it passes, and the tile shows the screen of the child editor as text.
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.getByRole('status').first()).toContainText('paused before step 2', { timeout: 30_000 });
  await expect(steps.nth(0)).toContainText('pass');
  await expect(page.getByRole('heading', { name: 'Step 1, lua' })).toBeVisible();
  await expect(page.getByText('Screen', { exact: true })).toBeVisible();
  // Now that step 1 ran, its result can be asked about: the request says what it did.
  await expect(explain).toHaveAttribute('open', '');
  await page.getByRole('button', { name: 'What does its result mean?' }).click();
  await expect(page.getByRole('heading', { name: 'Request about step 1: What does its result mean?' })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByLabel('The whole request, instruction and question')).toContainText('It ran: pass in');

  // A note: too long in bytes is said, not sent; it belongs to the step the run stands before, and its tile shows it.
  const field = page.getByLabel('Note on the step where the run stands');
  // N goes to the field with Shift only: an N without it (Caps Lock on) does nothing
  await page.getByRole('heading', { name: /alpha smoke/ }).click();
  await page.keyboard.press('N');
  await expect(field).not.toBeFocused();
  await page.keyboard.press('Shift+N');
  await expect(field).toBeFocused();
  await expect(field).toHaveValue('');
  await field.fill('€'.repeat(334));
  await expect(page.getByText('A note is at most 1000 bytes.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add note', exact: true })).toBeDisabled();
  await expect(field).toHaveAttribute('aria-invalid', 'true');
  await field.fill('  the value looks right ');
  await page.getByRole('button', { name: 'Add note', exact: true }).click();
  await expect(page.getByText('Note: the value looks right')).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole('heading', { name: 'Step 2, expect' })).toBeVisible();
  await expect(page.getByLabel('Note on the step where the run stands')).toHaveValue('');

  // On to the end: green, and the run counts.
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('finished: pass', { timeout: 30_000 });
  await expect(steps.nth(1)).toContainText('pass');
  await expect(page.getByText('This run counts as a verdict.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Next', exact: true })).toBeDisabled();

  // Back to the repository, and the run is still there to look at through the menu.
  await page.getByRole('button', { name: /Back to the repository/ }).click();
  await expect(page.getByRole('heading', { name: 'alpha.nvim' })).toBeVisible();
  await page.getByRole('menubar').getByRole('menuitem', { name: 'View' }).click();
  await page.getByRole('menu').getByRole('menuitemradio', { name: 'Step by step' }).click();
  await expect(page.getByRole('heading', { name: /alpha smoke/ })).toBeVisible();
  await expect(page.getByRole('status')).toContainText('finished: pass');

  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
  await expectAccessible(page, 'stepped run');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expectAccessible(page, 'stepped run (dark)');
});

test('a run can be aborted, and a sheet the engine does not know is refused with its reason', async ({ page }) => {
  const seen = await watch(page);
  await openRepository(page);
  await page.getByRole('button', { name: 'Step through smoke' }).click();
  await expect(page.getByRole('status')).toContainText('paused before step 1', { timeout: 30_000 });
  await page.getByRole('button', { name: 'Abort', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('finished: aborted', { timeout: 30_000 });
  await expect(page.getByText('does not count as a verdict')).toBeVisible();
  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

for (const scheme of ['light', 'dark'] as const) {
  test(`a step whose status the page does not know reads as unknown, never as passed or skipped (${scheme})`, async ({ page }, testInfo) => {
    await page.emulateMedia({ colorScheme: scheme });
    const seen = await watch(page);
    // A status of a newer testing.nvim cannot come from the pinned engine, so the answer of the start is stood in:
    // a run that ended, whose second step carries a word this page does not know.
    const SCHEMA = 'testing-sheet-session/1';
    const ev = (event: string, fields: Record<string, unknown>) => ({ schema: SCHEMA, event, ...fields });
    await page.route('**/api/testing/sheet-step/start', async (route) => {
      const body = JSON.parse(route.request().postData() ?? '{}') as { key?: string };
      await route.fulfill({
        json: {
          schema: 1,
          kind: 'hub.sheet_step',
          available: true,
          session: 99,
          seq: 5,
          running: false,
          key: body.key ?? null,
          sheet: 'smoke',
          events: [
            ev('start', {
              id: 'alpha.smoke',
              title: 'alpha smoke',
              engine: 'rpc',
              sizes: [{ rows: 24, cols: 80 }],
              steps: [
                { n: 1, action: 'lua', value: 'vim.g.alpha_smoke = 1', note: 'set the value' },
                { n: 2, action: 'expect', value: 'vim.g.alpha_smoke == 1', note: 'the value holds' },
              ],
              headed: false,
              delay_ms: 0,
              breaks: [],
            }),
            ev('size', { size: '80x24', state: 'open' }),
            ev('step-after', { size: '80x24', n: 1, status: 'pass', ms: 3 }),
            ev('step-after', { size: '80x24', n: 2, status: 'flaky', ms: 4, reason: 'passed on the second try' }),
            ev('end', { status: 'pass', ok: true, verdict: true, failures: [], snapshots: [], ms: 9 }),
          ],
        },
      });
    });
    await openRepository(page);
    await page.getByRole('button', { name: 'Step through smoke' }).click();
    await expect(page.getByRole('heading', { name: /alpha smoke/ })).toBeVisible();
    const steps = page.getByRole('list', { name: 'Steps' }).getByRole('listitem');
    await expect(steps.nth(0)).toContainText('pass');
    await expect(steps.nth(1)).toContainText('unknown status (a newer testing.nvim)');
    await expect(steps.nth(1)).not.toContainText('pass');
    await expect(steps.nth(1)).not.toContainText('skipped');
    await steps.nth(1).getByRole('button', { name: /the value holds/ }).click();
    await expect(page.getByRole('heading', { name: 'Step 2, expect' })).toBeVisible();
    await expect(page.getByText('passed on the second try')).toBeVisible();
    await expectAccessible(page, `a step of an unknown status (${scheme})`);
    await testInfo.attach(`unknown-${scheme}`, { body: await page.screenshot(), contentType: 'image/png' });
    expect(seen.errors).toEqual([]);
    expect(seen.foreign).toEqual([]);
    expect(await violations(page)).toEqual([]);
  });
}
