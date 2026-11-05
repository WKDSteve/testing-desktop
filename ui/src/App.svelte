<script lang="ts">
  // The frame of the page: what the session says while there is no fleet, then the overview or the open repository.
  import { untrack } from 'svelte';
  // The mark of the app, from assets/icons of this repository (rule APP-11).
  import logo from '../../assets/icons/testing-desktop/testing-desktop-small.svg?url';
  import { defaultKeymap, resolveKey, type ChordState } from 'appkit-ui/keymap';
  import MenuBar, { type Menu } from 'appkit-ui/MenuBar.svelte';
  import type { App } from './lib/app.svelte';
  import { DICTIONARIES, langOf, textsOf, type Key } from './lib/i18n';
  import { shown } from './lib/model';
  import Detail from './views/Detail.svelte';
  import Overview from './views/Overview.svelte';
  import Palette from './views/Palette.svelte';

  let { app, code, languages }: { app: App; code: string | null; languages: readonly string[] } = $props();

  const t = $derived(textsOf(langOf(languages)));

  $effect(() => {
    // untrack: start reads the state it writes; tracked, the effect would start the page again and again.
    untrack(() => void app.start(code));
    return () => app.stop();
  });

  function onFocus(): void {
    if (app.phase === 'ready') void app.refresh();
  }

  /** The words of what `about` says of GitHub; a word this page does not know says nothing. */
  function ciWord(word: string | null): Key | null {
    const key = `about.ci.${word ?? ''}`;
    return key in DICTIONARIES.en ? (key as Key) : null;
  }

  const menus = $derived<Menu[]>([
    {
      id: 'view',
      label: t('menu.view'),
      items: [
        { kind: 'radio', label: t('menu.overview'), checked: app.openKey === null, onselect: () => app.close() },
        { kind: 'check', label: t('fleet.only_red'), checked: app.onlyRed, onselect: () => (app.onlyRed = !app.onlyRed) },
        { kind: 'separator' },
        { kind: 'action', label: t('menu.reload'), onselect: () => void app.reload() },
      ],
    },
    {
      id: 'help',
      label: t('menu.help'),
      items: [
        { kind: 'info', label: t('help.keys') },
        ...(app.about
          ? [
              { kind: 'separator' as const },
              {
                kind: 'info' as const,
                label: app.about.engine
                  ? t('about.engine', {
                      version: shown(app.about.engine.version) ?? '?',
                      git: shown(app.about.engine.git) ?? '?',
                      nvim: shown(app.about.engine.nvim) ?? '?',
                    })
                  : t('about.no_engine'),
              },
              {
                kind: 'info' as const,
                label: t('about.reads', { state: shown(app.about.reads.stateDir) ?? '?', areas: shown(app.about.reads.areas) ?? '?' }),
              },
              ...(ciWord(app.about.ci) ? [{ kind: 'info' as const, label: t(ciWord(app.about.ci) as Key) }] : []),
            ]
          : []),
      ],
    },
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

<svelte:window onfocus={onFocus} onkeydown={onKeydown} />

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
  {#if app.phase === 'ready'}
    {#if app.openKey}
      <Detail {app} {t} />
    {:else}
      <Overview {app} {t} />
    {/if}
  {:else if app.phase === 'needs_pairing'}
    <p class="state" role="status">{app.pairLimited ? t('phase.pair_limited') : app.pairRefused ? t('phase.pair_refused') : t('phase.needs_pairing')}</p>
  {:else if app.phase === 'offline'}
    <p class="state" role="status">{t('phase.offline')}</p>
  {:else if app.phase === 'unavailable'}
    <p class="state" role="status">{t('phase.unavailable', { problem: app.problem ?? '' })}</p>
  {:else if app.phase === 'failed'}
    <p class="state" role="status">{t('phase.failed', { message: app.failure ?? '' })}</p>
  {:else}
    <p class="state" role="status">{t('phase.starting')}</p>
  {/if}
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
  .state {
    color: var(--muted);
  }
</style>
