<script lang="ts">
  // What a stepped run of a test sheet looks like, wherever it comes from: the sizes, the steps of the size that runs
  // with their state, the tile of the chosen step (what it did, its reason, the child's screen as text or the picture
  // of the target), and how the run ended. The page on the hub follows a run with it (Step.svelte), the web variant
  // plays a recording back (web/Replay.svelte). It starts nothing: what a click does comes in.
  import type { Snippet } from 'svelte';
  import type { Key } from '../lib/i18n';
  import { shown } from '../lib/model';
  import { marksOf, notesOf, oneLine, screenRows, statusOf, tileStep, type Mark, type SheetStepDoc, type StepState, type StepStatus } from '../lib/step';
  import Picture, { type LoadPicture } from './Picture.svelte';

  let {
    run,
    chosen,
    t,
    onChoose,
    loadPicture,
    onToggleBreak,
    tileExtra,
  }: {
    run: StepState;
    /** The step chosen in the list; `null`: the last one that did something. */
    chosen: number | null;
    t: (key: Key, params?: Record<string, string | number>) => string;
    onChoose: (n: number) => void;
    loadPicture: LoadPicture;
    /** Set or remove a breakpoint; without it the list has no breakpoint dots (a recording). */
    onToggleBreak?: (n: number) => void;
    /** More of the tile of a step, under what it did. */
    tileExtra?: Snippet<[SheetStepDoc, Mark | undefined]>;
  } = $props();

  const doc = $derived(run.doc);
  const marks = $derived(marksOf(run));
  const tile = $derived(tileStep(run, chosen));
  const tileMark = $derived(tile !== null ? marks[tile] : undefined);
  const tileStepDoc = $derived(doc?.steps.find((s) => s.n === tile) ?? null);

  const ICON: Record<StepStatus, string> = { pass: '✓', fail: '✗', running: '▶', next: '▷', pending: ' ', skipped: '–' };

  function stateWord(status: StepStatus): string {
    return t(`step.state.${status}` as Key);
  }
</script>

{#if doc}
  {#if run.sizes.length > 1 || doc.sizes.length > 1}
    <p class="muted sizes">
      {t('step.sizes')}:
      {#each doc.sizes as size (size)}
        {@const found = run.sizes.find((s) => s.size === size)}
        <span class:current={size === run.size} class="tag">{size}{found ? ` · ${t(`step.size.${found.state}` as Key)}` : ''}</span>
      {/each}
    </p>
  {/if}

  <div class="split">
    <ol class="steps" aria-label={t('step.steps')}>
      {#each doc.steps as step (step.n)}
        {@const status = statusOf(run, step.n)}
        {@const mark = marks[step.n]}
        <li class={status} class:chosen={tile === step.n} aria-current={step.n === run.at ? 'step' : undefined}>
          {#if onToggleBreak}
            <button
              type="button"
              class="dot"
              aria-pressed={run.breaks.includes(step.n)}
              aria-label={t('step.break_on', { n: step.n })}
              onclick={() => onToggleBreak(step.n)}>{run.breaks.includes(step.n) ? '●' : '○'}</button>
          {/if}
          <button type="button" class="row" onclick={() => onChoose(step.n)}>
            <span class="icon" aria-hidden="true">{ICON[status]}</span>
            <span class="sr">{stateWord(status)}</span>
            <span class="n">{step.n}</span>
            <span class="action mono">{shown(step.action)}</span>
            <span class="text">{oneLine(step.note ?? step.value, 70)}</span>
            {#if mark?.ms !== null && mark?.ms !== undefined}<span class="muted">{t('step.ms', { ms: mark.ms })}</span>{/if}
          </button>
        </li>
      {/each}
    </ol>

    <aside class="tile" aria-live="polite">
      {#if tileStepDoc}
        <h2>{t('step.tile', { n: tileStepDoc.n, action: shown(tileStepDoc.action) ?? '' })}</h2>
        <p class="mono value">{oneLine(tileStepDoc.value, 300)}</p>
        {#if tileStepDoc.note}<p class="muted">{oneLine(tileStepDoc.note, 300)}</p>{/if}
        {#each notesOf(run, tileStepDoc.n) as note, i (i)}
          <p>{t('step.note', { text: oneLine(note, 400) })}</p>
        {/each}
        {#if tileMark}
          <p>
            <span class={tileMark.status}>{stateWord(tileMark.status)}</span>
            {#if tileMark.ms !== null}<span class="muted"> · {t('step.ms', { ms: tileMark.ms })}</span>{/if}
          </p>
          {#if tileMark.reason}
            <p class="label">{t('step.reason')}</p>
            <pre>{shown(tileMark.reason)}</pre>
          {/if}
          {#if tileMark.failure}
            <p class="label">{t('step.failure')}</p>
            <pre>{shown(tileMark.failure)}</pre>
          {/if}
          {#if tileMark.screen}
            <p class="label">{t('step.screen')}</p>
            <pre class="screen">{screenRows(tileMark.screen).join('\n')}</pre>
          {/if}
          {#if tileMark.image && run.size}
            <Picture load={loadPicture} {t} size={run.size} n={tileStepDoc.n} kind="image" label={t('step.picture')} />
          {/if}
          {#if tileMark.shot && run.size}
            <Picture load={loadPicture} {t} size={run.size} n={tileStepDoc.n} kind="shot" label={t('step.shot')} />
          {/if}
        {:else}
          <p class="muted">{t('step.not_yet')}</p>
        {/if}
        {#if tileExtra}{@render tileExtra(tileStepDoc, tileMark)}{/if}
      {:else}
        <p class="muted">{t('step.no_tile')}</p>
      {/if}
    </aside>
  </div>

  {#if run.finished}
    <p class="muted">{run.finished.verdict ? t('step.verdict') : t('step.no_verdict')}</p>
    {#if run.finished.failures.length > 0}
      <h2>{t('step.failures')}</h2>
      <ul class="failures">
        {#each run.finished.failures as f, i (i)}
          <li class="mono">{oneLine(f, 400)}</li>
        {/each}
      </ul>
    {/if}
  {/if}
  {#if run.task}
    <p class="muted">{t('step.task', { id: shown(run.task.id) ?? '', outcome: shown(run.task.outcome) ?? '' })}{run.task.message ? `: ${oneLine(run.task.message, 200)}` : ''}</p>
  {/if}
  {#if doc.judge.length > 0}
    <h2>{t('step.judge')}</h2>
    <ul>
      {#each doc.judge as j, i (i)}<li>{shown(j)}</li>{/each}
    </ul>
  {/if}
  {#if doc.manual.length > 0}
    <h2>{t('step.manual')}</h2>
    <ul>
      {#each doc.manual as m, i (i)}<li>{shown(m)}</li>{/each}
    </ul>
  {/if}
{/if}

<style>
  h2 {
    font-size: 1rem;
    margin: 1rem 0 0.5rem;
  }
  .muted {
    color: var(--muted);
  }
  .fail {
    color: var(--bad);
  }
  .pass {
    color: var(--ok);
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
    max-height: 30rem;
    overflow: auto;
  }
  .screen {
    white-space: pre;
  }
  .split {
    display: grid;
    grid-template-columns: minmax(18rem, 2fr) minmax(18rem, 3fr);
    gap: 1rem;
    align-items: start;
  }
  @media (max-width: 50rem) {
    .split {
      grid-template-columns: 1fr;
    }
  }
  .steps {
    list-style: none;
    padding: 0;
    margin: 0;
  }
  .steps > li {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    border-left: 3px solid transparent;
  }
  .steps > li.chosen {
    border-left-color: var(--accent);
    background: var(--surface-2);
  }
  .steps > li.running .icon {
    color: var(--accent);
  }
  .steps > li.pass .icon {
    color: var(--ok);
  }
  .steps > li.fail .icon {
    color: var(--bad);
  }
  .dot {
    background: none;
    border: none;
    padding: 0 0.25rem;
    cursor: pointer;
    color: var(--bad);
    font: inherit;
  }
  .dot[aria-pressed='false'] {
    color: var(--muted);
  }
  .row {
    flex: 1;
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
    background: none;
    border: none;
    padding: 0.2rem 0.25rem;
    text-align: left;
    font: inherit;
    color: inherit;
    cursor: pointer;
    min-width: 0;
  }
  .icon {
    width: 1em;
    display: inline-block;
  }
  .n {
    min-width: 1.5em;
    text-align: right;
    color: var(--muted);
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .tile {
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.5rem 0.75rem;
    min-width: 0;
  }
  .tile h2 {
    margin-top: 0;
  }
  .value {
    overflow-wrap: anywhere;
  }
  .label {
    margin: 0.4rem 0 0;
    color: var(--muted);
    font-size: 0.85em;
  }
  .tag {
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    padding: 0 0.4rem;
    font-size: 0.85em;
    margin-right: 0.25rem;
  }
  .tag.current {
    border-color: var(--accent);
  }
  .failures {
    padding-left: 1.25rem;
  }
</style>
