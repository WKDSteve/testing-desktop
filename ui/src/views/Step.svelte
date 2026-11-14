<script lang="ts">
  // A test sheet step by step, as the panel of testing.nvim shows it: the steps of the size that runs with their
  // run and breakpoints, the keys of a debugger (next, continue, until here, abort), and the tile of the chosen
  // step (StepView.svelte). A question of the sheet is a dialog with yes, no and a note. The page never runs a step
  // itself: every key is a command to the hub.
  import type { App } from '../lib/app.svelte';
  import type { Key } from '../lib/i18n';
  import { shown } from '../lib/model';
  import { EXPLAIN_KINDS, NOTE_MAX_BYTES, noteProblem, oneLine, sheetTitle, statusLine } from '../lib/step';
  import CopyText from './CopyText.svelte';
  import StepView from './StepView.svelte';

  let { app, t }: { app: App; t: (key: Key, params?: Record<string, string | number>) => string } = $props();

  const run = $derived(app.step);
  const doc = $derived(run.doc);
  const canUntil = $derived(app.stepChosen !== null && app.stepChosen > (run.at ?? 0) && run.paused !== null && !run.question);
  const paused = $derived(run.running && run.paused !== null && !run.question);
  // The run reads a question about a step while it waits: paused, or for an answer.
  const canExplain = $derived(run.running && (run.paused !== null || run.question !== null));

  let answer = $state('');
  let note = $state('');
  let noteField: HTMLInputElement | undefined = $state();

  let sendingNote = $state(false);
  const noteTrouble = $derived(noteProblem(note));

  // One note at a time; what is typed while it is on its way stays in the field.
  async function onNote(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (sendingNote || noteTrouble !== null) return;
    const sent = note;
    sendingNote = true;
    try {
      if ((await app.stepNote(sent)) && note === sent) note = '';
    } finally {
      sendingNote = false;
    }
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
    // N with Shift only: with Caps Lock on, an n would land in the note field instead of running the step
    if (event.key === 'N' && !event.shiftKey) return;
    const action = actions[event.key];
    if (!action) return;
    event.preventDefault();
    action();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<section aria-labelledby="step-heading">
  <button type="button" class="back" onclick={() => app.hideStep()}>← {app.openKey ? t('step.back_repo') : t('detail.back')}</button>
  <h1 id="step-heading">{sheetTitle(doc) ?? shown(run.sheet) ?? t('step.heading')}</h1>
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
        <input
          type="text"
          bind:value={note}
          bind:this={noteField}
          disabled={!run.running}
          aria-invalid={noteTrouble === 'too_long'}
          aria-describedby={noteTrouble === 'too_long' ? 'step-note-hint' : undefined}
        />
      </label>
      <button type="submit" disabled={!run.running || noteTrouble !== null || sendingNote}>{t('step.note_add')}</button>
      {#if noteTrouble === 'too_long'}
        <span id="step-note-hint" class="warn" role="status">{t('step.note_too_long', { max: NOTE_MAX_BYTES })}</span>
      {/if}
    </form>

    <StepView
      {run}
      {t}
      chosen={app.stepChosen}
      onChoose={(n) => (app.stepChosen = n)}
      loadPicture={(size, n, kind, signal) => app.stepImage(size, n, kind, signal)}
      onToggleBreak={(n) => void app.stepToggleBreak(n)}
    >
      {#snippet tileExtra(step, mark)}
        {@const request = app.stepExplain && app.stepExplain.n === step.n && app.stepExplain.size === run.size ? app.stepExplain : null}
        <!-- An offer, never a prompt: closed until a person opens it, and nothing is asked before a click. -->
        <details class="explain">
          <summary>{t('explain.heading')}</summary>
          <p class="muted">{t('explain.lead')}</p>
          <div class="controls">
            {#each EXPLAIN_KINDS as kind (kind)}
              <button type="button" disabled={!canExplain || (kind === 'result' && !mark)} onclick={() => void app.stepExplainAsk(step.n, kind)}>
                {t(`explain.kind.${kind}` as Key)}
              </button>
            {/each}
          </div>
          {#if !canExplain}
            <p class="muted">{t('explain.when')}</p>
          {/if}
          {#if app.stepExplainAsked?.n === step.n}
            <p class="muted" role="status">{t('explain.asking')}</p>
          {/if}
          {#if request}
            <h3>{t('explain.request', { n: request.n, question: t(`explain.kind.${request.kind}` as Key) })}</h3>
            <p class="target">{t('explain.target')}</p>
            <p class="label">{t('explain.prompt')}</p>
            <CopyText text={`${request.system}\n\n${request.prompt}`} label={t('explain.copy_label')} {t} block />
          {/if}
        </details>
      {/snippet}
    </StepView>
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
  .bad {
    color: var(--bad);
  }
  .warn {
    color: var(--warn);
  }
  pre {
    font-family: var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0.5rem;
    margin: 0.25rem 0;
    max-height: 30rem;
    overflow: auto;
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
  .explain {
    margin-top: 0.75rem;
    border-top: 1px solid var(--line);
    padding-top: 0.5rem;
  }
  .explain summary {
    cursor: pointer;
    color: var(--accent);
  }
  .explain h3 {
    font-size: 0.95rem;
    margin: 0.75rem 0 0.25rem;
  }
  .target {
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.4rem 0.6rem;
    margin: 0.25rem 0;
  }
  .label {
    margin: 0.4rem 0 0;
    color: var(--muted);
    font-size: 0.85em;
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
</style>
