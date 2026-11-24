// Numbers, shares, days and moments are written with `Intl` in the language of the page (lib/format.ts), never by hand:
// `toFixed` leaves out the grouping and the decimal separator of the language, and a bare `toLocaleString()` takes the
// language of the machine instead of the one of the page. This reads the sources of the page and names such a place.

import { describe, expect, it } from 'vitest';

const SOURCES = import.meta.glob(['./**/*.svelte', './**/*.ts', '!./**/*.test.ts', '!./**/*.generated.*'], {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

/** `toFixed(`, or a locale call without the language of the page (a language is the first argument, and not `undefined`). */
const BY_HAND = /\.toFixed\(|\.toLocale(?:Date|Time)?String\(\s*(?:\)|undefined)/;

export function byHand(source: string, file: string): string[] {
  return source
    .split('\n')
    .map((text, i) => ({ text, line: i + 1 }))
    .filter(({ text }) => BY_HAND.test(text))
    .map(({ line }) => `${file}:${line}`);
}

describe('nothing is formatted by hand', () => {
  it('finds a hand-built number and a locale call without the page language', () => {
    expect(byHand('a.toFixed(1)\nb.toLocaleString()\nc.toLocaleDateString(undefined, {})\nd.toLocaleString(lang)', 'x.ts')).toEqual([
      'x.ts:1',
      'x.ts:2',
      'x.ts:3',
    ]);
  });

  it('in every source of the page', () => {
    expect(Object.keys(SOURCES).length).toBeGreaterThan(30);
    expect(Object.entries(SOURCES).flatMap(([file, source]) => byHand(source, file.slice(2)))).toEqual([]);
  });
});
