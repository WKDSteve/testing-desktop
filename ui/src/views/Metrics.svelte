<script lang="ts">
  // What the heavy runs of one repository cost on the machine that exported it: runs, machine minutes, runs per push,
  // the cache, the queue, per kind of run and per day. The numbers are the engine's (`metrics` of the repository's
  // document, `testing stats`; for the fleet the same part of the fleet's document, all repositories together); the
  // page only adds them up for the tiles.
  import type { Metrics } from '../lib/contract';
  import type { Key } from '../lib/i18n';
  import { metricsSummary } from '../lib/model';

  let {
    metrics,
    t,
    heading = 'metrics.heading',
    id = 'metrics-heading',
  }: {
    metrics: Metrics;
    t: (key: Key, params?: Record<string, string | number>) => string;
    heading?: Key;
    id?: string;
  } = $props();

  const sum = $derived(metricsSummary(metrics));
  const most = $derived(Math.max(1, ...metrics.days.map((d) => d.minutes)));

  function day(iso: string): string {
    const date = new Date(iso);
    return Number.isNaN(date.getTime()) ? iso : date.toLocaleDateString();
  }

  function number(n: number): string {
    return n.toLocaleString(undefined, { maximumFractionDigits: 1 });
  }

  function verdicts(list: Metrics['kinds'][number]['verdicts']): string {
    return list.map((v) => `${v.verdict} ${v.n}`).join(', ');
  }
</script>

<section aria-labelledby={id}>
  <h2 {id}>{t(heading)}</h2>
  <p class="muted">{t('metrics.period', { first: day(metrics.first), last: day(metrics.last) })}</p>
  <dl class="tiles">
    <div>
      <dt>{t('metrics.runs')}</dt>
      <dd>{sum.runs}</dd>
      {#if metrics.backfilled > 0}<dd class="small">{t('metrics.backfilled', { n: metrics.backfilled })}</dd>{/if}
    </div>
    <div>
      <dt>{t('metrics.minutes')}</dt>
      <dd>{number(sum.minutes)}</dd>
    </div>
    <div>
      <dt>{t('metrics.per_push')}</dt>
      <dd>{sum.perPush === null ? '–' : number(sum.perPush)}</dd>
      <dd class="small">{sum.pushes === null ? t('metrics.pushes_unknown') : t('metrics.pushes', { n: sum.pushes })}</dd>
    </div>
    <div>
      <dt>{t('metrics.cache')}</dt>
      <dd>{sum.hitRate === null ? '–' : `${number(sum.hitRate)} %`}</dd>
      <dd class="small">{t('metrics.cache_files', { cached: metrics.cache.cached, selected: metrics.cache.selected })}</dd>
    </div>
    <div>
      <dt>{t('metrics.wait')}</dt>
      <dd>{number(sum.waitMinutes)}</dd>
      <dd class="small">{t('metrics.waited', { n: sum.waited })}</dd>
    </div>
    {#if sum.quickShare !== null}
      <div>
        <dt>{t('metrics.quick')}</dt>
        <dd>{sum.quickShare} %</dd>
      </div>
    {/if}
  </dl>

  {#if metrics.days.length > 0}
    <h3>{t('metrics.per_day')}</h3>
    <ol class="bars">
      {#each metrics.days as d (d.day)}
        {@const words = t('metrics.day', { day: d.day, n: d.runs, minutes: number(d.minutes) })}
        <li title={words} aria-label={words}>
          <span class="bar" style:height={`${(d.minutes / most) * 100}%`}></span>
        </li>
      {/each}
    </ol>
  {/if}

  <h3>{t('metrics.per_kind')}</h3>
  <div class="scroll">
    <table>
      <thead>
        <tr>
          <th scope="col">{t('metrics.col.kind')}</th>
          <th scope="col" class="number">{t('metrics.col.runs')}</th>
          <th scope="col" class="number">{t('metrics.col.minutes')}</th>
          <th scope="col" class="number">{t('metrics.col.median')}</th>
          <th scope="col" class="number">{t('metrics.col.wait')}</th>
          <th scope="col">{t('metrics.col.verdicts')}</th>
        </tr>
      </thead>
      <tbody>
        {#each metrics.kinds as k (k.kind)}
          <tr>
            <td>{t(`metrics.kind.${k.kind}` as Key)}</td>
            <td class="number">{k.runs}</td>
            <td class="number">{number(k.minutes)}</td>
            <td class="number">{t('history.seconds', { n: number(k.median_s) })}</td>
            <td class="number">{number(k.wait_minutes)}</td>
            <td class="mono">{verdicts(k.verdicts)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
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
    border-radius: 4px 4px 0 0;
    background: var(--accent);
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
