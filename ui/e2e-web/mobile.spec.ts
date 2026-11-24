import { createServer, type Server } from 'node:http';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, type Page } from '@playwright/test';
import { exportFiles } from './page';

// The page of testing mobil in a phone's window: the built page (dist-mobile) is served from a folder, and the shell of
// the app (Tauri's `invoke`) is a fake that answers the commands the page uses. The pairing form and the safe area are
// the page's own; the notch and the gesture bar of a real phone are the hand run on a device.

test.use({ locale: 'en-US', viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

const here = dirname(fileURLToPath(import.meta.url));
const DIST = resolve(here, '..', 'dist-mobile');
const TYPES: Record<string, string> = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' };

let server: Server;
let origin = '';

test.beforeAll(async () => {
  if (!existsSync(join(DIST, 'index.mobile.html'))) throw new Error(`${DIST} is not built: run "pnpm mobile:build" first`);
  server = createServer((req, res) => {
    const name = (req.url ?? '/').split('?')[0]!.replace(/^\/+/, '') || 'index.mobile.html';
    const file = resolve(DIST, name);
    if (!file.startsWith(DIST) || !existsSync(file)) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
  });
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
  const address = server.address();
  origin = `http://127.0.0.1:${typeof address === 'object' && address ? address.port : 0}`;
});
test.afterAll(() => server.close());

/**
 * The shell of the app, faked in the page: nobody is paired. `phone_pair` waits until the test lets it go and then
 * refuses with `bad_link`; choosing a vault folder answers with the golden export as the folder.
 */
async function fakeShell(page: Page): Promise<void> {
  await page.addInitScript((tree: Record<string, string>) => {
    let state = { paired: false, way: null, hub: null, name: null, confirm: false, vault: false };
    const win = window as unknown as Record<string, unknown>;
    win['__pairs'] = [] as unknown[];
    win['__TAURI_INTERNALS__'] = {
      invoke(cmd: string, args?: { request?: { link?: string }; path?: string }): Promise<unknown> {
        switch (cmd) {
          case 'phone_state':
            return Promise.resolve(state);
          case 'phone_pair':
            return new Promise((_, reject) => (win['__pairs'] as unknown[]).push(() => reject({ kind: 'bad_link' })));
          case 'vault_pick':
            state = { ...state, vault: true };
            return Promise.resolve(state);
          case 'vault_list': {
            const prefix = args?.path ? `${args.path}/` : '';
            const seen = new Map<string, { name: string; dir: boolean; size: number }>();
            for (const [path, text] of Object.entries(tree)) {
              if (!path.startsWith(prefix)) continue;
              const rest = path.slice(prefix.length);
              const head = rest.split('/')[0]!;
              seen.set(head, { name: head, dir: rest.includes('/'), size: rest.includes('/') ? 0 : text.length });
            }
            return Promise.resolve([...seen.values()]);
          }
          case 'vault_read':
            return Promise.resolve(tree[args?.path ?? ''] ?? '');
          default:
            return Promise.reject({ kind: 'failed' });
        }
      },
    };
  }, exportFiles());
}

test('the pairing form: usable while empty, says what is wrong at the field, waits with the same words', async ({ page }) => {
  await fakeShell(page);
  await page.goto(`${origin}/`);
  const go = page.getByRole('button', { name: 'Pair', exact: true });
  const link = page.getByLabel('Pairing link');
  await expect(go).toBeEnabled();

  // Nothing typed: the button works, the words stand at the field, and the cursor is in it.
  await go.click();
  await expect(link).toBeFocused();
  await expect(link).toHaveAttribute('aria-invalid', 'true');
  const problem = page.locator('#pair-link-problem');
  await expect(problem).toHaveText('Paste the pairing link first, or scan the code.');
  await expect(link).toHaveAttribute('aria-describedby', 'pair-link-problem');
  await expect(page.locator('.notice')).toHaveCount(0);

  // A link on its way: the button keeps its words, is locked and says it is busy; a second tap sends nothing.
  await link.fill('testing://pair/x');
  await expect(problem).toHaveCount(0);
  await go.click();
  await expect(go).toBeDisabled();
  await expect(go).toHaveAttribute('aria-busy', 'true');
  await expect(go).toHaveText('Pair');
  await expect(page.getByRole('status').filter({ hasText: 'Pairing …' })).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __pairs: unknown[] }).__pairs.length)).toBe(1);

  // The hub says no: the words are at the field again, the cursor goes back, the button is usable again.
  await page.evaluate(() => ((window as unknown as { __pairs: (() => void)[] }).__pairs[0] as () => void)());
  await expect(problem).toContainText('not a pairing link');
  await expect(link).toBeFocused();
  await expect(go).toBeEnabled();
  await expect(go).not.toHaveAttribute('aria-busy', 'true');
  await expect(page.locator('.notice')).toHaveCount(0);
});

test.describe('the safe area of a phone', () => {
  test('the bar, the page and the notice keep out of the notch and the gesture bar', async ({ page }) => {
    await fakeShell(page);
    // A phone with a notch: Chrome's override of the safe-area insets stands in for the device.
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Emulation.setSafeAreaInsetsOverride', { insets: { top: 47, bottom: 34, left: 0, right: 0 } });
    await page.goto(`${origin}/`);
    await page.getByRole('button', { name: 'Choose its folder' }).click();
    await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
    const px = (selector: string, property: string) =>
      page.locator(selector).first().evaluate((el, p) => parseFloat(getComputedStyle(el).getPropertyValue(p)), property);
    // the bar carries the room of the notch itself (and sticks to the very top, where the notch is)
    expect(await px('.bar', 'padding-top')).toBe(8 + 47);
    expect(await px('.bar', 'margin-top')).toBe(-47);
    expect(await px('.phone', 'padding-top')).toBe(47);
    expect(await px('.phone', 'padding-bottom')).toBe(32 + 34);
    // the first thing on the page is under the notch, not behind it
    const bar = await page.locator('header.bar').boundingBox();
    const first = await page.locator('header.bar .where').first().boundingBox();
    expect(bar!.y).toBe(0);
    expect(first!.y).toBeGreaterThanOrEqual(47);
  });

  test('the controls of the page are at least 44 px on a side', async ({ page }) => {
    await fakeShell(page);
    await page.goto(`${origin}/`);
    await page.getByRole('button', { name: 'Choose its folder' }).click();
    await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
    await page.getByRole('button', { name: 'beta.nvim' }).click();
    await expect(page.getByRole('heading', { name: 'beta.nvim' })).toBeVisible();
    const small = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>("button, select, summary, input:not([type='hidden'], [type='checkbox'])")]
        .map((el) => ({ el: `${el.tagName.toLowerCase()} ${(el.textContent ?? '').trim().slice(0, 16)}`, r: el.getBoundingClientRect() }))
        .filter(({ r }) => (r.width > 0 || r.height > 0) && (r.width < 43.5 || r.height < 43.5))
        .map(({ el, r }) => `${el} ${Math.round(r.width)}x${Math.round(r.height)}`),
    );
    expect(small).toEqual([]);
    const tap = await page.evaluate(() => getComputedStyle(document.querySelector('.phone')!).getPropertyValue('-webkit-tap-highlight-color'));
    expect(tap).toBe('rgba(0, 0, 0, 0)');
    expect(await page.evaluate(() => getComputedStyle(document.querySelector('button')!).touchAction)).toBe('manipulation');
  });
});
