import { expect, test } from './fixture';

// What the hub serves of its own page, measured on the real binary: the files of the build carry a hash in their name
// (so a new build is a new name), and the hub answers every file of the page, the page itself included, with
// `Cache-Control: no-store` and without compression. That is the decision for a hub on this machine (the page is read
// from the loopback address, a few hundred kilobytes, once per tab; nothing can be served stale after an update). A
// hosted copy of the page would have to cache the hashed files for long and compress them; this test is the place to
// change when the hub ever serves more than its own machine (docs/SECURITY.md).

test('the page of the hub: hashed file names, never cached, not compressed', async ({ request }) => {
  const page = await request.get('/', { headers: { 'accept-encoding': 'gzip, br' } });
  expect(page.status()).toBe(200);
  expect(page.headers()['cache-control']).toBe('no-store');
  expect(page.headers()['content-encoding']).toBeUndefined();

  const html = await page.text();
  const files = [...html.matchAll(/(?:src|href)="(\.?\/?assets\/[^"]+)"/g)].map((m) => (m[1] as string).replace(/^\.?\//, '/'));
  expect(files.length).toBeGreaterThanOrEqual(2);
  for (const file of files) {
    // name-<hash>.ext: eight characters of a content hash (Vite's default), so a changed file is a new name
    expect(file, file).toMatch(/^\/assets\/[\w.-]+-[A-Za-z0-9_-]{8}\.(js|css|svg)$/);
    const answer = await request.get(file, { headers: { 'accept-encoding': 'gzip, br' } });
    expect(answer.status(), file).toBe(200);
    expect(answer.headers()['cache-control'], file).toBe('no-store');
    expect(answer.headers()['content-encoding'], file).toBeUndefined();
  }
});
