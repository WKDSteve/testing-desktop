// Numbers, shares, days and moments as people read them: written with `Intl` in the language of the page, never by hand
// (`toFixed`, `slice`, a bare `toLocaleString()` that takes the language of the machine and not the one of the page).
// A page puts `formatsOf(lang)` into the context (`setFormats`), the views take it out again (`getFormats`).

import { getContext, setContext } from 'svelte';
import type { Lang } from './i18n';

export interface Formats {
  /** A number with at most `digits` places after the decimal separator (grouping of the language). */
  number(n: number, digits?: number): string;
  /** A share given in whole percent (`93` is "93%" in English, "93 %" in German). */
  percent(whole: number): string;
  /** The day of a timestamp or of a plain `YYYY-MM-DD`, medium length; a text that is no date comes back as it is. */
  day(iso: string): string;
  /** The moment of a timestamp (date and minutes); a text that is no date comes back as it is. */
  moment(iso: string): string;
  /** The moment of a Unix time in seconds. */
  momentOfSeconds(seconds: number): string;
}

const PLAIN_DAY = /^\d{4}-\d{2}-\d{2}$/;

export function formatsOf(lang: Lang): Formats {
  const numbers = new Map<number, Intl.NumberFormat>();
  const numberFormat = (digits: number): Intl.NumberFormat => {
    let f = numbers.get(digits);
    if (!f) {
      f = new Intl.NumberFormat(lang, { maximumFractionDigits: digits });
      numbers.set(digits, f);
    }
    return f;
  };
  const percent = new Intl.NumberFormat(lang, { style: 'percent', maximumFractionDigits: 1 });
  const dayFormat = new Intl.DateTimeFormat(lang, { dateStyle: 'medium' });
  // A day without a time is a calendar day, not a moment: in UTC it stays the day it says in every time zone.
  const plainDayFormat = new Intl.DateTimeFormat(lang, { dateStyle: 'medium', timeZone: 'UTC' });
  const momentFormat = new Intl.DateTimeFormat(lang, { dateStyle: 'medium', timeStyle: 'short' });
  const parse = (iso: string): Date | null => {
    const date = new Date(iso);
    return Number.isNaN(date.getTime()) ? null : date;
  };
  return {
    number: (n, digits = 1) => numberFormat(digits).format(n),
    percent: (whole) => percent.format(whole / 100),
    day(iso) {
      const date = parse(iso);
      if (date === null) return iso;
      return (PLAIN_DAY.test(iso) ? plainDayFormat : dayFormat).format(date);
    },
    moment(iso) {
      const date = parse(iso);
      return date === null ? iso : momentFormat.format(date);
    },
    momentOfSeconds: (seconds) => momentFormat.format(new Date(seconds * 1000)),
  };
}

const KEY = Symbol('testing-desktop.formats');

/** The page says in which language its numbers and days are written. */
export function setFormats(read: () => Lang): void {
  setContext(KEY, read);
}

/** The formats of the page this view sits in (English when no page said otherwise: a view shown alone). */
export function getFormats(): () => Formats {
  const read = getContext<(() => Lang) | undefined>(KEY);
  return () => formatsOf(read?.() ?? 'en');
}
