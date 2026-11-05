// What the palette can do for the state the page is in, and which repositories it can open. The words of a
// repository are foreign text: the palette shows them through the kit's hidden-character guard.

import type { PaletteAction, PaletteEntry } from 'appkit-ui/palette';
import type { Reader } from './reader.svelte';
import type { Key } from './i18n';

type Say = (key: Key, params?: Record<string, string | number>) => string;

export function paletteActions(app: Reader, t: Say): PaletteAction[] {
  const out: PaletteAction[] = [];
  if (app.openKey !== null) out.push({ key: 'go.overview', label: t('menu.overview'), hint: '', run: () => app.close() });
  out.push({
    key: 'only_red',
    label: app.onlyRed ? t('palette.only_red_off') : t('palette.only_red_on'),
    hint: '',
    run: () => {
      app.onlyRed = !app.onlyRed;
    },
  });
  out.push({ key: 'reload', label: t('menu.reload'), hint: '', run: () => app.reload() });
  return out;
}

/** The repositories of the fleet, by key and name. */
export function paletteEntries(app: Reader): PaletteEntry[] {
  return (app.fleet?.repos ?? []).map((r) => ({ id: r.key, title: r.name }));
}
