// A page translation of the browser rewrote ids, file names, commit hashes and the output of a test as if they were
// prose. What is an identifier, a path, a command or the name of the app says translate="no" (on itself or on an
// element around it); the words of the page stay translatable. This reads every component and names an element that
// forgot it.

import { describe, expect, it } from 'vitest';
import { COMPONENTS, elementsOf, where, type Element } from './pageRules';

/** Elements that are not prose: output, commands, identifiers and paths (`mono`), the name of the app, a repository name. */
const NOT_PROSE_CLASSES = new Set(['mono', 'brand', 'link']);

const notProse = (el: Element): boolean => el.name === 'pre' || el.name === 'code' || el.classes.some((c) => NOT_PROSE_CLASSES.has(c));
const untranslated = (el: Element): boolean => el.chain.some((e) => e.attr('translate') === 'no');

export function forgotten(source: string, file: string): string[] {
  return elementsOf(source, file)
    .filter((el) => notProse(el) && !untranslated(el))
    .map(where);
}

describe('identifiers, paths and output are not translated by the browser', () => {
  it('names an element that forgot it, and not one inside an element that has it', () => {
    const src = [
      '<pre>output</pre>',
      '<pre translate="no">output</pre>',
      '<span class="muted mono">a/b.lua</span>',
      '<div translate="no"><span class="mono">x</span><code>y</code></div>',
      '<span class="brand">testing</span>',
      '<p>Some prose that is translated.</p>',
    ].join('\n');
    expect(forgotten(src, 'T.svelte')).toEqual(['T.svelte:1 <pre>', 'T.svelte:3 <span>', 'T.svelte:5 <span>']);
  });

  it('in every component of the page', () => {
    expect(Object.keys(COMPONENTS).length).toBeGreaterThan(10);
    expect(Object.entries(COMPONENTS).flatMap(([file, source]) => forgotten(source, file.slice(2)))).toEqual([]);
  });
});
