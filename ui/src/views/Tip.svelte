<script lang="ts">
  // A short word of a table with the sentence that explains it: shown on hover and when something in it has the
  // focus, and read out after the word (a screen reader gets the sentence without a pointer).
  import type { Snippet } from 'svelte';

  let { text, end = false, children }: { text: string; end?: boolean; children: Snippet } = $props();
</script>

<span class="tip" class:end data-tip={text}>{@render children()}<span class="sr">. {text}</span></span>

<style>
  .tip {
    position: relative;
    display: inline-block;
  }
  .tip::after {
    content: attr(data-tip);
    position: absolute;
    z-index: 20;
    top: calc(100% + 0.3rem);
    left: 0;
    width: max-content;
    max-width: min(22rem, 80vw);
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--text);
    font-weight: 400;
    white-space: normal;
    box-shadow: var(--shadow);
    pointer-events: none;
    display: none;
  }
  /* A column at the right edge: the sentence grows to the left, not out of the page. */
  .tip.end::after {
    left: auto;
    right: 0;
  }
  .tip:hover::after,
  .tip:focus-within::after {
    display: block;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
