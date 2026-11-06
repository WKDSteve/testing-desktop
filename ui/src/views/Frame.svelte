<script lang="ts">
  // The frame of both pages (the hub's and the web variant's): the bar with the mark and the menus, the palette and its
  // key, and the content. What is only one page's comes in: the mark, the lines of the help menu and the content.
  import { defaultKeymap, resolveKey, type ChordState } from 'appkit-ui/keymap';
  import MenuBar, { type Menu, type MenuItem } from 'appkit-ui/MenuBar.svelte';
  import type { Snippet } from 'svelte';
  import type { Key } from '../lib/i18n';
  import type { Reader } from '../lib/reader.svelte';
  import Palette from './Palette.svelte';

  let {
    app,
    t,
    logo,
    help,
    children,
  }: {
    app: Reader;
    t: (key: Key, params?: Record<string, string | number>) => string;
    logo: string;
    help: MenuItem[];
    children: Snippet;
  } = $props();

  const menus = $derived<Menu[]>([
    {
      id: 'view',
      label: t('menu.view'),
      items: [
        { kind: 'radio', label: t('menu.overview'), checked: app.openKey === null && !app.stepView, onselect: () => app.close() },
        ...(app.hasStep ? [{ kind: 'radio' as const, label: t('menu.step'), checked: app.stepView, onselect: () => app.showStep() }] : []),
        { kind: 'check', label: t('fleet.only_red'), checked: app.onlyRed, onselect: () => (app.onlyRed = !app.onlyRed) },
        { kind: 'separator' },
        { kind: 'action', label: t('menu.reload'), onselect: () => void app.reload() },
      ],
    },
    { id: 'help', label: t('menu.help'), items: [{ kind: 'info', label: t('help.keys') }, ...help] },
  ]);

  // The keyboard: Ctrl or Cmd and K open the palette, also from a field (the kit's map says so); no other key is used.
  let chord: ChordState = null;
  function onKeydown(event: KeyboardEvent): void {
    if (event.isComposing || app.phase !== 'ready') return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    const typing = target !== null && target.closest('input, textarea, select, [contenteditable="true"], [role="menu"], dialog') !== null;
    const result = resolveKey(
      defaultKeymap,
      { key: event.key, ctrlKey: event.ctrlKey, metaKey: event.metaKey, altKey: event.altKey, shiftKey: event.shiftKey, typing },
      chord,
      Date.now(),
      true,
    );
    chord = result.chord;
    if (result.command?.name !== 'palette') return;
    event.preventDefault();
    app.openPalette();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<header>
  <span class="brand"><img src={logo} alt="" width="20" height="20" />{t('app.title')}</span>
  {#if app.phase === 'ready'}
    <MenuBar {menus} label={t('menu.label')} />
  {/if}
</header>

{#if app.phase === 'ready'}
  <Palette {app} {t} />
{/if}

<main>
  {@render children()}
</main>

<style>
  header {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.6rem 1rem;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }
  .brand {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 700;
  }
  main {
    padding: 1rem;
    max-width: 80rem;
  }
</style>
