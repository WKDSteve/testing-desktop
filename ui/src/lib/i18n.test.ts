import { describe, expect, it } from 'vitest';
import { DICTIONARIES, langOf, textsOf } from './i18n';

describe('the words of the page', () => {
  it('has the same keys and the same placeholders in every language', () => {
    const en = DICTIONARIES.en as Record<string, string>;
    const de = DICTIONARIES.de as Record<string, string>;
    expect(Object.keys(de).sort()).toEqual(Object.keys(en).sort());
    const holes = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    for (const key of Object.keys(en)) {
      expect(holes(de[key] ?? ''), key).toEqual(holes(en[key] ?? ''));
      expect((de[key] ?? '').trim(), key).not.toBe('');
    }
  });

  it('follows the browser and fills the placeholders', () => {
    expect(langOf(['de-AT', 'en'])).toBe('de');
    expect(langOf(['fr'])).toBe('en');
    expect(textsOf('en')('local.failed', { n: 3 })).toBe('3 failed');
  });
});
