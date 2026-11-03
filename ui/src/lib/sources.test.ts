// Rules of the source code of the page, checked by reading it: no Tauri in the page (the same page runs in the
// browser, in a native shell and as a panel), no request outside the one transport (`source.ts`), no HTML from data.

import { describe, expect, it } from 'vitest';

// Every module of the page as its text (the bundler reads them; the tests themselves are left out).
const modules = import.meta.glob(['../**/*.ts', '../**/*.svelte', '!../**/*.test.ts'], {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

const sources = Object.entries(modules).map(([path, text]) => ({ path, text }));

describe('the source of the page', () => {
  it('is there to be read', () => {
    expect(sources.length).toBeGreaterThan(5);
    expect(sources.some((s) => s.path.endsWith('source.ts'))).toBe(true);
  });

  it('uses no Tauri API', () => {
    for (const { path, text } of sources) {
      expect(text, path).not.toMatch(/__TAURI__|@tauri-apps|\binvoke\(/);
    }
  });

  it('makes requests through the source only', () => {
    for (const { path, text } of sources) {
      if (path.endsWith('/source.ts') || path.endsWith('/main.ts')) continue;
      expect(text, path).not.toMatch(/\bfetch\(|XMLHttpRequest|WebSocket|EventSource|sendBeacon/);
    }
  });

  it('never renders data as HTML', () => {
    for (const { path, text } of sources) {
      expect(text, path).not.toMatch(/\{@html|innerHTML|outerHTML|insertAdjacentHTML/);
    }
  });
});
