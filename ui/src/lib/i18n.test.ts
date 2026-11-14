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

  it('names a rejected run as a workflow file GitHub refused, never as the runner guard', () => {
    // `rejected` of testing.nvim: GitHub did not take the workflow file. A job the guard refused is a `failure`.
    for (const lang of ['en', 'de'] as const) {
      for (const key of ['mobile.rerun.rejected', 'tip.ci.rejected'] as const) {
        const text = (DICTIONARIES[lang] as Record<string, string>)[key] ?? '';
        expect(text, `${lang} ${key}`).toMatch(/workflow/i);
        expect(text, `${lang} ${key}`).not.toMatch(/guard|testing ci release/i);
      }
    }
  });

  it('follows the browser and fills the placeholders', () => {
    expect(langOf(['de-AT', 'en'])).toBe('de');
    expect(langOf(['fr'])).toBe('en');
    expect(textsOf('en')('local.failed', { n: 3 })).toBe('3 failed');
  });
});
