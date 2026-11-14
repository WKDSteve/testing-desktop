import { deflateSync } from 'node:zlib';
import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { binary, changedRecording, exportFiles, fakePicker, pageUrl, recordingFiles, violations, watchPage, type Watch } from './page';

// The single file on the recording of a stepped run (`events.jsonl`, as a real run of testing.nvim left it): the run is
// played back with the view of the hub's "Step by step" page, and the page still does nothing it must not: no request,
// no violation of its policy, no error.

test.use({ locale: 'en-US' });

async function clean(page: Page, seen: Watch): Promise<void> {
  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
  expect(await violations(page)).toEqual([]);
}

async function openRecording(page: Page, files: Record<string, string> = recordingFiles()): Promise<void> {
  await fakePicker(page, files, 'demo.greeting');
  await page.goto(pageUrl());
  await page.getByRole('button', { name: 'Choose folder' }).click();
  await page.getByRole('heading', { name: /The greeting buffer/ }).waitFor();
}

/** A PNG of one colour, `width` x `height`, made here: no file of the repository is a picture of a run. */
function png(width: number, height: number): Uint8Array {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (bytes: Uint8Array) => {
    let c = 0xffffffff;
    for (const b of bytes) c = (crcTable[(c ^ b) & 0xff] as number) ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type: string, data: Uint8Array) => {
    const body = new Uint8Array([...Buffer.from(type, 'ascii'), ...data]);
    const out = Buffer.alloc(8 + data.length + 4);
    out.writeUInt32BE(data.length, 0);
    out.set(body, 4);
    out.writeUInt32BE(crc(body), 4 + body.length);
    return out;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8); // 8 bit, RGB
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(width * 3, 0x66)]);
  const pixels = deflateSync(Buffer.concat(Array.from({ length: height }, () => row)));
  return new Uint8Array(Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', header), chunk('IDAT', pixels), chunk('IEND', new Uint8Array())]));
}

test('a folder with a recording is played back: the sheet, how it ended, the failed step and its screen', async ({ page }) => {
  const seen = await watchPage(page);
  await openRecording(page);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('The greeting buffer [rpc]');
  await expect(page.getByText('read from the folder demo.greeting. Nothing runs here.')).toBeVisible();
  // It opens at its end: the verdict of the run.
  await expect(page.getByText(/^finished: fail in \d+ ms$/)).toBeVisible();
  const steps = page.getByRole('list', { name: 'Steps' }).getByRole('listitem');
  await expect(steps).toHaveCount(4);
  await expect(steps.nth(0)).toContainText('pass');
  await expect(steps.nth(2)).toContainText('fail');
  await expect(steps.nth(3)).toContainText('not run');
  // The tile is the last step that did something: the failed one, with its failure line, its note and the screen.
  await expect(page.getByRole('heading', { name: 'Step 3, expect' })).toBeVisible();
  await expect(page.getByText('Note: the cursor did move')).toBeVisible();
  await expect(page.getByText(/80x24 step 3, the cursor is on line 2 .*got false/).first()).toBeVisible();
  await expect(page.locator('pre.screen')).toContainText('hello from the sheet');
  await expect(page.getByText('This run counts as a verdict.')).toBeVisible();
  // A recording takes no command: nothing of the hub's run controls is here, and no breakpoint.
  for (const name of ['Next', 'Continue', 'Abort', 'Add note']) await expect(page.getByRole('button', { name, exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: /Breakpoint/ })).toHaveCount(0);
  await clean(page, seen);
});

test('the recording is walked with the buttons and the keys, and a click on a step shows its tile', async ({ page }) => {
  const seen = await watchPage(page);
  await openRecording(page);
  const button = (name: string) => page.getByRole('toolbar', { name: 'Recording' }).getByRole('button', { name });
  const steps = page.getByRole('list', { name: 'Steps' }).getByRole('listitem');
  await expect(button('To the end')).toBeDisabled();
  await expect(button('Step forward')).toBeDisabled();

  await button('To the start').click();
  await expect(page.getByText('0 of 3 steps played')).toBeVisible();
  await expect(steps.nth(0)).toContainText('next');
  await expect(steps.nth(2)).toContainText('not yet');
  await expect(button('Step back')).toBeDisabled();
  await expect(page.getByText(/^finished:/)).toHaveCount(0);

  // The keys: n forward, p back, s start, e end.
  await page.keyboard.press('n');
  await expect(page.getByText('1 of 3 steps played')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Step 1, lua' })).toBeVisible();
  await expect(steps.nth(0)).toContainText('pass');
  await expect(steps.nth(1)).toContainText('next');
  await page.keyboard.press('n');
  await expect(page.getByText('2 of 3 steps played')).toBeVisible();
  await page.keyboard.press('p');
  await expect(page.getByText('1 of 3 steps played')).toBeVisible();
  await page.keyboard.press('e');
  await expect(page.getByText(/^finished: fail/)).toBeVisible();
  await page.keyboard.press('s');
  await expect(page.getByText('0 of 3 steps played')).toBeVisible();

  // The same with the keyboard alone on the buttons: the focus reaches them and Enter moves.
  await button('Step forward').focus();
  await page.keyboard.press('Enter');
  await page.keyboard.press('Enter');
  await expect(page.getByText('2 of 3 steps played')).toBeVisible();

  // A click on a step that ran shows what it did then.
  await steps.nth(0).getByRole('button').click();
  await expect(page.getByRole('heading', { name: 'Step 1, lua' })).toBeVisible();
  await expect(page.getByText('write two lines').last()).toBeVisible();
  await clean(page, seen);
});

test('a step that was skipped here reads as skipped, not as passed, in the list and in its tile', async ({ page }, testInfo) => {
  const seen = await watchPage(page);
  const events = changedRecording(
    (e) => e['event'] === 'step-after' && e['n'] === 1,
    (e) => {
      e['status'] = 'skip';
      e['reason'] = 'a shot needs a browser';
    },
  );
  await openRecording(page, { 'events.jsonl': events });
  const first = page.getByRole('list', { name: 'Steps' }).getByRole('listitem').nth(0);
  await expect(first).toContainText('skipped here');
  await expect(first).not.toContainText('pass');
  await expect(first).toHaveClass(/(^| )skip( |$)/);
  await expect(page.getByRole('list', { name: 'Steps' }).getByRole('listitem').nth(1)).toContainText('pass');
  await first.getByRole('button').click();
  await expect(page.getByRole('heading', { name: 'Step 1, lua' })).toBeVisible();
  await expect(page.getByText('skipped here', { exact: true }).last()).toBeVisible();
  await expect(page.getByText('a shot needs a browser')).toBeVisible();
  for (const colorScheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme });
    expect(await new AxeBuilder({ page }).analyze().then((r) => r.violations)).toEqual([]);
    await testInfo.attach(`skip-${colorScheme}`, { body: await page.screenshot(), contentType: 'image/png' });
  }
  await clean(page, seen);
});

test('a picture of a step is read from the folder when its tile shows, and only a file the recording named', async ({ page }) => {
  const seen = await watchPage(page);
  const events = changedRecording(
    (e) => e['event'] === 'step-after' && (e['n'] === 1 || e['n'] === 2),
    (e) => {
      // Step 1 names a picture of the folder; step 2 names a way out of it.
      e['image'] = e['n'] === 1 ? '80x24-1.png' : '../outside.png';
    },
  );
  await openRecording(page, { 'events.jsonl': events, '80x24-1.png': binary(png(7, 5)), 'outside.png': binary(png(3, 3)) });
  await page.getByRole('list', { name: 'Steps' }).getByRole('listitem').nth(0).getByRole('button').click();
  const picture = page.getByRole('img', { name: 'Picture of the target after this step' });
  await expect(picture).toBeVisible();
  await expect.poll(() => picture.locator('canvas').evaluate((c: HTMLCanvasElement) => [c.width, c.height])).toEqual([7, 5]);
  await expect(page.getByText(/The picture could not be read/)).toHaveCount(0);
  await page.getByRole('list', { name: 'Steps' }).getByRole('listitem').nth(1).getByRole('button').click();
  await expect(page.getByRole('heading', { name: 'Step 2, keys' })).toBeVisible();
  await expect(picture).toHaveCount(0);
  await clean(page, seen);
});

test('a recording that is no good stays on the way in and says why, and "Other folder" leads back to it', async ({ page }) => {
  const seen = await watchPage(page);
  const cases: [string, RegExp][] = [
    ['not json\n', /no event of the contract: events\.jsonl:1: not JSON/],
    [changedRecording((e) => e['event'] === 'end', (e) => (e['status'] = 'maybe')), /no event of the contract: events\.jsonl:15\/status \(enum\)/],
    [changedRecording(() => true, (e) => (e['schema'] = 'testing-sheet-session/2')), /too old for that recording \(testing-sheet-session\/2\)/],
    ['\n', /recording is empty/],
  ];
  for (const [events, message] of cases) {
    const inner = await page.context().newPage();
    const watch = await watchPage(inner);
    await fakePicker(inner, { 'events.jsonl': events }, 'demo.greeting');
    await inner.goto(pageUrl());
    await inner.getByRole('button', { name: 'Choose folder' }).click();
    await expect(inner.getByRole('alert')).toContainText(message);
    await expect(inner.getByRole('button', { name: 'Choose folder' })).toBeVisible();
    await clean(inner, watch);
    await inner.close();
  }
  await openRecording(page);
  await page.getByRole('button', { name: 'Other folder…' }).click();
  await expect(page.getByRole('heading', { name: 'Open an export folder' })).toBeVisible();
  await expect(page.getByText('It also plays back the recording')).toBeVisible();
  await clean(page, seen);
});

test('an export stays an export: a folder with both shows the fleet, as before', async ({ page }) => {
  const seen = await watchPage(page);
  await fakePicker(page, { ...exportFiles(), ...recordingFiles() });
  await page.goto(pageUrl());
  await page.getByRole('button', { name: 'Choose folder' }).click();
  await expect(page.getByRole('heading', { name: 'Repositories' })).toBeVisible();
  await expect(page.getByRole('toolbar', { name: 'Recording' })).toHaveCount(0);
  await clean(page, seen);
});

for (const scheme of ['light', 'dark'] as const) {
  test(`the replay is accessible in ${scheme}: at its end, at its start and with the focus on a button`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await openRecording(page);
    const axe = async (what: string) => {
      const { violations: found } = await new AxeBuilder({ page }).analyze();
      expect(found.map((v) => `${what}: ${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
    };
    await axe('the end');
    await page.keyboard.press('s');
    await page.getByText('0 of 3 steps played').waitFor();
    await axe('the start');
    const forward = page.getByRole('button', { name: 'Step forward' });
    await forward.hover();
    await forward.focus();
    await axe('a button under the pointer with the focus ring');
    await page.keyboard.press('Enter');
    await page.getByRole('heading', { name: 'Step 1, lua' }).waitFor();
    await axe('the tile of a step with its screen');
  });
}
