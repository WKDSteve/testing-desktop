// Fields that take an identifier, a path, a commit or a link are not spell-checked: the red squiggle under a repository
// name or a test id says nothing and hides what is typed. The fields of prose (a note, an answer, a device name) keep
// the spell check. This reads every component and names a field of the first kind that forgot `spellcheck="false"`.

import { describe, expect, it } from 'vitest';
import { COMPONENTS, elementsOf, where, type Element } from './pageRules';

/** A search box looks for names, ids and commits; a link and a file name are not words either. */
const IDENTIFIER_TYPES = new Set(['search', 'url']);
/** Text fields that take a file name (the `data-hint` names them: a name of a spec file). */
const IDENTIFIER_FIELDS = new Set(['run-file']);

const takesIdentifier = (el: Element): boolean =>
  el.name === 'input' && (IDENTIFIER_TYPES.has(String(el.attr('type'))) || el.chain.some((e) => e.classes.some((c) => IDENTIFIER_FIELDS.has(c))));

export function spellChecked(source: string, file: string): string[] {
  return elementsOf(source, file)
    .filter((el) => takesIdentifier(el) && el.attr('spellcheck') !== 'false')
    .map(where);
}

describe('no spell check under identifiers', () => {
  it('names a search box or link field that forgot it', () => {
    const src = [
      '<input type="search" />',
      '<input type="search" spellcheck="false" />',
      '<input type="url" />',
      '<input type="text" />',
      '<form class="run-file"><input type="text" /></form>',
    ].join('\n');
    expect(spellChecked(src, 'T.svelte')).toEqual(['T.svelte:1 <input>', 'T.svelte:3 <input>', 'T.svelte:5 <input>']);
  });

  it('in every component of the page', () => {
    expect(Object.entries(COMPONENTS).flatMap(([file, source]) => spellChecked(source, file.slice(2)))).toEqual([]);
  });
});
