<script lang="ts">
  // The log of the runs of one repository: what can be said about it, the runs per day, and the list of runs. The numbers
  // are the engine's (`history` of the repository's document); the page only shows them, on the hub and from a folder.
  // The list can be searched and cut to a period, and what it shows can be saved as CSV or JSON: a download of what the
  // page already holds, nothing is written anywhere else (docs/SECURITY.md).
  import type { History, HistoryRun } from '../lib/contract';
  import type { Key } from '../lib/i18n';
  import { filterRuns, greenPercent, RUN_PERIODS, runsCsv, runsJson, runTone, shown, type RunPeriod, type Tone } from '../lib/model';
  import ToneView from './Tone.svelte';

  let {
    history,
    repo,
    t,
  }: { history: History; repo: string; t: (key: Key, params?: Record<string, string | number>) => string } = $props();

  let onlyRed = $state(false);
  let query = $state('');
  let period = $state<RunPeriod>(null);

  const stats = $derived(history.stats);
  const rate = $derived(greenPercent(stats.green, stats.red));
  const before = $derived(greenPercent(stats.previous.green, stats.previous.red));
  const most = $derived(Math.max(1, ...stats.days.map((d) => d.n)));
  const runs = $derived(filterRuns(history.runs, { onlyRed, query, days: period }));

  /** The file name of a download: the repository's name with what is not safe in a file name replaced. */
  function fileName(ext: string): string {
    return `${repo.replace(/[^A-Za-z0-9._-]+/g, '_') || 'runs'}-runs.${ext}`;
  }

  /** Hand the shown runs to the browser as a file to save. */
  function save(kind: 'csv' | 'json') {
    const text = kind === 'csv' ? runsCsv(runs) : runsJson(repo, runs);
    const blob = new Blob([text], { type: kind === 'csv' ? 'text/csv;charset=utf-8' : 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = fileName(kind);
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function when(iso: string): string {
    const date = new Date(iso);
    return Number.isNaN(date.getTime()) ? iso : date.toLocaleString();
  }

  function seconds(ms: number | null): string {
    return ms === null ? '–' : t('history.seconds', { n: (ms / 1000).toFixed(1) });
  }

  function toneWord(tone: Tone): string {
    return t(`tone.${tone}` as Key);
  }

  function redOf(run: HistoryRun): number {
    return run.red.length + run.red_more;
  }
</script>

<section aria-labelledby="history-heading">
  <h2 id="history-heading">{t('history.heading')}</h2>
  {#if history.runs.length === 0}
    <p class="muted">{t('history.none')}</p>
  {:else}
    <dl class="tiles">
      <div>
        <dt>{t('history.rate', { days: stats.period_days })}</dt>
        <dd>{rate === null ? '–' : `${rate} %`}</dd>
        <dd class="small">{t('history.whole_runs', { n: stats.n })}</dd>
      </div>
      <div>
        <dt>{t('history.before', { days: stats.period_days })}</dt>
        <dd>{before === null ? '–' : `${before} %`}</dd>
        <dd class="small">{t('history.whole_runs', { n: stats.previous.n })}</dd>
      </div>
      <div>
        <dt>{t('history.median')}</dt>
        <dd>{seconds(stats.ms_median)}</dd>
        <dd class="small">{t('history.p90', { value: seconds(stats.ms_p90) })}</dd>
      </div>
      {#if stats.red_since}
        <div>
          <dt>{t('history.red_since')}</dt>
          <dd class="when">{when(stats.red_since)}</dd>
        </div>
      {/if}
    </dl>

    {#if stats.days.length > 0}
      <h3>{t('history.per_day')}</h3>
      <div class="legend" aria-hidden="true">
        <span><i class="key green"></i>{t('tone.green')}</span>
        <span><i class="key red"></i>{t('tone.red')}</span>
        <span><i class="key other"></i>{t('history.other')}</span>
      </div>
      <ol class="bars">
        {#each stats.days as day (day.day)}
          {@const other = day.n - day.green - day.red}
          {@const words = t('history.day', { day: day.day, n: day.n, green: day.green, red: day.red })}
          <li title={words} aria-label={words}>
            <span class="bar" style:height={`${(day.n / most) * 100}%`}>
              {#if other > 0}<span class="part other" style:flex-grow={other}></span>{/if}
              {#if day.red > 0}<span class="part red" style:flex-grow={day.red}></span>{/if}
              {#if day.green > 0}<span class="part green" style:flex-grow={day.green}></span>{/if}
            </span>
          </li>
        {/each}
      </ol>
    {/if}

    {#if stats.flaky.length > 0}
      <h3>{t('history.flaky')}</h3>
      <ul class="ids">
        {#each stats.flaky as id, i (i)}
          <li class="mono">{shown(id)}</li>
        {/each}
      </ul>
    {/if}
    {#if stats.red_top.length > 0}
      <h3>{t('history.red_top')}</h3>
      <ul class="ids">
        {#each stats.red_top as item, i (i)}
          <li><span class="count">{item.n} ×</span> <span class="mono">{shown(item.id)}</span></li>
        {/each}
      </ul>
    {/if}

    <div class="runs-head">
      <h3>{t('history.runs')}</h3>
      <label><input type="checkbox" bind:checked={onlyRed} /> {t('history.only_red')}</label>
      <input class="search" type="search" placeholder={t('history.search')} aria-label={t('history.search')} bind:value={query} />
      <label>
        {t('history.period')}
        <select bind:value={period}>
          {#each RUN_PERIODS as p (p ?? 'all')}
            <option value={p}>{p === null ? t('history.period.all') : t('history.period.days', { n: p })}</option>
          {/each}
        </select>
      </label>
      <span class="muted">{t('history.shown', { shown: runs.length, total: history.runs.length })}</span>
      <span class="save">
        <button type="button" onclick={() => save('csv')} disabled={runs.length === 0}>{t('history.save_csv')}</button>
        <button type="button" onclick={() => save('json')} disabled={runs.length === 0}>{t('history.save_json')}</button>
      </span>
    </div>
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th scope="col">{t('history.col.time')}</th>
            <th scope="col">{t('history.col.what')}</th>
            <th scope="col">{t('history.col.result')}</th>
            <th scope="col" class="number">{t('history.col.duration')}</th>
            <th scope="col">{t('history.col.commit')}</th>
          </tr>
        </thead>
        <tbody>
          <!-- By position: nothing in the log says that the id of a run is unique. -->
          {#each runs as run, i (i)}
            <tr>
              <td>{when(run.ts)}</td>
              <td>
                {t(`history.kind.${run.kind}` as Key)}{#if run.profile} ({shown(run.profile)}){/if}{#if run.scope === 'partial'}, {t('history.partial')}{/if}
              </td>
              <td>
                <ToneView tone={runTone(run.verdict)} label={toneWord(runTone(run.verdict))} />
                {#if redOf(run) > 0}<span class="muted">{t('local.failed', { n: redOf(run) })}</span>{/if}
              </td>
              <td class="number">{seconds(run.ms)}</td>
              <td class="mono">{run.sha ? run.sha.slice(0, 7) : '–'}{#if run.dirty} {t('history.dirty')}{/if}</td>
            </tr>
          {:else}
            <tr><td colspan="5" class="muted">{onlyRed && query === '' && period === null ? t('history.none_red') : t('history.none_match')}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if history.runs_omitted > 0}
      <p class="muted">{t('history.omitted', { n: history.runs_omitted })}</p>
    {/if}
  {/if}
</section>

<style>
  h2 {
    font-size: 1rem;
    margin: 1rem 0 0.5rem;
  }
  h3 {
    margin: 1rem 0 0.4rem;
  }
  .muted,
  .small {
    color: var(--muted);
  }
  .small {
    font-size: 0.85em;
    font-weight: 400;
  }
  .mono {
    font-family: var(--mono);
    overflow-wrap: anywhere;
  }
  .tiles {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    margin: 0;
  }
  .tiles > div {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.5rem 0.75rem;
    min-width: 9rem;
    background: var(--surface);
  }
  dt {
    color: var(--muted);
    font-size: 0.85em;
  }
  dd {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
  }
  dd.small {
    font-size: 0.85em;
    font-weight: 400;
  }
  dd.when {
    font-size: 1rem;
  }
  .legend {
    display: flex;
    gap: 0.75rem;
    color: var(--muted);
    font-size: 0.85em;
    margin-bottom: 0.25rem;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .key {
    width: 0.65rem;
    height: 0.65rem;
    border-radius: 2px;
    display: inline-block;
  }
  .green {
    background: var(--ok);
  }
  .red {
    background: var(--bad);
  }
  .other {
    background: var(--muted);
  }
  .bars {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    list-style: none;
    margin: 0;
    padding: 0;
    height: 4.5rem;
    border-bottom: 1px solid var(--line);
    max-width: 40rem;
  }
  .bars li {
    flex: 1 1 0;
    max-width: 1.25rem;
    height: 100%;
    display: flex;
    align-items: flex-end;
  }
  .bars li:hover {
    background: var(--surface-2);
  }
  .bar {
    width: 100%;
    min-height: 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .part {
    flex-basis: 0;
    min-height: 2px;
  }
  .part:first-child {
    border-radius: 4px 4px 0 0;
  }
  .ids {
    margin: 0;
    padding-left: 1.1rem;
  }
  .count {
    color: var(--muted);
  }
  .runs-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.5rem 1rem;
  }
  .runs-head .search {
    min-width: 12rem;
  }
  .runs-head .save {
    display: flex;
    gap: 0.5rem;
    margin-left: auto;
  }
  /* the buttons of the page (as the sheets of a repository): its colours, in both schemes */
  .save button {
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0.2rem 0.6rem;
    font: inherit;
    cursor: pointer;
  }
  .save button:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .save button:disabled {
    color: var(--muted);
    cursor: default;
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th,
  td {
    text-align: left;
    padding: 0.3rem 0.75rem 0.3rem 0;
    border-bottom: 1px solid var(--line);
    white-space: nowrap;
    font-weight: 400;
  }
  th {
    color: var(--muted);
  }
  .number {
    text-align: right;
  }
</style>
