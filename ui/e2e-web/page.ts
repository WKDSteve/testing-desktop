// What the tests of the web variant share: the built file, the golden export as a fake folder, and the watch on what the
// page must never do (a request, a violation of its policy; the kit's, appkit-web/e2e).

import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import type { Page } from '@playwright/test';
import { violations, watchPage as watchKitPage, type Watch } from 'appkit-web/e2e';

const here = dirname(fileURLToPath(import.meta.url));
export const FILE = resolve(here, '..', 'dist-web', 'testing.html');
const GOLDEN = resolve(here, '..', '..', 'crates', 'testing-hub', 'tests', 'fixtures', 'golden-export');

export type { Watch };
export { violations };

export function pageUrl(): string {
  if (!existsSync(FILE)) throw new Error(`${FILE} is not there: run "pnpm web:build" first`);
  return pathToFileURL(FILE).href;
}

export function watchPage(page: Page): Promise<Watch> {
  return watchKitPage(page, { pageUrl: pageUrl() });
}

/** The golden export: path relative to its root -> text. */
export function exportFiles(): Record<string, string> {
  const files: Record<string, string> = {};
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else files[relative(GOLDEN, path).replaceAll('\\', '/')] = readFileSync(path, 'utf8');
    }
  };
  walk(GOLDEN);
  return files;
}

const RECORDING = resolve(here, '..', '..', 'crates', 'testing-hub', 'tests', 'fixtures', 'recording');

/** The recording of a stepped run a real run of testing.nvim left: the folder with `events.jsonl`. */
export function recordingFiles(): Record<string, string> {
  return { 'events.jsonl': readFileSync(join(RECORDING, 'events.jsonl'), 'utf8') };
}

/** The recording with the events `match` picks changed by `change`. */
export function changedRecording(match: (event: Record<string, unknown>) => boolean, change: (event: Record<string, unknown>) => void): string {
  const lines = (recordingFiles()['events.jsonl'] as string).split('\n').filter((line) => line.trim() !== '');
  return (
    lines
      .map((line) => {
        const event = JSON.parse(line) as Record<string, unknown>;
        if (match(event)) change(event);
        return JSON.stringify(event);
      })
      .join('\n') + '\n'
  );
}

/** A file of a fake folder that is not text: its bytes, as the fake folder takes them. */
export function binary(bytes: Uint8Array): string {
  return `base64:${Buffer.from(bytes).toString('base64')}`;
}

/** The export with one document changed by `change`, and its size and sum in the manifest made right. */
export function changed(files: Record<string, string>, path: string, change: (doc: Record<string, unknown>) => void): Record<string, string> {
  const doc = JSON.parse(files[path] as string) as Record<string, unknown>;
  change(doc);
  const text = JSON.stringify(doc);
  const manifest = JSON.parse(files['testing-export.json'] as string) as { files: { path: string; bytes: number; sha256: string }[] };
  for (const entry of manifest.files) {
    if (entry.path === path) {
      entry.bytes = Buffer.byteLength(text);
      entry.sha256 = createHash('sha256').update(text).digest('hex');
    }
  }
  return { ...files, [path]: text, 'testing-export.json': JSON.stringify(manifest) };
}

/**
 * A `showDirectoryPicker` that hands out `files` as a folder, read-only like the page asks for it. `window.__disk.set` is
 * the other program in the folder: new text, newer time. The permission is `granted`. A handle of this fake does not
 * clone into IndexedDB, as a real one does: that the page keeps its folders is the hand run on a real machine.
 */
export async function fakePicker(page: Page, files: Record<string, string>, name = 'testing-results'): Promise<void> {
  await page.addInitScript(
    ({ tree, rootName }: { tree: Record<string, string>; rootName: string }) => {
      const enc = new TextEncoder();
      const disk = new Map<string, { bytes: Uint8Array; time: number }>();
      let clock = 1_700_000_000_000;
      // `base64:<...>` is a file that is not text (a picture of a recording).
      const bytesOf = (text: string) => (text.startsWith('base64:') ? Uint8Array.from(atob(text.slice(7)), (c) => c.charCodeAt(0)) : enc.encode(text));
      const put = (path: string, text: string) => disk.set(path, { bytes: bytesOf(text), time: (clock += 5000) });
      for (const [path, text] of Object.entries(tree)) put(path, text);
      const dir = (prefix: string, dirName: string): unknown => ({
        kind: 'directory',
        name: dirName,
        queryPermission: () => Promise.resolve('granted'),
        requestPermission: () => Promise.resolve('granted'),
        async *values() {
          const seen = new Set<string>();
          for (const [path, file] of disk) {
            if (!path.startsWith(prefix)) continue;
            const rest = path.slice(prefix.length);
            const head = rest.split('/')[0] as string;
            if (seen.has(head)) continue;
            seen.add(head);
            if (rest.includes('/')) {
              yield dir(`${prefix}${head}/`, head);
            } else {
              yield {
                kind: 'file',
                name: head,
                getFile: () =>
                  Promise.resolve({
                    size: file.bytes.length,
                    lastModified: file.time,
                    arrayBuffer: () => Promise.resolve(file.bytes.slice().buffer),
                  }),
              };
            }
          }
        },
      });
      const win = window as unknown as Record<string, unknown>;
      win['__pickerOptions'] = [] as unknown[];
      win['showDirectoryPicker'] = (options: unknown) => {
        (win['__pickerOptions'] as unknown[]).push(options);
        return Promise.resolve(dir('', rootName));
      };
      win['__disk'] = { set: put, remove: (path: string) => disk.delete(path) };
    },
    { tree: files, rootName: name },
  );
}

/** Open the page and the fake folder, up to the overview. */
export async function openExport(page: Page): Promise<void> {
  await page.goto(pageUrl());
  await page.getByRole('button', { name: 'Choose folder' }).click();
  await page.getByRole('heading', { name: 'Repositories' }).waitFor();
}
