<script lang="ts">
  // A text to take along, with a button that copies it: a command for a terminal, a request for an AI. The text is
  // shown whole, so a browser that refuses the clipboard still leaves it to be selected and copied by hand.
  import type { Key } from '../lib/i18n';

  let {
    text,
    label,
    t,
    block = false,
  }: {
    text: string;
    /** What the text is, for a screen reader. */
    label: string;
    t: (key: Key, params?: Record<string, string | number>) => string;
    /** A text of many lines (a box that scrolls) instead of one line. */
    block?: boolean;
  } = $props();

  let outcome = $state<'idle' | 'copied' | 'failed'>('idle');
  // A new text has not been copied yet.
  $effect(() => {
    void text;
    outcome = 'idle';
  });

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      outcome = 'copied';
    } catch {
      outcome = 'failed';
    }
  }
</script>

<div class="copy" class:block>
  <!-- A long text scrolls in its box, so the keyboard must reach it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <pre class="text" tabindex="0" aria-label={label}>{text}</pre>
  <span class="act">
    <button type="button" onclick={() => void copy()}>{t('copy.copy')}</button>
    <span class="outcome" class:bad={outcome === 'failed'} role="status">
      {outcome === 'copied' ? t('copy.copied') : outcome === 'failed' ? t('copy.failed') : ''}
    </span>
  </span>
</div>

<style>
  .copy {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
    margin: 0.25rem 0;
  }
  .copy.block {
    flex-direction: column;
    align-items: stretch;
  }
  .text {
    font-family: var(--mono);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0.35rem 0.5rem;
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 24rem;
    overflow: auto;
    min-width: 0;
  }
  .act {
    display: inline-flex;
    gap: 0.5rem;
    align-items: center;
  }
  button {
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0.2rem 0.6rem;
    font: inherit;
    cursor: pointer;
  }
  button:hover {
    border-color: var(--accent);
  }
  .outcome {
    color: var(--muted);
    font-size: 0.85em;
  }
  .outcome.bad {
    color: var(--bad);
  }
</style>
