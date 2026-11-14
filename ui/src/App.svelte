<script lang="ts">
  // The page on the hub: what the session says while there is no fleet, then the overview or the open repository.
  import type { MenuItem } from 'appkit-ui/MenuBar.svelte';
  import { untrack } from 'svelte';
  // The mark of the app, from assets/icons of this repository (rule APP-11).
  import logo from '../../assets/icons/testing-desktop/testing-desktop-small.svg?url';
  import type { App } from './lib/app.svelte';
  import { DICTIONARIES, langOf, textsOf, type Key } from './lib/i18n';
  import { shown } from './lib/model';
  import Detail from './views/Detail.svelte';
  import Frame from './views/Frame.svelte';
  import Overview from './views/Overview.svelte';
  import Release from './views/Release.svelte';
  import Step from './views/Step.svelte';

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

  const help = $derived<MenuItem[]>(
    app.about
      ? [
          { kind: 'separator' },
          {
            kind: 'info',
            label: app.about.engine
              ? t('about.engine', {
                  version: shown(app.about.engine.version) ?? '?',
                  git: shown(app.about.engine.git) ?? '?',
                  nvim: shown(app.about.engine.nvim) ?? '?',
                })
              : t('about.no_engine'),
          },
          {
            kind: 'info',
            label: t('about.reads', { state: shown(app.about.reads.stateDir) ?? '?', areas: shown(app.about.reads.areas) ?? '?' }),
          },
          ...(ciWord(app.about.ci) ? [{ kind: 'info' as const, label: t(ciWord(app.about.ci) as Key) }] : []),
        ]
      : [],
  );
</script>

<svelte:window onfocus={onFocus} />

<Frame {app} {t} {logo} {help}>
  {#if app.phase === 'ready'}
    {#if app.releaseView}
      <Release {app} {t} />
    {:else if app.stepView}
      <Step {app} {t} />
    {:else if app.openKey}
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
  {:else if app.phase === 'reading'}
    <p class="state" role="status">{t('phase.reading')}</p>
  {:else if app.phase === 'failed'}
    <p class="state" role="status">{t('phase.failed', { message: app.failure ?? '' })}</p>
  {:else}
    <p class="state" role="status">{t('phase.starting')}</p>
  {/if}
</Frame>

<style>
  .state {
    color: var(--muted);
  }
</style>
