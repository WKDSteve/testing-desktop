<script lang="ts">
  // The frame of the page: what the session says while there is no fleet, then the overview or the open repository.
  import { untrack } from 'svelte';
  import type { App } from './lib/app.svelte';
  import { langOf, textsOf } from './lib/i18n';
  import Detail from './views/Detail.svelte';
  import Overview from './views/Overview.svelte';

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
</script>

<svelte:window onfocus={onFocus} />

<header>
  <span class="brand">{t('app.title')}</span>
</header>

<main>
  {#if app.phase === 'ready'}
    {#if app.openKey}
      <Detail {app} {t} />
    {:else}
      <Overview {app} {t} />
    {/if}
  {:else if app.phase === 'needs_pairing'}
    <p class="state" role="status">{app.pairRefused ? t('phase.pair_refused') : t('phase.needs_pairing')}</p>
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
    padding: 0.6rem 1rem;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }
  .brand {
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
