<script lang="ts">
  // One repository: its rows of the overview again, the link to its CI run, and its red cases with their output.
  import type { Reader } from '../lib/reader.svelte';
  import type { Key } from '../lib/i18n';
  import { caseTitle, redCases, shown, viewOf, type Tone } from '../lib/model';
  import HistoryView from './History.svelte';
  import ToneView from './Tone.svelte';

  let { app, t }: { app: Reader; t: (key: Key, params?: Record<string, string | number>) => string } = $props();

  const doc = $derived(app.repo);
  const view = $derived(doc ? viewOf(doc.repo) : null);
  const cases = $derived(doc ? redCases(doc.cases) : []);
  const name = $derived(view?.name ?? app.fleet?.repos.find((r) => r.key === app.openKey)?.name ?? '');

  function toneWord(tone: Tone): string {
    return t(`tone.${tone}` as Key);
  }
</script>

<section aria-labelledby="detail-heading">
  <button type="button" class="back" onclick={() => app.close()}>← {t('detail.back')}</button>
  <h1 id="detail-heading">{shown(name)}</h1>

  {#if app.repoFailure}
    <p class="bad" role="status">{t('detail.failed', { message: shown(app.repoFailure) ?? '' })}</p>
  {:else if !doc || !view}
    <p class="muted" role="status">{t('detail.loading', { name: shown(name) ?? '' })}</p>
  {:else}
    <p class="summary">
      {#if view.local}
        <ToneView tone={view.local.tone} label={view.local.failed > 0 ? t('local.failed', { n: view.local.failed }) : toneWord(view.local.tone)} />
      {:else}
        <span class="muted">{t('local.none')}</span>
      {/if}
      {#if view.run}
        <ToneView tone={view.run.tone} label={`${t('ci.run')}: ${toneWord(view.run.tone)}`} link={view.run.link} />
      {/if}
    </p>

    {#if doc.notes.length > 0}
      <h2>{t('detail.notes')}</h2>
      <ul class="notes">
        {#each doc.notes as note, i (i)}
          <li>{shown(note)}</li>
        {/each}
      </ul>
    {/if}

    <h2>{t('detail.red_cases')}</h2>
    {#if cases.length === 0}
      <p class="muted">{t('detail.no_red')}</p>
    {:else}
      <ol class="cases">
        <!-- By position: two cases of one name in one file have the same id, and a key twice stops the page. -->
        {#each cases as c, i (i)}
          <li>
            <div class="case-head">
              <ToneView tone="red" label={c.status} />
              <strong>{caseTitle(c)}</strong>
              {#if c.flaky}<span class="tag">{t('detail.flaky')}</span>{/if}
              <span class="muted mono">{shown(c.file)}{#if c.line !== null}, {t('detail.line', { n: c.line })}{/if}</span>
            </div>
            {#if c.message}
              <p class="label">{t('detail.message')}</p>
              <pre>{shown(c.message)}</pre>
            {/if}
            {#if c.expected !== null || c.actual !== null}
              <div class="pair">
                <div>
                  <p class="label">{t('detail.expected')}</p>
                  <pre>{shown(c.expected) ?? ''}</pre>
                </div>
                <div>
                  <p class="label">{t('detail.actual')}</p>
                  <pre>{shown(c.actual) ?? ''}</pre>
                </div>
              </div>
            {/if}
            {#if c.fails.length > 0}
              <ul class="fails">
                {#each c.fails as f, i (i)}
                  <li class="mono">{#if f.line !== null}{t('detail.line', { n: f.line })}: {/if}{shown(f.msg) ?? ''}</li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ol>
      {#if doc.cases_omitted > 0}
        <p class="muted">{t('detail.omitted', { n: doc.cases_omitted })}</p>
      {/if}
    {/if}

    {#if doc.history}
      <HistoryView history={doc.history} {t} />
    {/if}

    {#if app.hasStep}
      <h2>{t('step.sheets')}</h2>
      {#if app.sheetsFailure}
        <p class="bad" role="status">{t('step.sheets.failed', { message: shown(app.sheetsFailure) ?? '' })}</p>
      {:else if app.sheets === null}
        <p class="muted" role="status">{t('step.sheets.loading')}</p>
      {:else if app.sheets.length === 0}
        <p class="muted">{t('step.sheets.none')}</p>
      {:else}
        <ul class="sheets">
          {#each app.sheets as name (name)}
            <li>
              <span class="mono">{shown(name)}</span>
              <button type="button" onclick={() => void app.stepSheet(name)}>{t('step.run', { name: shown(name) ?? '' })}</button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    {#if app.hasFindings}
    <h2>{t('findings.heading')}</h2>
    {#if app.findingsFailure}
      <p class="bad" role="status">{t('findings.failed', { message: shown(app.findingsFailure) ?? '' })}</p>
    {:else if app.findings === null}
      <p class="muted" role="status">{t('findings.loading')}</p>
    {:else if app.findings.length === 0}
      <p class="muted">{t('findings.none')}</p>
    {:else}
      <ul class="findings">
        <!-- By position: the fingerprint is the engine's, and a key twice stops the page. -->
        {#each app.findings as f, i (i)}
          <li>
            <div class="case-head">
              <span class="tag">{shown(f.kind) ?? ''}</span>
              <strong>{shown(f.title) ?? ''}</strong>
              {#if f.where}
                <span class="muted mono">{shown(f.where.file)}{#if f.where.line !== null}, {t('detail.line', { n: f.where.line })}{/if}</span>
              {/if}
            </div>
            {#if f.excerpt}
              <pre>{shown(f.excerpt)}</pre>
            {/if}
            {#if f.repro}
              <p class="label">{t('findings.repro')}</p>
              <pre>{shown(f.repro)}</pre>
            {/if}
            {#if f.rule}
              <p class="label">{t('findings.rule')}: <span class="mono">{shown(f.rule)}</span></p>
            {/if}
            {#if f.run}
              <p class="label">
                {#if f.run.link}
                  <a href={f.run.link} target="_blank" rel="noopener noreferrer">{t('findings.run')}: {shown(f.run.source)}{#if f.run.os} ({shown(f.run.os)}){/if}</a>
                {:else}
                  {t('findings.run')}: {shown(f.run.source)}{#if f.run.os} ({shown(f.run.os)}){/if}
                {/if}
              </p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {/if}
  {/if}
</section>

<style>
  .back {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
  }
  h1 {
    font-size: 1.25rem;
    margin: 0.5rem 0;
  }
  h2 {
    font-size: 1rem;
    margin: 1rem 0 0.5rem;
  }
  .summary {
    display: flex;
    gap: 1.5rem;
    flex-wrap: wrap;
  }
  .muted {
    color: var(--muted);
  }
  .bad {
    color: var(--bad);
  }
  .mono,
  pre {
    font-family: var(--mono);
  }
  pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0.5rem;
    margin: 0.25rem 0;
    max-height: 20rem;
    overflow: auto;
  }
  .cases > li,
  .findings > li {
    margin-bottom: 1rem;
    border-bottom: 1px solid var(--line);
    padding-bottom: 0.75rem;
  }
  .case-head {
    display: flex;
    gap: 0.75rem;
    flex-wrap: wrap;
    align-items: baseline;
  }
  .tag {
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    padding: 0 0.4rem;
    font-size: 0.85em;
  }
  .label {
    margin: 0.4rem 0 0;
    color: var(--muted);
    font-size: 0.85em;
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.75rem;
  }
  .sheets {
    list-style: none;
    padding: 0;
    margin: 0;
  }
  .sheets > li {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    padding: 0.25rem 0;
  }
</style>
