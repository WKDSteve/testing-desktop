<script lang="ts">
  // A test sheet step by step, as the panel of testing.nvim shows it: the steps of the size that runs with their
  // run and breakpoints, the keys of a debugger (next, continue, until here, abort), and the tile of the chosen
  // step: what it did, its reason, the child's screen as text or the picture of the target. A question of the sheet
  // is a dialog with yes, no and a note. The page never runs a step itself: every key is a command to the hub.
  import type { App } from '../lib/app.svelte';
  import type { Key } from '../lib/i18n';
  import { shown } from '../lib/model';
  import { marksOf, notesOf, oneLine, screenRows, statusLine, statusOf, tileStep, type StepStatus } from '../lib/step';
  import Picture from './Picture.svelte';

  let { app, t }: { app: App; t: (key: Key, params?: Record<string, string | number>) => string } = $props();

  const run = $derived(app.step);
  const doc = $derived(run.doc);
  const marks = $derived(marksOf(run));
  const tile = $derived(tileStep(run, app.stepChosen));
  const tileMark = $derived(tile !== null ? marks[tile] : undefined);
  const tileStepDoc = $derived(doc?.steps.find((s) => s.n === tile) ?? null);
  const canUntil = $derived(app.stepChosen !== null && app.stepChosen > (run.at ?? 0) && run.paused !== null && !run.question);
  const paused = $derived(run.running && run.paused !== null && !run.question);

  const ICON: Record<StepStatus, string> = { pass: '✓', fail: '✗', running: '▶', next: '▷', pending: ' ', skipped: '–' };

  let answer = $state('');
  let note = $state('');
  let noteField: HTMLInputElement | undefined = $state();

  async function onNote(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (await app.stepNote(note)) note = '';
  }

  function statusText(): string {
    const line = statusLine(run);
    switch (line.kind) {
      case 'finished':
        return t('step.status.finished', { status: line.status, ms: line.ms }) + (line.skipped ? ` (${oneLine(line.skipped, 100)})` : '');
      case 'exit':
        return t('step.status.exit', { code: line.code ?? '?' });
      case 'asks':
        return t('step.status.asks', { text: oneLine(line.text, 110) });
      case 'paused_failed':
        return t('step.status.paused_failed', { n: line.n, size: line.size });
      case 'paused_before':
        return t('step.status.paused_before', { n: line.n, size: line.size });
      case 'running':
        return t('step.status.running', { n: line.n, size: line.size });
      case 'starting':
        return t('step.status.starting');
      case 'none':
        return run.available ? t('step.none') : t('step.unavailable');
    }
  }

  function title(): string {
    if (!doc) return shown(run.sheet) ?? t('step.heading');
    const app_ = doc.app ? `, ${oneLine(doc.app, 40)}` : '';
    return `${oneLine(doc.title, 80)}  [${oneLine(doc.engine, 20)}${app_}]`;
  }

  function stateWord(status: StepStatus): string {
    return t(`step.state.${status}` as Key);
  }

  function choose(n: number): void {
    app.stepChosen = n;
  }

  function onYes(): void {
    void app.stepAnswer(true, answer);
    answer = '';
  }

  function onNo(): void {
    void app.stepAnswer(false, answer);
    answer = '';
  }

  // The keys of the panel, outside a field: n next, c continue, u until the chosen step, b breakpoint, a abort,
  // y yes. A question takes y and n only through its dialog, where the note is typed.
  function onKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (target !== null && target.closest('input, textarea, select, [contenteditable="true"], [role="menu"], dialog') !== null) return;
    if (!run.running) return;
    const actions: Record<string, () => void> = {
      n: () => void app.stepNext(),
      y: () => void app.stepYes(),
      c: () => void app.stepContinue(),
      u: () => void app.stepUntil(),
      b: () => void app.stepToggleBreak(),
      a: () => void app.stepAbort(),
      N: () => noteField?.focus(),
    };
    const action = actions[event.key];
    if (!action) return;
    event.preventDefault();
    action();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<section aria-labelledby="step-heading">
  <button type="button" class="back" onclick={() => app.hideStep()}>← {app.openKey ? t('step.back_repo') : t('detail.back')}</button>
  <h1 id="step-heading">{title()}</h1>
  <p class="status" role="status">{statusText()}</p>

  {#if app.stepFailure}
    <p class="bad" role="alert">{t('step.failed', { message: shown(app.stepFailure) ?? '' })}</p>
  {/if}
  {#if run.error}
    <p class="warn" role="status">{t('step.error', { message: shown(run.error) ?? '' })}</p>
  {/if}
  {#if run.exit?.detail}
    <pre>{shown(run.exit.detail)}</pre>
  {/if}

  {#if doc}
    <div class="controls" role="toolbar" aria-label={t('step.controls')}>
      <button type="button" onclick={() => void app.stepNext()} disabled={!paused}>{t('step.next')}</button>
      <button type="button" onclick={() => void app.stepContinue()} disabled={!paused}>{t('step.continue')}</button>
      <button type="button" onclick={() => void app.stepUntil()} disabled={!canUntil}>{t('step.until')}</button>
      <button type="button" onclick={() => void app.stepToggleBreak()} disabled={app.stepChosen === null || !run.running}>{t('step.break')}</button>
      <button type="button" onclick={() => void app.stepAbort()} disabled={!run.running}>{t('step.abort')}</button>
      <span class="muted keys">{t('step.keys')}</span>
    </div>

    <form class="controls note" onsubmit={onNote}>
      <label>
        {t('step.note_label')}
        <input type="text" maxlength="1000" bind:value={note} bind:this={noteField} disabled={!run.running} />
      </label>
      <button type="submit" disabled={!run.running || note.trim() === ''}>{t('step.note_add')}</button>
    </form>

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
            <button
              type="button"
              class="dot"
              aria-pressed={run.breaks.includes(step.n)}
              aria-label={t('step.break_on', { n: step.n })}
              onclick={() => void app.stepToggleBreak(step.n)}>{run.breaks.includes(step.n) ? '●' : '○'}</button>
            <button type="button" class="row" onclick={() => choose(step.n)}>
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
              <Picture {app} {t} size={run.size} n={tileStepDoc.n} kind="image" label={t('step.picture')} />
            {/if}
            {#if tileMark.shot && run.size}
              <Picture {app} {t} size={run.size} n={tileStepDoc.n} kind="shot" label={t('step.shot')} />
            {/if}
          {:else}
            <p class="muted">{t('step.not_yet')}</p>
          {/if}
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

  {#if run.question}
    <div class="question" role="dialog" aria-modal="false" aria-labelledby="step-question">
      <h2 id="step-question">{t('step.question')}</h2>
      <p class="ask">{shown(run.question.text)}</p>
      <label>
        {t('step.answer_text')}
        <input type="text" maxlength="400" bind:value={answer} />
      </label>
      <div class="controls">
        <button type="button" onclick={onYes}>{t('step.yes')}</button>
        <button type="button" onclick={onNo}>{t('step.no')}</button>
      </div>
    </div>
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
  .status {
    margin: 0 0 0.5rem;
  }
  .muted {
    color: var(--muted);
  }
  .bad,
  .fail {
    color: var(--bad);
  }
  .warn {
    color: var(--warn);
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
  .controls button {
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0.2rem 0.6rem;
    font: inherit;
    cursor: pointer;
  }
  .controls button:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .controls button:disabled {
    color: var(--muted);
    cursor: default;
  }
  .controls {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    align-items: center;
    margin: 0.5rem 0;
  }
  .keys {
    font-size: 0.85em;
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
  .question {
    margin-top: 1rem;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    padding: 0.75rem 1rem;
    background: var(--surface);
  }
  .question label {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }
  .question input {
    flex: 1;
  }
  .failures {
    padding-left: 1.25rem;
  }
</style>
