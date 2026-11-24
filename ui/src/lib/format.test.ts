import { describe, expect, it } from 'vitest';
import { formatsOf } from './format';

describe('numbers, shares and days in the language of the page', () => {
  const en = formatsOf('en');
  const de = formatsOf('de');

  it('groups and separates as the language does, and keeps at most one place', () => {
    expect(en.number(1234.56)).toBe('1,234.6');
    expect(de.number(1234.56)).toBe('1.234,6');
    expect(en.number(2)).toBe('2');
    expect(de.number(0.25, 2)).toBe('0,25');
  });

  it('writes a share with the percent sign of the language', () => {
    expect(en.percent(93)).toBe('93%');
    expect(de.percent(93).replace(/\s/g, ' ')).toBe('93 %');
  });

  it('writes a day and a moment in the language, and leaves what is no date alone', () => {
    expect(en.day('2026-11-24')).toBe('Nov 24, 2026');
    expect(de.day('2026-11-24')).toBe('24.11.2026');
    expect(en.day('2026-11-24T10:00:00Z')).toContain('2026');
    expect(en.moment('2026-11-24T10:00:00Z')).not.toBe(de.moment('2026-11-24T10:00:00Z'));
    expect(en.day('yesterday')).toBe('yesterday');
    expect(de.moment('not a time')).toBe('not a time');
    expect(en.momentOfSeconds(0)).not.toBe(de.momentOfSeconds(0));
  });
});
