import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { changed, exportFiles, fakePicker, FILE, openExport, pageUrl, violations, watchPage, type Watch } from './page';

// The single file, opened from disk, on an export folder: what it shows, and that it never does what it must not: a
// request, a violation of its policy, an error, a touch at another page's database.

test.use({ locale: 'en-US' });

const ALPHA = 'repos/alpha.nvim-356bf237c7fc.json';
const BETA = 'repos/beta.nvim-ac6d3a4ece7f.json';

async function clean(page: import('@playwright/test').Page, seen: Watch): Promise<void> {
  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
  expect(await violations(page)).toEqual([]);
}

test('the file holds its own policy and the way in asks for a folder to read', async ({ page }) => {
  const seen = await watchPage(page);
  await fakePicker(page, exportFiles());
  await page.goto(pageUrl());
  const policy = await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute('content');
  expect(policy).toContain("connect-src 'none'");
  expect(policy).not.toContain('wasm-unsafe-eval');
  expect(policy).not.toContain('worker-src');
  await expect(page.getByRole('heading', { name: 'Open an export folder' })).toBeVisible();
  await page.getByRole('button', { name: 'Choose folder' }).click();
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __pickerOptions: unknown[] }).__pickerOptions)).toEqual([
    { id: 'testing-web-export', mode: 'read' },
  ]);
  await clean(page, seen);
});

test('the fleet and a repository are those of the export, with how old it is and without the hub', async ({ page }) => {
  const seen = await watchPage(page);
  await fakePicker(page, exportFiles());
  await openExport(page);
  // Red first, then by name.
  await expect(page.getByRole('rowheader')).toHaveText(['beta.nvim', 'alpha.nvim', 'gamma.nvim']);
  await expect(page.getByText(/^Export of .*ago\)$/)).toBeVisible();
  await page.getByRole('button', { name: 'alpha.nvim' }).click();
  await expect(page.getByRole('heading', { name: 'alpha.nvim' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Red cases' })).toBeVisible();
  // The log of the runs comes with the document of the repository, also from a folder.
  await expect(page.getByRole('heading', { name: 'History' })).toBeVisible();
  // An export holds no findings: the section is not there, not empty.
  await expect(page.getByRole('heading', { name: 'Findings' })).toHaveCount(0);
  await expect(page.getByText(/pair|testing-hub/i)).toHaveCount(0);
  await clean(page, seen);
});

test('the palette and the menus are those of the hub, and Help names the folder', async ({ page }) => {
  const seen = await watchPage(page);
  await fakePicker(page, exportFiles());
  await openExport(page);
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox').fill('beta');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'beta.nvim' })).toBeVisible();
  await page.getByRole('menubar').getByRole('menuitem', { name: 'Help' }).click();
  await expect(page.getByRole('menu')).toContainText('Folder: testing-results');
  await page.keyboard.press('Escape');
  await clean(page, seen);
});

test('a new export in the folder shows when the window gets the focus back, and half of one does not', async ({ page }) => {
  const seen = await watchPage(page);
  const files = exportFiles();
  await fakePicker(page, files);
  await openExport(page);
  const count = await page.getByRole('rowheader').count();
  expect(count).toBeGreaterThan(0);

  // A file of a new export is there, its manifest not yet: the sum does not match, the page stays on what it read.
  const next = changed(files, 'fleet.json', (doc) => {
    (doc['repos'] as unknown[]).splice(0, 1);
    doc['digest'] = 'sha256:0123456789abcdef';
  });
  await page.evaluate((text) => (window as unknown as { __disk: { set(p: string, t: string): void } }).__disk.set('fleet.json', text), next['fleet.json'] as string);
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await expect(page.getByRole('alert')).toContainText('does not match its sum');
  await expect(page.getByRole('rowheader')).toHaveCount(count);

  // The manifest comes last: now the export is whole.
  await page.evaluate((text) => (window as unknown as { __disk: { set(p: string, t: string): void } }).__disk.set('testing-export.json', text), next['testing-export.json'] as string);
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await expect(page.getByRole('rowheader')).toHaveCount(count - 1);
  await expect(page.getByRole('alert')).toHaveCount(0);
  await clean(page, seen);
});

test('a folder that is no good export stays on the way in and says why', async ({ page }) => {
  const seen = await watchPage(page);
  const files = exportFiles();
  const cases: [Record<string, string>, RegExp][] = [
    [{ 'notes.txt': 'x' }, /no testing-export\.json/],
    [{ ...files, 'fleet.json': `${files['fleet.json']} ` }, /does not match its sum/],
    [(() => { const { [ALPHA]: _gone, ...rest } = files; return rest; })(), /is missing/],
    [{ ...files, 'testing-export.json': (files['testing-export.json'] as string).replace('"schema":"testing-export/1"', '"schema":"testing-export/2"').replace('"schema": "testing-export/1"', '"schema": "testing-export/2"') }, /too old/],
  ];
  for (const [folder, message] of cases) {
    const context = await page.context().newPage();
    const inner = await watchPage(context);
    await fakePicker(context, folder);
    await context.goto(pageUrl());
    await context.getByRole('button', { name: 'Choose folder' }).click();
    await expect(context.getByRole('alert')).toContainText(message);
    await expect(context.getByRole('button', { name: 'Choose folder' })).toBeVisible();
    await clean(context, inner);
    await context.close();
  }
  expect(seen.requests).toEqual([]);
});

test('the history of a repository: the numbers of the engine, a filter, and a red case as text', async ({ page }) => {
  const seen = await watchPage(page);
  const hostile = '<img src=x onerror=window.__xss=2>';
  const files = changed(exportFiles(), BETA, (doc) => {
    const history = doc['history'] as { stats: { red_top: { id: string; n: number }[] } };
    (history.stats.red_top[0] as { id: string }).id = hostile;
  });
  await fakePicker(page, files);
  await openExport(page);
  await page.getByRole('button', { name: 'beta.nvim' }).click();
  const section = page.getByRole('region', { name: 'History' });
  await expect(section).toContainText('Green, last 30 days');
  await expect(section).toContainText('50 %');
  await expect(section).toContainText('Most often red');
  await expect(section).toContainText(hostile);
  expect(await page.evaluate(() => (window as unknown as { __xss?: number }).__xss)).toBeUndefined();
  const rows = section.getByRole('row');
  await expect(rows).toHaveCount(4);
  await expect(rows.nth(1)).toContainText('a part of the suite');
  await section.getByLabel('Only red').check();
  await expect(rows).toHaveCount(3);
  await clean(page, seen);
});

test('hostile output of an export is shown as text, and a link is made only of a CI run of GitHub', async ({ page }) => {
  const seen = await watchPage(page);
  const files = changed(exportFiles(), BETA, (doc) => {
    const cases = doc['cases'] as Record<string, unknown>[];
    (cases[0] as Record<string, unknown>)['message'] = `<img src=x onerror=window.__xss=1> ${String.fromCodePoint(0x202e)}reversed`;
    const repo = doc['repo'] as { ci_remote: { run: { url: string } | null } | null };
    if (repo.ci_remote?.run) repo.ci_remote.run.url = 'https://github.com/WKDSteve/testing.nvim/actions/runs/1';
  });
  await fakePicker(page, files);
  await openExport(page);
  await page.getByRole('button', { name: 'beta.nvim' }).click();
  await expect(page.locator('main')).toContainText('<img src=x onerror=window.__xss=1>');
  await expect(page.locator('main')).toContainText('⟦U+202E⟧');
  expect(await page.evaluate(() => (window as unknown as { __xss?: number }).__xss)).toBeUndefined();
  await expect(page.locator('main img')).toHaveCount(0);
  for (const href of await page.locator('main a').evaluateAll((links) => links.map((a) => a.getAttribute('href')))) {
    expect(href).toMatch(/^https:\/\/github\.com\/WKDSteve\/[^/]+\/actions\/runs\/\d+/);
  }
  await clean(page, seen);
});

test('the database of another page on this origin stays as it is, and nothing of an export is kept', async ({ page }) => {
  const seen = await watchPage(page);
  await fakePicker(page, exportFiles());
  await page.goto(pageUrl());
  // What tasks-web left in this browser: every local file shares the origin file://.
  await page.evaluate(
    () =>
      new Promise<void>((resolve, reject) => {
        const open = indexedDB.open('tasks-web', 1);
        open.onupgradeneeded = () => open.result.createObjectStore('handles');
        open.onerror = () => reject(new Error('open'));
        open.onsuccess = () => {
          const tx = open.result.transaction('handles', 'readwrite');
          tx.objectStore('handles').put('the folders of tasks-web', 'list');
          tx.oncomplete = () => {
            open.result.close();
            resolve();
          };
        };
      }),
  );
  await page.reload();
  await page.getByRole('button', { name: 'Choose folder' }).click();
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();

  const state = await page.evaluate(async () => {
    const names = (await indexedDB.databases()).map((d) => d.name).sort();
    const other = await new Promise<unknown>((resolve, reject) => {
      const open = indexedDB.open('tasks-web');
      open.onerror = () => reject(new Error('open'));
      open.onsuccess = () => {
        const get = open.result.transaction('handles').objectStore('handles').get('list');
        get.onsuccess = () => {
          open.result.close();
          resolve(get.result);
        };
      };
    });
    return { names, other, local: localStorage.length, session: sessionStorage.length };
  });
  expect(state.other).toBe('the folders of tasks-web');
  expect(state.names.every((n) => n === 'tasks-web' || n === 'testing-web')).toBe(true);
  expect(state.local).toBe(0);
  expect(state.session).toBe(0);
  await clean(page, seen);
});

test('without the picker the folder comes through an input, read once', async ({ page }) => {
  const seen = await watchPage(page);
  await page.addInitScript(() => {
    delete (window as unknown as Record<string, unknown>)['showDirectoryPicker'];
    delete (Object.getPrototypeOf(window) as Record<string, unknown>)['showDirectoryPicker'];
    delete (Window.prototype as unknown as Record<string, unknown>)['showDirectoryPicker'];
  });
  await page.goto(pageUrl());
  await expect(page.getByText('This browser has no folder picker')).toBeVisible();
  const here = dirname(fileURLToPath(import.meta.url));
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Choose folder' }).click();
  await (await chooser).setFiles(join(here, '..', '..', 'crates', 'testing-hub', 'tests', 'fixtures', 'golden-export'));
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
  await expect(page.getByRole('rowheader')).not.toHaveCount(0);
  await clean(page, seen);
});

for (const scheme of ['light', 'dark'] as const) {
  test(`the way in, the fleet and a repository are accessible in ${scheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await fakePicker(page, exportFiles());
    await page.goto(pageUrl());
    const axe = async (what: string) => {
      const { violations: found } = await new AxeBuilder({ page }).analyze();
      expect(found.map((v) => `${what}: ${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
    };
    await axe('way in');
    // Axe is content with a button that does not react, so the colours are compared here.
    const choose = page.getByRole('button', { name: 'Choose folder' });
    const fill = () => choose.evaluate((node) => getComputedStyle(node).backgroundColor);
    const atRest = await fill();
    await choose.hover();
    expect(await fill()).not.toBe(atRest);
    await axe('way in, the button under the pointer');
    await choose.focus();
    await axe('way in, the button under the pointer and with the focus ring');
    await page.getByRole('button', { name: 'Choose folder' }).click();
    await page.getByRole('heading', { name: 'Repositories' }).waitFor();
    await axe('fleet');
    await page.getByRole('button', { name: 'alpha.nvim' }).click();
    await page.getByRole('heading', { name: 'Red cases' }).waitFor();
    await axe('repository');
  });
}

test('the built file names no address in its shell', () => {
  const html = readFileSync(FILE, 'utf8');
  const shell = html.replace(/<script[\s\S]*?<\/script>/g, '').replace(/<style[\s\S]*?<\/style>/g, '');
  expect(shell).not.toMatch(/https?:\/\//);
});
