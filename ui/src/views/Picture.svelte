<script lang="ts" module>
  /** Reads the picture of a step: from the hub with the session, or from the folder of a recording. */
  export type LoadPicture = (size: string, n: number, kind: 'image' | 'shot', signal?: AbortSignal) => Promise<Blob>;
</script>

<script lang="ts">
  // The picture of a step: read through `load` (from the hub with the session, since an `<img>` could carry no token;
  // from the folder of a recording in the web variant) and drawn into a canvas. Whatever the file holds is pixels
  // here, never markup.
  import type { Key } from '../lib/i18n';
  import { shown } from '../lib/model';
  import { messageOf } from '../lib/reader.svelte';

  let {
    load,
    t,
    size,
    n,
    kind,
    label,
  }: {
    load: LoadPicture;
    t: (key: Key, params?: Record<string, string | number>) => string;
    size: string;
    n: number;
    kind: 'image' | 'shot';
    label: string;
  } = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let failure = $state<string | null>(null);

  $effect(() => {
    const target = canvas;
    const at = { size, n, kind };
    const read = load;
    if (!target) return;
    const abort = new AbortController();
    failure = null;
    void (async () => {
      try {
        const blob = await read(at.size, at.n, at.kind, abort.signal);
        const bitmap = await createImageBitmap(blob);
        if (abort.signal.aborted) return;
        target.width = bitmap.width;
        target.height = bitmap.height;
        target.getContext('2d')?.drawImage(bitmap, 0, 0);
        bitmap.close();
      } catch (error) {
        if (!abort.signal.aborted) failure = messageOf(error);
      }
    })();
    return () => abort.abort();
  });
</script>

<figure>
  <figcaption class="label">{label}</figcaption>
  {#if failure}
    <p class="bad" role="status">{t('step.picture_failed', { message: shown(failure) ?? '' })}</p>
  {/if}
  <div role="img" aria-label={label}><canvas bind:this={canvas}></canvas></div>
</figure>

<style>
  figure {
    margin: 0.5rem 0 0;
  }
  canvas {
    max-width: 100%;
    height: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .label {
    margin: 0 0 0.25rem;
    color: var(--muted);
    font-size: 0.85em;
  }
  .bad {
    color: var(--bad);
  }
</style>
