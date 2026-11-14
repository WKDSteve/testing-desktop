import { readFileSync } from 'node:fs';
import { expect, test, type Page } from './fixture';
import { expectAccessible, violations, watchViolations } from './a11y';
import { STATE } from './global';
import { origin } from './hub';

// The release assistant for the runner guard, on the page of the real hub. Its first step is the real engine's (an
// empty list, no GitHub: nothing to release). The other steps need GitHub, so the hub's answers for them are stood in
// for here (`page.route`): what is tested is the page: that the way to the release leads through the review aid and
// the question, what it sends, the keyboard, and that it reads well in both colour schemes. What the hub asks of the
// engine for these routes is `crates/testing-hub/tests/release.rs`.

test.use({ locale: 'en-US' });

const S1 = '1'.repeat(40);
const S2 = '2'.repeat(40);
const RED = 'fixture-owner/beta.nvim';
const API = '/api/testing/release';

const token = (): string => (JSON.parse(readFileSync(STATE, 'utf8')) as { token: string }).token;

async function watch(page: Page): Promise<{ errors: string[]; foreign: string[] }> {
  const errors: string[] = [];
  const foreign: string[] = [];
  page.on('console', (m) => {
    // The refusal of a release the test asks for is a 409 the browser reports; nothing else is expected.
    if (m.type() === 'error' && !m.text().includes('409')) errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('request', (r) => {
    if (new URL(r.url()).origin !== origin()) foreign.push(r.url());
  });
  await watchViolations(page);
  return { errors, foreign };
}

interface Stand {
  /** The bodies of the POSTs to `release`. */
  released: unknown[];
  /** What was asked of the reading steps, path and query. */
  asked: string[];
}

/** The hub's answers of the assistant, as `crates/testing-hub/src/releasing.rs` gives them. */
async function standIn(page: Page, options: { refuse?: boolean; rerunFails?: boolean } = {}): Promise<Stand> {
  const stand: Stand = { released: [], asked: [] };
  let outcomeReads = 0;
  const json = (body: unknown, status = 200) => ({ status, contentType: 'application/json', body: JSON.stringify(body) });
  await page.route(`**${API}**`, async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.headers()['authorization'] !== `Bearer ${token()}`) return route.fulfill(json({ error: { code: 'unauthorized', message: 'no session' } }, 401));
    if (request.method() === 'POST') {
      stand.released.push(request.postDataJSON());
      if (options.refuse) {
        return route.fulfill(json({ error: { code: 'not_shown', message: 'the review aid of this commit was not read just now (read release/show for it first)' } }, 409));
      }
      // The hub starts it and answers at once.
      return route.fulfill(json({ schema: 1, kind: 'hub.release.started', repo: RED, sha: S2 }, 202));
    }
    if (url.pathname === `${API}/outcome`) {
      outcomeReads += 1;
      // It runs for a moment: the first look finds it pending.
      if (outcomeReads === 1) return route.fulfill(json({ schema: 1, kind: 'hub.release.outcome', state: 'pending', repo: RED, sha: S2 }));
      const failed = options.rerunFails === true;
      return route.fulfill(
        json({
          schema: 1,
          kind: 'hub.release.outcome',
          state: 'done',
          repo: RED,
          sha: S2,
          exit: failed ? 3 : 0,
          problem: failed ? 're-run 72 failed: HTTP 403' : null,
          release: {
            already: false,
            recorded: true,
            reruns: [
              { id: 71, name: 'ci', was: 'failure', ok: true },
              ...(failed ? [{ id: 72, name: 'rolling', was: 'cancelled', ok: false, problem: 'HTTP 403' }] : []),
            ],
          },
        }),
      );
    }
    stand.asked.push(`${url.pathname.slice(API.length)}${url.search}`);
    if (url.pathname === `${API}/repos`) {
      return route.fulfill(
        json({
          schema: 1,
          kind: 'hub.release.repos',
          exit: 0,
          problem: null,
          repos: [
            { name: RED, listed: false, mode: null, reviewed: 0, ci: 'failure', red: true },
            { name: 'fixture-owner/alpha.nvim', listed: true, mode: 'auto', reviewed: 2, ci: 'success', red: false },
          ],
        }),
      );
    }
    if (url.pathname === `${API}/commits`) {
      return route.fulfill(
        json({
          schema: 1,
          kind: 'hub.release.commits',
          exit: 0,
          problem: null,
          repo: RED,
          commits: [
            { sha: S2, title: 'feat: <b>the newest</b>', author: 'Alice', date: '2026-11-07T10:00:00Z', released: false },
            { sha: S1, title: 'fix: the one before', author: 'Bob', date: '2026-11-06T10:00:00Z', released: true },
          ],
        }),
      );
    }
    if (url.pathname === `${API}/show`) {
      const sha = url.searchParams.get('sha') ?? '';
      return route.fulfill(
        json({
          schema: 1,
          kind: 'hub.release.show',
          exit: 0,
          problem: null,
          repo: RED,
          sha,
          show: {
            sha,
            already: false,
            prev: S1,
            compared: true,
            status: 'ahead',
            total_commits: 1,
            commits: [{ sha, author: 'Alice', title: 'feat: <b>the newest</b>' }],
            files: [
              { path: 'lua/beta/init.lua', status: 'modified' },
              { path: '.github/workflows/ci.yml', status: 'modified', flag: 'workflow' },
            ],
            files_total: 2,
            truncated: false,
          },
        }),
      );
    }
    return route.fulfill(json({ error: { code: 'not_found', message: 'no such route' } }, 404));
  });
  return stand;
}

async function openAssistant(page: Page): Promise<void> {
  await page.addInitScript((t) => sessionStorage.setItem('testing-desktop.session', t), token());
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
  await page.getByRole('menubar').getByRole('menuitem', { name: 'View' }).click();
  await page.getByRole('menu').getByRole('menuitemradio', { name: 'Release a commit for the runner…' }).click();
  await expect(page.getByRole('heading', { name: 'Release a commit for the runner', level: 1 })).toBeVisible();
}

const focused = (page: Page): Promise<string> => page.evaluate(() => document.activeElement?.textContent?.trim() ?? '');

test('the first step is the real engine: an empty list and no GitHub is nothing to release', async ({ page }) => {
  const seen = await watch(page);
  await openAssistant(page);
  await expect(page.getByRole('heading', { name: 'Which repository?' })).toBeVisible();
  await expect(page.getByText('No repository to release a commit of')).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(page.getByRole('list', { name: 'Steps of the release' }).getByRole('listitem')).toHaveText(['Repository', 'Commit', 'What changed', 'Release', 'Result']);
  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
  await expectAccessible(page, 'the assistant with nothing to release');
});

test('the way to a release leads through the repository, the commit, its review aid and a question', async ({ page }) => {
  // What the page hands to the clipboard is kept, and handed on to the real one (the policy of the page lets a page
  // write there and not read).
  await page.addInitScript(() => {
    const kept: string[] = [];
    (window as unknown as { __copied: string[] }).__copied = kept;
    const write = navigator.clipboard.writeText.bind(navigator.clipboard);
    navigator.clipboard.writeText = (text: string) => {
      kept.push(text);
      return write(text);
    };
  });
  const seen = await watch(page);
  const stand = await standIn(page);
  await openAssistant(page);

  // 1. The repository: the one with the red CI first.
  const repos = page.getByRole('list').filter({ has: page.getByRole('button', { name: /fixture-owner/ }) }).getByRole('button');
  await expect(repos).toHaveCount(2);
  await expect(repos.nth(0)).toContainText('fixture-owner/beta.nvim');
  await expect(repos.nth(0)).toContainText('CI red');
  await expect(repos.nth(0)).toContainText('not on the list yet');
  await expect(repos.nth(1)).toContainText('on the list (auto), 2 released');
  await expectAccessible(page, 'step 1');
  // With the keyboard: the focus on the choice, Enter takes it.
  await repos.nth(0).focus();
  await page.keyboard.press('Enter');

  // 2. The commit. The new step begins at its heading; a title is foreign text, shown as text.
  await expect(page.getByRole('heading', { name: `Which commit of ${RED}?` })).toBeVisible();
  expect(await focused(page)).toBe(`Which commit of ${RED}?`);
  const commits = page.getByRole('button', { name: /^(1{12}|2{12})/ });
  await expect(commits).toHaveCount(2);
  await expect(commits.nth(0)).toContainText('feat: <b>the newest</b>');
  await expect(commits.nth(1)).toContainText('released');
  await expect(page.locator('b')).toHaveCount(0);
  await expectAccessible(page, 'step 2');
  await commits.nth(0).click();

  // 3. The review aid: the full id, what to look at first, the file that decides what the runner executes.
  await expect(page.getByRole('heading', { name: 'What this commit changes' })).toBeVisible();
  await expect(page.getByText(S2).first()).toBeVisible();
  await expect(page.getByText('Since the last released commit (111111111111). 1 commits, 2 files.')).toBeVisible();
  await expect(page.getByRole('list', { name: 'To look at first' })).toContainText('Files that decide what the runner executes changed');
  const files = page.getByRole('heading', { name: 'Files' }).locator('+ ul').getByRole('listitem');
  await expect(files.nth(0)).toContainText('.github/workflows/ci.yml');
  await expect(files.nth(0)).toContainText('look closely: workflow');
  await expect(files.nth(1)).toContainText('lua/beta/init.lua');
  // No way on before the person says they looked, and nothing was sent.
  const next = page.getByRole('button', { name: 'On to the release' });
  await expect(next).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Release and re-run' })).toHaveCount(0);
  // The same as a command for a terminal, to copy.
  const command = `testing ci release ${RED} ${S2}`;
  await expect(page.getByLabel('The command that releases this commit')).toHaveText(command);
  await page.getByRole('button', { name: 'Copy' }).click();
  await expect(page.getByText('Copied.')).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __copied: string[] }).__copied)).toEqual([command]);
  await expectAccessible(page, 'step 3');
  await page.getByLabel('I looked at these changes.').check();
  await expect(next).toBeEnabled();
  await next.click();

  // 4. The question, with the full id; back keeps what was looked at.
  await expect(page.getByRole('heading', { name: 'Release this commit?' })).toBeVisible();
  await expect(page.getByText(`Release this commit of ${RED} for the self-hosted runner`)).toBeVisible();
  await expect(page.locator('p.sha')).toHaveText(S2);
  await expectAccessible(page, 'step 4');
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page.getByLabel('I looked at these changes.')).toBeChecked();
  await page.getByRole('button', { name: 'On to the release' }).click();
  expect(stand.released).toEqual([]);
  await page.getByRole('button', { name: 'Release and re-run' }).click();
  // While the hub releases, the page says so and takes no second click.
  await expect(page.getByText('Releasing…')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Release and re-run' })).toBeDisabled();

  // 5. The result, with the runs that started again; exactly the chosen repository and commit were sent, once.
  await expect(page.getByRole('heading', { name: 'Result' })).toBeVisible();
  await expect(page.getByText('released', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Runs started again' }).locator('+ ul')).toContainText('71 ci (was failure) started again');
  expect(stand.released).toEqual([{ repo: RED, sha: S2 }]);
  expect(stand.asked).toEqual(['/repos', `/commits?repo=${encodeURIComponent(RED)}`, `/show?repo=${encodeURIComponent(RED)}&sha=${S2}`]);
  await expectAccessible(page, 'step 5');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expectAccessible(page, 'step 5 (dark)');

  // Another one begins at the first step; leaving goes back to the fleet.
  await page.getByRole('button', { name: 'Release another commit' }).click();
  await expect(page.getByRole('heading', { name: 'Which repository?' })).toBeVisible();
  await page.getByRole('button', { name: /All repositories/ }).click();
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();

  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

test('a release the hub refuses stays at the question and says why; one whose re-run failed says both', async ({ page }) => {
  const seen = await watch(page);
  const stand = await standIn(page, { refuse: true });
  await openAssistant(page);
  const walk = async (): Promise<void> => {
    await page.getByRole('button', { name: /fixture-owner\/beta\.nvim/ }).click();
    await page.getByRole('button', { name: /^2{12}/ }).click();
    await page.getByLabel('I looked at these changes.').check();
    await page.getByRole('button', { name: 'On to the release' }).click();
    await page.getByRole('button', { name: 'Release and re-run' }).click();
  };
  await walk();
  await expect(page.getByRole('alert')).toContainText('not_shown: the review aid of this commit was not read just now');
  await expect(page.getByRole('heading', { name: 'Release this commit?' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Result' })).toHaveCount(0);
  expect(stand.released).toHaveLength(1);

  await page.unroute(`**${API}**`);
  await standIn(page, { rerunFails: true });
  await page.getByRole('button', { name: /All repositories/ }).click();
  await page.getByRole('menubar').getByRole('menuitem', { name: 'View' }).click();
  await page.getByRole('menu').getByRole('menuitemradio', { name: 'Release a commit for the runner…' }).click();
  await walk();
  await expect(page.getByRole('heading', { name: 'Result' })).toBeVisible();
  await expect(page.getByText('released', { exact: true })).toBeVisible();
  await expect(page.getByRole('alert')).toContainText('testing.nvim says: re-run 72 failed: HTTP 403');
  await expect(page.getByText('not started: HTTP 403')).toBeVisible();
  await expectAccessible(page, 'a release whose re-run failed');
  expect(seen.errors).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

for (const scheme of ['light', 'dark'] as const) {
  test(`the steps of the assistant are accessible in ${scheme}, with the focus on a choice`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await standIn(page);
    await openAssistant(page);
    const first = page.getByRole('button', { name: /fixture-owner\/beta\.nvim/ });
    await first.hover();
    await first.focus();
    await expectAccessible(page, `step 1 with the focus on a choice (${scheme})`);
    await first.click();
    await page.getByRole('button', { name: /^2{12}/ }).click();
    await page.getByRole('heading', { name: 'What this commit changes' }).waitFor();
    await expectAccessible(page, `step 3 (${scheme})`);
    await page.getByLabel('I looked at these changes.').check();
    await page.getByRole('button', { name: 'On to the release' }).click();
    const go = page.getByRole('button', { name: 'Release and re-run' });
    await go.hover();
    await go.focus();
    await expectAccessible(page, `step 4 with the focus on the release (${scheme})`);
  });
}
