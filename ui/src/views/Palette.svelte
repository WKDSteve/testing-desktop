<script lang="ts">
  // The command palette (Ctrl+K): the box, the list and the keys are the kit's; this file gives it the actions of the
  // page, the repositories of the fleet and the words. Nothing is remembered in the browser (frecency stays empty).
  import KitPalette from 'appkit-ui/Palette.svelte';
  import { paletteItems, type PaletteItem } from 'appkit-ui/palette';
  import type { Reader } from '../lib/reader.svelte';
  import type { Key } from '../lib/i18n';
  import { paletteActions, paletteEntries } from '../lib/paletteActions';

  let { app, t }: { app: Reader; t: (key: Key, params?: Record<string, string | number>) => string } = $props();

  function items(query: string): PaletteItem[] {
    return paletteItems(query, paletteActions(app, t), paletteEntries(app), {}, Date.now());
  }
</script>

<KitPalette
  open={app.paletteOpen}
  {items}
  onchoose={(item) => app.choose(item)}
  onclose={() => app.closePalette()}
  labels={{
    title: t('palette.title'),
    label: t('palette.label'),
    placeholder: t('palette.placeholder'),
    none: t('palette.empty'),
    action: t('palette.action'),
    entry: t('palette.repo'),
  }}
/>
