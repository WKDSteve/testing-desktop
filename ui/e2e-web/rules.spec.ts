import { expect, test, type Page } from '@playwright/test';
import { changed, exportFiles, fakePicker, openExport, pageUrl, violations, watchPage } from './page';

// What the pages owe every reader whatever the content: a hit area a finger and a pointer can find, text that breaks
// instead of widening the page, numbers that keep their width, fields that are readable and not spell-checked, and
// names that a translation leaves alone. Measured in the real browser on the single file, on the golden export.

test.use({ locale: 'en-US' });

const BETA = 'repos/beta.nvim-ac6d3a4ece7f.json';
const LONG = 'long'.repeat(50);

/** What is clicked or tapped on the page: controls, and links that stand on their own (not a link inside a sentence). */
const TARGETS = "button, select, summary, a.tone, input:not([type='hidden'], [type='file']), label:has(> input[type='checkbox'])";

/** The visible targets that are smaller than `min` px on a side. */
async function tooSmall(page: Page, min: number): Promise<string[]> {
  return page.evaluate(
    ({ selector, size }) => {
      const out: string[] = [];
      for (const el of document.querySelectorAll<HTMLElement>(selector)) {
        // A checkbox itself is not the target: its label is, and a label that wraps it is one area with it.
        if (el instanceof HTMLInputElement && (el.type === 'checkbox' || el.type === 'radio')) continue;
        const r = el.getBoundingClientRect();
        if (r.width === 0 && r.height === 0) continue;
        if (r.width < size - 0.5 || r.height < size - 0.5) {
          out.push(`${el.tagName.toLowerCase()}.${el.className} "${(el.textContent ?? '').trim().slice(0, 20)}" ${Math.round(r.width)}x${Math.round(r.height)}`);
        }
      }
      return out;
    },
    { selector: TARGETS, size: min },
  );
}

async function openBeta(page: Page, files = exportFiles()): Promise<void> {
  await fakePicker(page, files);
  await openExport(page);
}

test.describe('hit areas with a pointer', () => {
  test('every control of the fleet and of a repository is at least 24 px on a side', async ({ page }) => {
    await openBeta(page);
    expect(await tooSmall(page, 24)).toEqual([]);
    await page.getByRole('button', { name: 'beta.nvim' }).click();
    await expect(page.getByRole('heading', { name: 'beta.nvim' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'History' })).toBeVisible();
    expect(await tooSmall(page, 24)).toEqual([]);
  });
});

test.describe('hit areas with a finger', () => {
  test.use({ hasTouch: true, isMobile: true });

  test('every control is at least 44 px on a side', async ({ page }) => {
    await openBeta(page);
    expect(await page.evaluate(() => matchMedia('(pointer: coarse)').matches)).toBe(true);
    expect(await tooSmall(page, 44)).toEqual([]);
    await page.getByRole('button', { name: 'beta.nvim' }).click();
    await expect(page.getByRole('region', { name: 'History' })).toBeVisible();
    expect(await tooSmall(page, 44)).toEqual([]);
  });
});

test('a long name without a space breaks and does not widen the page', async ({ page }) => {
  const files = changed(exportFiles(), BETA, (doc) => {
    (doc['repo'] as { name: string }).name = LONG;
    const c = (doc['cases'] as Record<string, unknown>[])[0] as Record<string, unknown>;
    c['name'] = LONG;
    c['file'] = `${LONG}/${LONG}.lua`;
    c['message'] = LONG;
    (c['fails'] as { msg: string }[])[0]!.msg = LONG;
  });
  const withFleet = changed(files, 'fleet.json', (doc) => {
    (doc['repos'] as { name: string }[]).find((r) => r.name === 'beta.nvim')!.name = LONG;
  });
  const seen = await watchPage(page);
  await page.setViewportSize({ width: 400, height: 800 });
  await fakePicker(page, withFleet);
  await openExport(page);
  const wide = () => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(await wide()).toBeLessThanOrEqual(0);
  await page.getByRole('button', { name: LONG }).click();
  await expect(page.getByRole('heading', { name: LONG })).toBeVisible();
  await expect(page.locator('.case-head strong').first()).toBeVisible();
  expect(await wide()).toBeLessThanOrEqual(0);
  expect(seen.errors).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

test('figures keep one width, headings do not leave one word alone, fields are 16 px', async ({ page }) => {
  await openBeta(page);
  const style = (selector: string, property: string) =>
    page.locator(selector).first().evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), property);
  // the fleet's cost tiles are definitions; a table keeps the width of its digits
  await expect(page.locator('dd').first()).toBeVisible();
  expect(await style('dd', 'font-variant-numeric')).toBe('tabular-nums');
  expect(await style('table', 'font-variant-numeric')).toBe('tabular-nums');
  expect(await style('h1', 'text-wrap-style')).toBe('balance');
  expect(await style('input[type="search"]', 'font-size')).not.toBe('');
  // (the command palette is a dialog of the kit; its box takes the size of the text around it, see the task for appkit)
  const small = await page.locator('input:not(dialog input), select, textarea').evaluateAll((els) =>
    els.map((el) => ({ type: (el as HTMLInputElement).type, size: parseFloat(getComputedStyle(el).fontSize) })).filter((f) => f.type !== 'hidden' && f.size < 16),
  );
  expect(small).toEqual([]);
  await page.getByRole('button', { name: 'beta.nvim' }).click();
  await expect(page.getByRole('region', { name: 'History' })).toBeVisible();
  expect(await style('.number', 'font-variant-numeric')).toBe('tabular-nums');
  expect(await style('h2', 'text-wrap-style')).toBe('balance');
  const smallHere = await page.locator('input:not(dialog input), select, textarea').evaluateAll((els) =>
    els.map((el) => parseFloat(getComputedStyle(el).fontSize)).filter((size) => size < 16),
  );
  expect(smallHere).toEqual([]);
  // the search fields name things, not words
  for (const field of await page.locator('input[type="search"]').all()) await expect(field).toHaveAttribute('spellcheck', 'false');
});

test('identifiers, paths, output and the name of the app are not translated by the browser', async ({ page }) => {
  await openBeta(page);
  const untranslated = (selector: string) =>
    page.locator(selector).evaluateAll((els) => els.map((el) => el.closest('[translate="no"]') !== null));
  expect(await untranslated('.brand')).toEqual([true]);
  expect(await untranslated('.link')).toEqual([true, true, true]);
  await page.getByRole('button', { name: 'beta.nvim' }).click();
  await expect(page.getByRole('region', { name: 'History' })).toBeVisible();
  expect(await untranslated('h1')).toEqual([true]);
  for (const selector of ['.mono', 'pre', '.case-head strong']) {
    const flags = await untranslated(selector);
    expect(flags.length, selector).toBeGreaterThan(0);
    expect(flags, selector).not.toContain(false);
  }
});

test.describe('numbers and days in the language of the page', () => {
  test.use({ locale: 'de-DE' });

  test('a German page writes the share, the seconds and the day the German way', async ({ page }) => {
    await fakePicker(page, exportFiles());
    await page.goto(pageUrl());
    await page.locator('button').first().click();
    await page.getByRole('heading', { name: 'Repositories' }).waitFor();
    await page.getByRole('button', { name: 'beta.nvim' }).click();
    const section = page.getByRole('region', { name: 'Verlauf' });
    await expect(section).toBeVisible();
    const text = await section.innerText();
    // 50 % with a space before the sign, a comma in a decimal, the day as day.month.year
    expect(text).toMatch(/50\s%/);
    expect(text).toMatch(/\b31\.10\.2026\b/);
    expect(text).not.toMatch(/Oct 31/);
  });
});
