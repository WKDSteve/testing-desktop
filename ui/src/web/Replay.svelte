<script lang="ts">
  // A recorded stepped run, played back: the same view as the hub's "Step by step" page (StepView.svelte) on the lines
  // of a recording, with a position a person moves. Nothing runs and nothing is asked of anyone: the events and the
  // pictures come from the folder that was chosen.
  import type { Key } from '../lib/i18n';
  import { shown } from '../lib/model';
  import { hasEnd, lastPosition, stateAt } from '../lib/replay';
  import { oneLine, sheetTitle } from '../lib/step';
  import StepView from '../views/StepView.svelte';
  import { pictureOf, type Recording } from './recording';

  let {
    recording,
    name,
    t,
    onOther,
  }: {
    recording: Recording;
    /** The name of the folder the recording was read from. */
    name: string;
    t: (key: Key, params?: Record<string, string | number>) => string;
    /** Back to the way in, for another folder. */
    onOther: () => void;
  } = $props();

  const last = $derived(lastPosition(recording.events));
  // A recording opens at its end: what the run came to. `null` follows the end, also of another recording.
  let moved = $state<number | null>(null);
  const position = $derived(moved === null ? last : Math.min(moved, last));
  let chosen = $state<number | null>(null);

  const run = $derived(stateAt(recording.events, position));
  const cut = $derived(!hasEnd(recording.events));

  function go(to: number): void {
    moved = Math.min(Math.max(to, 0), last);
    // The tile follows the position again: the last step that did something there.
    chosen = null;
  }

  function statusText(): string {
    if (position === last && run.finished) {
      const skipped = run.finished.skipped ? ` (${oneLine(run.finished.skipped, 100)})` : '';
      return t('step.status.finished', { status: run.finished.status, ms: run.finished.ms ?? 0 }) + skipped;
    }
    return t('replay.position', { done: position, total: last });
  }

  // The keys, outside a field: n forward, p back, s to the start, e to the end.
  function onKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (target !== null && target.closest('input, textarea, select, [contenteditable="true"], [role="menu"], dialog') !== null) return;
    const actions: Record<string, () => void> = {
      n: () => go(position + 1),
      p: () => go(position - 1),
      s: () => go(0),
      e: () => go(last),
    };
    const action = actions[event.key];
    if (!action) return;
    event.preventDefault();
    action();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<section aria-labelledby="replay-heading">
  <h1 id="replay-heading">{sheetTitle(run.doc) ?? t('replay.heading')}</h1>
  <p class="muted">{t('replay.lead', { name: shown(name) ?? '' })}</p>
  <p class="status" role="status">{statusText()}</p>
  {#if cut && position === last}
    <p class="warn" role="status">{t('replay.cut')}</p>
  {/if}

  <div class="controls" role="toolbar" aria-label={t('replay.controls')}>
    <button type="button" onclick={() => go(0)} disabled={position === 0}>{t('replay.first')}</button>
    <button type="button" onclick={() => go(position - 1)} disabled={position === 0}>{t('replay.back')}</button>
    <button type="button" onclick={() => go(position + 1)} disabled={position === last}>{t('replay.forward')}</button>
    <button type="button" onclick={() => go(last)} disabled={position === last}>{t('replay.last')}</button>
    <span class="muted keys">{t('replay.keys')}</span>
    <button type="button" class="other" onclick={onOther}>{t('web.other_folder')}</button>
  </div>

  <StepView
    {run}
    {t}
    {chosen}
    onChoose={(n) => (chosen = n)}
    loadPicture={(size, n, kind) => pictureOf(recording, size, n, kind)}
  />
</section>

<style>
  h1 {
    font-size: 1.25rem;
    margin: 0.5rem 0;
  }
  .status {
    margin: 0 0 0.5rem;
  }
  .muted {
    color: var(--muted);
  }
  .warn {
    color: var(--warn);
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
  .other {
    margin-left: auto;
  }
</style>
