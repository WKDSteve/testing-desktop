// Reads a component of the page as a tree, for the tests that hold a rule of the page over every component (identifiers
// are not translated, fields of identifiers are not spell-checked, nothing is formatted by hand): a component added
// later is read as well, without a change in the test.

import { parse } from 'svelte/compiler';

/** Every component of the page, read as text. */
export const COMPONENTS = import.meta.glob('./**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

interface RawAttr {
  type: string;
  name: string;
  value?: true | { type: string; data?: string }[] | { type: string };
}
interface RawNode {
  type: string;
  name?: string;
  start: number;
  attributes?: RawAttr[];
  [key: string]: unknown;
}

export interface Element {
  file: string;
  line: number;
  name: string;
  /** Static text value of an attribute; `true` for one without a value; `undefined` when there is none or it is computed. */
  attr(name: string): string | true | undefined;
  /** The element has the attribute at all (also a computed one). */
  has(name: string): boolean;
  /** The classes it names in a static `class="..."`. */
  classes: string[];
  /** The element and what is around it, the outermost first. */
  chain: Element[];
}

/** Every element (not component) of one component, with the elements around it. */
export function elementsOf(source: string, file: string): Element[] {
  const out: Element[] = [];
  const lineOf = (pos: number) => source.slice(0, pos).split('\n').length;
  function make(n: RawNode, chain: Element[]): Element {
    const find = (name: string) => n.attributes?.find((a) => a.type === 'Attribute' && a.name === name);
    const attr = (name: string): string | true | undefined => {
      const a = find(name);
      if (!a) return undefined;
      if (a.value === true) return true;
      if (!Array.isArray(a.value)) return undefined;
      const [only, ...more] = a.value;
      return only?.type === 'Text' && more.length === 0 ? only.data : undefined;
    };
    const cls = attr('class');
    const el: Element = {
      file,
      line: lineOf(n.start),
      name: n.name ?? '',
      attr,
      has: (name) => find(name) !== undefined,
      classes: typeof cls === 'string' ? cls.split(/\s+/).filter(Boolean) : [],
      chain: [],
    };
    el.chain = [...chain, el];
    return el;
  }
  function walk(value: unknown, chain: Element[]): void {
    if (!value || typeof value !== 'object') return;
    if (Array.isArray(value)) {
      for (const v of value) walk(v, chain);
      return;
    }
    const n = value as RawNode;
    let next = chain;
    if (n.type === 'RegularElement') {
      const el = make(n, chain);
      out.push(el);
      next = el.chain;
    }
    for (const [key, child] of Object.entries(n)) {
      if (key === 'attributes' || key === 'metadata' || key === 'parent') continue;
      walk(child, next);
    }
  }
  walk((parse(source, { modern: true }) as unknown as { fragment: unknown }).fragment, []);
  return out;
}

/** The text of the `<style>` block of a component ('' when it has none). */
export function styleOf(source: string): string {
  const m = /<style[^>]*>([\s\S]*?)<\/style>/.exec(source);
  return m?.[1] ?? '';
}

/** Where an element is: `File.svelte:12 <pre>`. */
export function where(el: Element): string {
  return `${el.file}:${el.line} <${el.name}>`;
}
