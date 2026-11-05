<script lang="ts">
  // The page of the web variant: the way in until a folder is open, then the same views as on the hub, with how old the
  // export is. It looks at the folder when the window gets the focus back and every 30 seconds (the kit's watch).
  import type { MenuItem } from 'appkit-ui/MenuBar.svelte';
  import { watchFolder, type Watcher } from 'appkit-web/watch';
  import { untrack } from 'svelte';
  // The mark of the page, from assets/icons of this repository (rule APP-11); a data: URL in the single file.
  import logo from '../../../assets/icons/testing-web/testing-web-small.svg?url';
  import { langOf, textsOf, type Key } from '../lib/i18n';
  import { ageOf, shown } from '../lib/model';
  import Detail from '../views/Detail.svelte';
  import Frame from '../views/Frame.svelte';
  import Overview from '../views/Overview.svelte';
  import type { Entry, EntryState } from './entry';
  import { signatureOf } from './folder-source';
  import { WebReader } from './web-reader.svelte';
  import WebEntry from './WebEntry.svelte';

  let { entry, languages, build }: { entry: Entry; languages: readonly string[]; build: string | null } = $props();

  const t = $derived(textsOf(langOf(languages)));

  let reader = $state<WebReader | null>(null);
  let folderName = $state('');
  let now = $state(Date.now());
  let watcher: Watcher | null = null;

  /** The way in is left when a folder is open: the reader takes over, and the folder is watched when it can be. */
  async function enter(state: Extract<EntryState, { step: 'open' }>): Promise<void> {
    leave();
    const next = new WebReader(state.handle, state.snapshot);
    folderName = state.name;
    reader = next;
    await next.start();
    // Another folder was chosen meanwhile: this one is not to be watched.
    if (reader !== next) return;
    // A folder chosen through an input is a list of files made once: there is nothing to look at again.
    if (state.readOnce) return;
    // No signature yet: one taken now could already hold an export newer than what was read, which would then never
    // show. The first look reads again, and finds the same export or the newer one.
    watcher = watchFolder('', {
      signature: () => signatureOf(state.handle),
      onChange: () => next.reload(),
      onError: () => {
        // The folder is gone or the permission was taken back: the page stays on what it read.
      },
      setTimer: (fn, ms) => setInterval(fn, ms),
      clearTimer: (handle) => clearInterval(handle as ReturnType<typeof setInterval>),
      onFocus: (listener) => {
        window.addEventListener('focus', listener);
        document.addEventListener('visibilitychange', listener);
        return () => {
          window.removeEventListener('focus', listener);
          document.removeEventListener('visibilitychange', listener);
        };
      },
    });
  }

  function leave(): void {
    watcher?.stop();
    watcher = null;
    reader?.stop();
  }

  function otherFolder(): void {
    leave();
    reader = null;
    void entry.start();
  }

  $effect(() => {
    untrack(() => {
      void entry.start();
    });
    const off = entry.subscribe((state) => {
      if (state.step === 'open') void enter(state);
    });
    // The age of the export on the page goes on while nothing changes.
    const clock = setInterval(() => (now = Date.now()), 60_000);
    return () => {
      off();
      clearInterval(clock);
      leave();
    };
  });

  const help = $derived<MenuItem[]>([
    { kind: 'separator' },
    { kind: 'info', label: t('web.folder', { name: shown(folderName) ?? '' }) },
    { kind: 'action', label: t('web.other_folder'), onselect: otherFolder },
    ...(build ? [{ kind: 'info' as const, label: t('web.build', { build }) }] : []),
  ]);

  function ageWord(iso: string | null): string {
    const age = iso ? ageOf(iso, now) : null;
    return age ? t(`web.ago.${age.unit}` as Key, { n: age.n }) : '';
  }

  function timeWord(iso: string | null): string {
    const then = iso ? new Date(iso) : null;
    return then && !Number.isNaN(then.getTime()) ? then.toLocaleString(langOf(languages)) : (iso ?? '');
  }
</script>

{#if reader}
  <Frame app={reader} {t} {logo} {help}>
    <p class="export" role="status">
      {ageWord(reader.generatedAt)
        ? t('web.exported', { time: timeWord(reader.generatedAt), ago: ageWord(reader.generatedAt) })
        : t('web.exported_at', { time: timeWord(reader.generatedAt) })}
    </p>
    {#if reader.stale}
      <p class="stale" role="alert">
        {t('web.stale')}
        {t(`web.notice.${reader.stale.code}` as Key, { detail: shown(reader.stale.detail) ?? '' })}
      </p>
    {/if}
    {#if reader.openKey}
      <Detail app={reader} {t} />
    {:else if reader.phase === 'ready'}
      <Overview app={reader} {t} />
    {/if}
  </Frame>
{:else}
  <header class="plain"><span class="brand"><img src={logo} alt="" width="20" height="20" />{t('app.title')}</span></header>
  <main><WebEntry {entry} {t} /></main>
{/if}

<style>
  .export {
    color: var(--muted);
    margin: 0 0 0.5rem;
  }
  .stale {
    color: var(--bad);
    overflow-wrap: anywhere;
  }
  .plain {
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
</style>
