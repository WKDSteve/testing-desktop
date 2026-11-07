<script lang="ts">
  // The way in of the web variant: choose the export folder, or take one used before. What happens is the state
  // machine's (entry.ts); this draws its state and passes the clicks on.
  import { untrack } from 'svelte';
  import type { Key } from '../lib/i18n';
  import { shown } from '../lib/model';
  import type { Entry, EntryState } from './entry';

  let { entry, t }: { entry: Entry; t: (key: Key, params?: Record<string, string | number>) => string } = $props();

  // The state at the start comes from the machine, once; later states come through the subscription.
  let view = $state<EntryState>(untrack(() => entry.state));
  $effect(() => {
    view = entry.state;
    return entry.subscribe((next) => (view = next));
  });
</script>

<section class="entry" aria-labelledby="web-entry-title" aria-busy={view.step === 'opening'}>
  <h1 id="web-entry-title">{t('web.entry.title')}</h1>

  {#if view.step === 'unsupported'}
    <p role="alert">{t(`web.entry.unsupported.${view.reason}` as Key)}</p>
  {:else}
    <p>{t('web.entry.lead')}</p>

    {#if view.step === 'choose' && view.notice}
      <p role="alert" class="notice">{t(`web.notice.${view.notice.code}` as Key, { detail: shown(view.notice.detail) ?? '' })}</p>
    {/if}

    {#if view.step === 'opening' || view.step === 'open'}
      <p role="status">{t('web.entry.opening', { name: shown(view.name) ?? '' })}</p>
    {:else if view.step === 'choose'}
      {#if view.readOnce}
        <p class="muted">{t('web.entry.readonly')}</p>
      {/if}
      <p><button type="button" class="primary" onclick={() => void entry.pick()}>{t('web.entry.pick')}</button></p>

      {#if view.recent.length > 0}
        <h2>{t('web.entry.recent')}</h2>
        <ul class="recent">
          {#each view.recent as item (item.id)}
            <li>
              <span class="name">{shown(item.name)}</span>
              <span class="muted state">{t(`web.entry.state.${item.access}` as Key)}</span>
              {#if item.access !== 'gone'}
                <button type="button" onclick={() => void entry.openRecent(item.id)}>
                  {item.access === 'granted' ? t('web.entry.open') : t('web.entry.allow')}
                </button>
              {/if}
              <button type="button" onclick={() => void entry.forget(item.id)}>{t('web.entry.forget')}</button>
            </li>
          {/each}
        </ul>
        {#if view.recent.some((item) => item.access === 'prompt')}
          <p class="muted">{t('web.entry.allow_hint')}</p>
        {/if}
      {/if}
    {/if}
  {/if}
</section>

<style>
  .entry {
    max-width: 38rem;
    margin: 3rem auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1.25rem;
  }
  h1 {
    font-size: 1.25rem;
    margin: 0 0 0.5rem;
  }
  h2 {
    font-size: 1rem;
  }
  .muted {
    color: var(--muted);
  }
  button {
    background: var(--surface-2);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.25rem 0.75rem;
    cursor: pointer;
  }
  button.primary {
    background: var(--accent);
    color: var(--accent-text);
    border-color: var(--accent);
  }
  button.primary:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }
  .notice {
    color: var(--bad);
    overflow-wrap: anywhere;
  }
  .recent {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.5rem;
  }
  .recent li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }
  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .state {
    flex: 1 1 auto;
  }
</style>
