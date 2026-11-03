<script lang="ts">
  // One result as a word with a mark: the mark is not the only carrier, the word says it too (colour alone is not
  // enough for every reader).
  import type { Tone } from '../lib/model';

  let { tone, label, link = null }: { tone: Tone; label: string; link?: string | null } = $props();
</script>

{#if link}
  <a class="tone {tone}" href={link} target="_blank" rel="noopener noreferrer"><span class="dot" aria-hidden="true"></span>{label}</a>
{:else}
  <span class="tone {tone}"><span class="dot" aria-hidden="true"></span>{label}</span>
{/if}

<style>
  .tone {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    white-space: nowrap;
  }
  .dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: var(--radius-pill);
    background: var(--muted);
    flex: none;
  }
  .green .dot {
    background: var(--ok);
  }
  .red .dot {
    background: var(--bad);
  }
  .running .dot,
  .waiting .dot {
    background: var(--warn);
  }
  .skipped .dot {
    background: var(--border);
  }
</style>
