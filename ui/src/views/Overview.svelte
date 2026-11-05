<script lang="ts">
  // The fleet: one row per repository, the last local run and the CI on each system; red rows first.
  import type { Reader } from '../lib/reader.svelte';
  import type { Key } from '../lib/i18n';
  import { notesOf, rowsOf, shown, SYSTEMS, type Tone } from '../lib/model';
  import ToneView from './Tone.svelte';

  let { app, t }: { app: Reader; t: (key: Key, params?: Record<string, string | number>) => string } = $props();

  const fleet = $derived(app.fleet);
  const rows = $derived(fleet ? rowsOf(fleet, { onlyRed: app.onlyRed, query: app.query }) : []);
  const notes = $derived(fleet ? notesOf(fleet) : null);

  function toneWord(tone: Tone): string {
    return t(`tone.${tone}` as Key);
  }
</script>

<section aria-labelledby="fleet-heading">
  <div class="bar">
    <h1 id="fleet-heading">{t('fleet.heading')}</h1>
    {#if fleet}
      <span class="muted">{t('fleet.count', { shown: rows.length, total: fleet.repos.length })}</span>
    {/if}
    <label class="filter"><input type="checkbox" bind:checked={app.onlyRed} /> {t('fleet.only_red')}</label>
    <input class="search" type="search" placeholder={t('fleet.search')} aria-label={t('fleet.search')} bind:value={app.query} />
  </div>

  {#if notes?.classesProblem}
    <p class="note bad" role="status">{t('fleet.classes_problem', { problem: shown(notes.classesProblem) ?? '' })}</p>
  {/if}
  {#if notes && notes.excluded > 0}
    <p class="note muted">{t('fleet.excluded', { n: notes.excluded })}</p>
  {/if}
  {#if notes?.remoteError}
    <p class="note warn">{t('fleet.remote_error', { error: shown(notes.remoteError) ?? '' })}</p>
  {/if}
  {#if notes && !notes.localRead}
    <p class="note muted">{t('fleet.local_not_read')}</p>
  {/if}
  {#if app.failure}
    <p class="note warn" role="status">{t('phase.failed', { message: shown(app.failure) ?? '' })}</p>
  {/if}

  {#if rows.length === 0}
    <p class="empty">{app.onlyRed ? t('fleet.empty_red') : t('fleet.empty')}</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th scope="col">{t('col.repo')}</th>
          <th scope="col">{t('col.local')}</th>
          {#each SYSTEMS as system (system)}
            <th scope="col">{t('col.ci')} {system}</th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each rows as row (row.key)}
          <tr class:red={row.red}>
            <th scope="row">
              <button type="button" class="link" onclick={() => app.open(row.key)}>{shown(row.name)}</button>
            </th>
            <td>
              {#if row.local}
                <ToneView tone={row.local.tone} label={row.local.failed > 0 ? t('local.failed', { n: row.local.failed }) : toneWord(row.local.tone)} />
              {:else}
                <span class="muted">{t('local.none')}</span>
              {/if}
            </td>
            {#each SYSTEMS as system (system)}
              <td>
                {#if row.run === null}
                  <span class="muted">{t('ci.not_asked')}</span>
                {:else}
                  <ToneView tone={row.ci[system].tone} label={toneWord(row.ci[system].tone)} link={row.ci[system].link} />
                {/if}
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 1rem;
    margin-bottom: 0.75rem;
  }
  h1 {
    font-size: 1.25rem;
    margin: 0;
  }
  .search {
    margin-left: auto;
    min-width: 14rem;
  }
  .muted {
    color: var(--muted);
  }
  .note {
    margin: 0.25rem 0;
  }
  .bad {
    color: var(--bad);
  }
  .warn {
    color: var(--warn);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    text-align: left;
    padding: 0.35rem 0.6rem;
    border-bottom: 1px solid var(--line);
  }
  thead th {
    color: var(--muted);
    font-weight: 600;
  }
  tr.red th[scope='row'] {
    border-left: 3px solid var(--bad);
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
    text-decoration: underline;
  }
  .empty {
    color: var(--muted);
  }
</style>
