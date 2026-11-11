<script lang="ts">
  // The page of testing mobil: pairing (QR code or link) or the folder of a cloned export; then the fleet and one
  // repository, with the views of the hub's page, under a bar that always says the way, the age and what went wrong
  // (MUI-05, MOB-09). A red run can be started again, confirmed twice: here, and on the phone by its lock (MUI-01,
  // MOB-03). Notices of a red main are switched on in the settings (UnifiedPush; MOB-09 when there is none).
  import { nextStep, WAY_ABILITIES } from 'appkit-ui/mobile';
  import { untrack } from 'svelte';
  import { DICTIONARIES, langOf, textsOf, type Key } from '../lib/i18n';
  import Detail from '../views/Detail.svelte';
  import Overview from '../views/Overview.svelte';
  import { readExport } from '../web/folder-source';
  import { WebReader } from '../web/web-reader.svelte';
  import { MobileReader } from './mobile-reader.svelte';
  import { PhoneSource, vaultFolder } from './phone-source';
  import { ShellError, type PhoneState, type PushStatus, type Shell } from './shell';

  let { shell, languages }: { shell: Shell; languages: readonly string[] } = $props();

  const t = $derived(textsOf(langOf(languages)));

  let phone = $state<PhoneState | null>(null);
  let reader = $state<MobileReader | WebReader | null>(null);
  let link = $state('');
  let name = $state('');
  let busy = $state(false);
  let notice = $state<string | null>(null);
  let settings = $state(false);
  let askRerun = $state(false);
  let askUnpair = $state(false);
  let push = $state<PushStatus | null>(null);

  /** The words of a refusal of the shell or the hub, by its word; a word without words of its own says it failed. */
  function said(prefix: 'mobile.err' | 'mobile.rerun', error: unknown): string {
    const kind = error instanceof ShellError ? error.kind : 'failed';
    const key = `${prefix}.${kind}`;
    return key in DICTIONARIES.en ? t(key as Key) : t(`${prefix}.failed`);
  }

  async function open(state: PhoneState): Promise<void> {
    reader?.stop();
    reader = null;
    phone = state;
    if (state.vault) {
      try {
        const handle = vaultFolder(shell);
        const first = await readExport(handle);
        const web = new WebReader(handle, first);
        reader = web;
        await web.start();
      } catch {
        notice = t('mobile.next.vault');
      }
    } else if (state.paired) {
      const mobile = new MobileReader(new PhoneSource(shell));
      reader = mobile;
      await mobile.start();
      if (state.way === 'tailnet') push = await shell.pushStatus().catch(() => null);
    }
  }

  $effect(() => {
    untrack(() => {
      void shell.state().then(open, () => (notice = t('mobile.err.failed')));
    });
    return () => reader?.stop();
  });

  // The hub does not take this phone any more: the shell forgot the pairing and its keys; the page pairs again.
  $effect(() => {
    const failure = reader instanceof MobileReader ? reader.linkFailure : null;
    if (failure === 'unpaired' || failure === 'other_hub') {
      untrack(() => {
        notice = t('mobile.next.pair');
        void shell.state().then(open);
      });
    }
  });

  // Back in front: read again (no polling in the background, MUI-08).
  function onVisible(): void {
    if (document.visibilityState === 'visible' && reader) void reader.reload();
  }

  async function scan(): Promise<void> {
    notice = null;
    try {
      const text = await shell.scan();
      if (text !== null) link = text;
    } catch (error) {
      notice = said('mobile.err', error);
    }
  }

  async function pair(): Promise<void> {
    if (busy) return;
    busy = true;
    notice = null;
    try {
      const state = await shell.pair(link.trim(), name.trim() || 'Phone');
      link = '';
      notice = t('mobile.pair.done');
      await open(state);
    } catch (error) {
      notice = said('mobile.err', error);
    } finally {
      busy = false;
    }
  }

  async function pickVault(): Promise<void> {
    notice = null;
    try {
      await open(await shell.vaultPick());
    } catch (error) {
      notice = said('mobile.err', error);
    }
  }

  async function vaultOff(): Promise<void> {
    await open(await shell.vaultOff());
  }

  async function unpair(): Promise<void> {
    askUnpair = false;
    settings = false;
    try {
      await open(await shell.forget());
    } catch (error) {
      notice = said('mobile.err', error);
    }
  }

  /** The row of the open repository, with its CI state. */
  const openRow = $derived(reader?.fleet?.repos.find((r) => r.key === reader?.openKey) ?? null);
  const red = $derived(['failure', 'cancelled', 'no_runner'].includes(openRow?.ci_remote?.state ?? ''));
  const canRerun = $derived(phone !== null && phone.way !== null && WAY_ABILITIES[phone.way].trigger && !phone.vault);

  async function rerun(): Promise<void> {
    askRerun = false;
    const key = reader?.openKey;
    if (!key || busy) return;
    busy = true;
    notice = null;
    try {
      const r = await shell.trigger('/api/testing/ci-rerun', JSON.stringify({ key }));
      if (r.status === 202) {
        notice = t('mobile.rerun.started');
      } else {
        let code = 'failed';
        try {
          const doc = JSON.parse(r.body) as { error?: { code?: unknown } };
          if (typeof doc.error?.code === 'string') code = doc.error.code;
        } catch {
          // not the hub's words
        }
        notice = said('mobile.rerun', new ShellError(code));
      }
      void reader?.reload();
    } catch (error) {
      const kind = error instanceof ShellError ? error.kind : 'failed';
      // A lost answer may have started it: look before trying again (MOB-07).
      notice =
        kind === 'cancelled'
          ? t('mobile.rerun.confirm_required')
          : kind === 'answer_lost'
            ? t('mobile.next.look')
            : kind === 'unpaired' || kind === 'other_hub'
              ? t('mobile.next.pair')
              : said('mobile.rerun', error);
      if (kind === 'unpaired' || kind === 'other_hub') void shell.state().then(open);
    } finally {
      busy = false;
    }
  }

  async function togglePush(on: boolean): Promise<void> {
    notice = null;
    try {
      push = on ? await shell.pushOn() : await shell.pushOff();
    } catch (error) {
      notice = error instanceof ShellError && error.kind === 'no_distributor' ? t('mobile.push.no_distributor') : said('mobile.err', error);
      push = await shell.pushStatus().catch(() => null);
    }
  }

  function wayText(state: PhoneState): string {
    if (state.vault) return t('mobile.way.vault');
    if (state.way === 'tailnet') return t('mobile.way.tailnet', { hub: state.hub ?? '' });
    return t('mobile.way.tunnel', { hub: state.hub ?? '' });
  }

  function timeText(seconds: number): string {
    return new Date(seconds * 1000).toLocaleString(langOf(languages));
  }

  const status = $derived.by(() => {
    if (!phone || !reader) return '';
    if (reader instanceof MobileReader) {
      if (reader.linkFailure) return t(`mobile.next.${nextStep(reader.linkFailure, phone.way ?? 'tunnel')}` as Key);
      if (reader.keptAt !== null) return t('mobile.state.kept', { time: timeText(reader.keptAt) });
    }
    return reader.phase === 'ready' ? t('mobile.state.fresh') : t('mobile.state.reading');
  });
</script>

<svelte:document onvisibilitychange={onVisible} />

<main class="phone">
  {#if phone === null}
    <p class="state" role="status">{t('mobile.loading')}</p>
  {:else if !phone.paired && !phone.vault}
    <section class="pair" aria-labelledby="pair-h">
      <h1 id="pair-h">{t('mobile.pair.heading')}</h1>
      <p>{t('mobile.pair.intro')}</p>
      <button type="button" class="big" onclick={scan}>{t('mobile.pair.scan')}</button>
      <label>
        {t('mobile.pair.link')}
        <input type="url" autocomplete="off" spellcheck="false" bind:value={link} />
      </label>
      <label>
        {t('mobile.pair.name')}
        <input type="text" maxlength="40" autocomplete="off" bind:value={name} />
      </label>
      <button type="button" class="big primary" disabled={busy || link.trim() === ''} onclick={pair}>{t('mobile.pair.go')}</button>
      <h2>{t('mobile.pair.vault')}</h2>
      <button type="button" class="big" onclick={pickVault}>{t('mobile.pair.vault_go')}</button>
    </section>
  {:else}
    <header class="bar">
      {#if reader?.openKey}
        <button type="button" class="icon" onclick={() => reader?.close()} aria-label={t('mobile.back')}>←</button>
      {/if}
      <div class="where">
        <span class="way">{wayText(phone)}</span>
        <span class="age" role="status">{status}</span>
      </div>
      <button type="button" class="icon" onclick={() => reader?.reload()} aria-label={t('mobile.reload')}>⟳</button>
      <button type="button" class="icon" onclick={() => (settings = !settings)} aria-label={t('mobile.settings')} aria-expanded={settings}>⚙</button>
    </header>

    {#if settings}
      <section class="settings" aria-label={t('mobile.settings')}>
        {#if phone.paired && !phone.vault}
          <h2>{t('mobile.push.heading')}</h2>
          {#if phone.way !== 'tailnet'}
            <p>{t('mobile.push.only_tailnet')}</p>
          {:else if push && !push.distributor}
            <p>{t('mobile.push.no_distributor')}</p>
          {:else}
            <label class="switch">
              <input type="checkbox" checked={push?.subscribed ?? false} onchange={(e) => togglePush(e.currentTarget.checked)} />
              {t('mobile.push.label')}
            </label>
            {#if push && push.subscribed && !push.notices}<p>{t('mobile.push.no_notices')}</p>{/if}
          {/if}
          <button type="button" class="big danger" onclick={() => (askUnpair = true)}>{t('mobile.unpair')}</button>
        {/if}
        {#if phone.vault}
          <button type="button" class="big" onclick={vaultOff}>{t('mobile.vault.off')}</button>
        {/if}
      </section>
    {/if}

    {#if askUnpair}
      <div class="ask" role="alertdialog" aria-labelledby="unpair-q">
        <p id="unpair-q">{t('mobile.unpair.ask')}</p>
        <button type="button" class="big danger" onclick={unpair}>{t('mobile.unpair.yes')}</button>
        <button type="button" class="big" onclick={() => (askUnpair = false)}>{t('mobile.rerun.no')}</button>
      </div>
    {/if}

    {#if reader}
      <p class="partial">{t('mobile.partial')}</p>
      {#if reader.openKey}
        {#if red && canRerun}
          <div class="rerun">
            {#if !phone.confirm}
              <p>{t('mobile.rerun.no_confirm')}</p>
            {:else if askRerun}
              <div role="alertdialog" aria-labelledby="rerun-q">
                <p id="rerun-q">{t('mobile.rerun.ask', { repo: openRow?.name ?? '' })}</p>
                <button type="button" class="big primary" disabled={busy} onclick={rerun}>{t('mobile.rerun.yes')}</button>
                <button type="button" class="big" onclick={() => (askRerun = false)}>{t('mobile.rerun.no')}</button>
              </div>
            {:else}
              <button type="button" class="big" onclick={() => (askRerun = true)}>{t('mobile.rerun.button')}</button>
            {/if}
          </div>
        {/if}
        <Detail app={reader} {t} />
      {:else if reader.fleet}
        <Overview app={reader} {t} />
      {:else if reader.phase === 'failed'}
        <p class="state" role="status">{status}</p>
      {:else}
        <p class="state" role="status">{t('mobile.state.reading')}</p>
      {/if}
    {/if}
  {/if}

  {#if notice}
    <p class="notice" role="status">{notice}</p>
  {/if}
</main>

<style>
  .phone {
    max-inline-size: 48rem;
    margin-inline: auto;
    padding: 0 0.75rem 2rem;
  }
  .bar {
    position: sticky;
    inset-block-start: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding-block: 0.5rem;
    background: var(--bg);
    border-block-end: 1px solid var(--line);
    z-index: 1;
  }
  .where {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }
  .way {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .age {
    color: var(--text-muted);
    font-size: 0.9em;
  }
  .icon,
  .big {
    min-block-size: 48px;
    min-inline-size: 48px;
  }
  .big {
    display: block;
    inline-size: 100%;
    margin-block: 0.5rem;
  }
  .primary {
    background: var(--accent);
    color: var(--on-accent);
  }
  .danger {
    color: var(--danger);
  }
  .pair label,
  .switch {
    display: block;
    margin-block: 0.75rem;
  }
  .pair input[type='url'],
  .pair input[type='text'] {
    display: block;
    inline-size: 100%;
    min-block-size: 48px;
    margin-block-start: 0.25rem;
  }
  .switch input {
    min-inline-size: 24px;
    min-block-size: 24px;
    margin-inline-end: 0.5rem;
  }
  .settings,
  .ask,
  .rerun {
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.75rem;
    margin-block: 0.75rem;
  }
  .partial,
  .state {
    color: var(--text-muted);
  }
  .notice {
    position: sticky;
    inset-block-end: 0.5rem;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.75rem;
  }
</style>
