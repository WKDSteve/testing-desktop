<script lang="ts">
  // The release assistant for the runner guard: the steps of testing.nvim's `testing ci release`, one after the other.
  // A release lets the code of a commit run on the self-hosted runner of this machine, so the page decides nothing: it
  // shows what the engine says (the repositories, the commits, what changed and which files deserve a close look), and
  // the way to the release leads through that review aid and a question. Every text of a commit is foreign text.
  import type { App } from '../lib/app.svelte';
  import type { Key } from '../lib/i18n';
  import { shown } from '../lib/model';
  import { filesFirstFlagged, releaseCommand, shortSha, warningsOf, type Stage } from '../lib/release';
  import CopyText from './CopyText.svelte';

  let { app, t }: { app: App; t: (key: Key, params?: Record<string, string | number>) => string } = $props();

  const w = $derived(app.release);
  const STAGES: Stage[] = ['repo', 'commit', 'review', 'confirm', 'done'];
  const warnings = $derived(w.review ? warningsOf(w.review) : []);
  const files = $derived(w.review ? filesFirstFlagged(w.review) : []);
  const commit = $derived(w.commits?.find((c) => c.sha === w.sha) ?? null);
  const command = $derived(releaseCommand(w.repo, w.sha));

  // A new step begins at its heading: the keyboard and a screen reader go on from there.
  let heading: HTMLHeadingElement | undefined = $state();
  let lastStage: Stage | null = null;
  $effect(() => {
    const stage = w.stage;
    if (lastStage !== null && stage !== lastStage) heading?.focus();
    lastStage = stage;
  });

  function day(iso: string): string {
    return iso.slice(0, 10);
  }
</script>

<section aria-labelledby="release-heading">
  <button type="button" class="back" onclick={() => app.hideRelease()}>← {app.openKey ? t('step.back_repo') : t('detail.back')}</button>
  <h1 id="release-heading">{t('release.heading')}</h1>
  <p class="muted">{t('release.lead')}</p>

  <ol class="stages" aria-label={t('release.stages')}>
    {#each STAGES as stage, i (stage)}
      <li class:current={stage === w.stage} class:done={STAGES.indexOf(w.stage) > i} aria-current={stage === w.stage ? 'step' : undefined}>
        {t(`release.stage.${stage}` as Key)}
      </li>
    {/each}
  </ol>

  {#if w.failure}
    <p class="bad" role="alert">{t('release.failed', { message: shown(w.failure) ?? '' })}</p>
  {/if}
  {#if w.busy}
    <p class="muted" role="status">{w.stage === 'confirm' ? t('release.releasing') : t('release.asking')}</p>
  {/if}

  {#if w.stage === 'repo'}
    <h2 bind:this={heading} tabindex="-1">{t('release.repo.heading')}</h2>
    {#if w.repos && w.repos.length > 0}
      <p class="muted">{t('release.repo.note')}</p>
      <ul class="choices">
        {#each w.repos as repo (repo.name)}
          <li>
            <button type="button" class="choice" disabled={w.busy} onclick={() => void app.releaseChooseRepo(repo.name)}>
              <span class="name">{shown(repo.name)}</span>
              {#if repo.red}<span class="tag red">{t('release.repo.red')}</span>{/if}
              <span class="muted">
                {repo.listed ? t('release.repo.listed', { mode: shown(repo.mode) ?? '', n: repo.reviewed }) : t('release.repo.unlisted')}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {:else if w.repos}
      <p class="muted">{t('release.repo.none')}</p>
    {/if}
  {:else if w.stage === 'commit'}
    <h2 bind:this={heading} tabindex="-1">{t('release.commit.heading', { repo: shown(w.repo) ?? '' })}</h2>
    {#if w.commits && w.commits.length > 0}
      <p class="muted">{t('release.commit.note')}</p>
      <ul class="choices">
        {#each w.commits as c, i (i)}
          <li>
            <button type="button" class="choice" disabled={w.busy} onclick={() => void app.releaseChooseCommit(c.sha)}>
              <span class="mono">{shortSha(c.sha)}</span>
              <span class="name">{shown(c.title)}</span>
              {#if c.released}<span class="tag ok">{t('release.commit.released')}</span>{/if}
              <span class="muted">{shown(c.author)}, {shown(day(c.date))}</span>
            </button>
          </li>
        {/each}
      </ul>
    {:else if w.commits}
      <p class="muted">{t('release.commit.none')}</p>
    {/if}
    <div class="controls">
      <button type="button" disabled={w.busy} onclick={() => app.releaseBack()}>{t('release.back')}</button>
    </div>
  {:else if w.stage === 'review'}
    <h2 bind:this={heading} tabindex="-1">{t('release.review.heading')}</h2>
    <p>
      {t('release.review.commit', { repo: shown(w.repo) ?? '' })}
      <span class="mono">{w.sha}</span>{#if commit}: {shown(commit.title)}{/if}
    </p>
    {#if w.review}
      {@const review = w.review}
      {#if review.already}
        <p class="note">{t('release.review.already')}</p>
      {:else}
        <p class="muted">
          {review.prev ? t('release.review.since', { sha: shortSha(review.prev) }) : t('release.review.no_prev')}
          {#if review.compared}{t('release.review.counts', { commits: review.totalCommits ?? review.commits.length, files: review.filesTotal })}{/if}
        </p>
        {#if warnings.length > 0}
          <ul class="warnings" aria-label={t('release.review.warnings')}>
            {#each warnings as warning (warning)}
              <li>
                {t(`release.warn.${warning}` as Key, { status: shown(review.status) ?? '', problem: shown(review.problem) ?? '' })}
              </li>
            {/each}
          </ul>
        {/if}
        {#if review.commits.length > 0}
          <h3>{t('release.review.commits')}</h3>
          <ul class="plain">
            {#each review.commits as c, i (i)}
              <li><span class="mono">{shortSha(c.sha)}</span> {shown(c.title)} <span class="muted">({shown(c.author)})</span></li>
            {/each}
          </ul>
        {/if}
        {#if files.length > 0}
          <h3>{t('release.review.files')}</h3>
          <ul class="plain files">
            {#each files as f, i (i)}
              <li class:flagged={f.flag !== null}>
                <span class="muted status">{shown(f.status)}</span>
                <span class="mono">{shown(f.path)}</span>
                {#if f.previous}<span class="muted">({t('release.review.was', { path: shown(f.previous) ?? '' })})</span>{/if}
                {#if f.flag}<span class="tag red">{t('release.review.look', { why: shown(f.flag) ?? '' })}</span>{/if}
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
      <label class="looked">
        <input type="checkbox" checked={w.looked} disabled={w.busy} onchange={(e) => app.releaseLooked(e.currentTarget.checked)} />
        {review.already ? t('release.review.looked_already') : t('release.review.looked')}
      </label>
    {/if}
    <div class="controls">
      <button type="button" disabled={w.busy} onclick={() => app.releaseBack()}>{t('release.back')}</button>
      <button type="button" disabled={w.busy || w.review === null || !w.looked} onclick={() => app.releaseToConfirm()}>{t('release.review.next')}</button>
    </div>
  {:else if w.stage === 'confirm'}
    <h2 bind:this={heading} tabindex="-1">{t('release.confirm.heading')}</h2>
    <p>{t('release.confirm.ask', { repo: shown(w.repo) ?? '' })}</p>
    <p class="mono sha">{w.sha}</p>
    {#if commit}<p>{shown(commit.title)} <span class="muted">({shown(commit.author)}, {shown(day(commit.date))})</span></p>{/if}
    <p class="muted">{t('release.confirm.note')}</p>
    <div class="controls">
      <button type="button" disabled={w.busy} onclick={() => app.releaseBack()}>{t('release.back')}</button>
      <button type="button" class="primary" disabled={w.busy} onclick={() => void app.releaseDo()}>{t('release.confirm.go')}</button>
    </div>
  {:else}
    <h2 bind:this={heading} tabindex="-1">{t('release.done.heading')}</h2>
    <p>
      <span class="mono">{shown(w.repo)} {w.sha ? shortSha(w.sha) : ''}</span>:
      {#if w.done?.recorded}
        <span class="ok">{w.done.already ? t('release.done.already') : t('release.done.recorded')}</span>
      {:else}
        <span class="bad">{t('release.done.not_recorded')}</span>
      {/if}
    </p>
    {#if w.doneProblem}
      <p class="bad" role="alert">{t('release.done.problem', { message: shown(w.doneProblem) ?? '' })}</p>
    {/if}
    {#if w.done}
      <h3>{t('release.done.reruns')}</h3>
      {#if w.done.reruns.length === 0}
        <p class="muted">{t('release.done.no_reruns')}</p>
      {:else}
        <ul class="plain">
          {#each w.done.reruns as r, i (i)}
            <li>
              <span class="mono">{r.id}</span>
              {shown(r.name)}
              <span class="muted">({t('release.done.was', { was: shown(r.was) ?? '' })})</span>
              {#if r.ok}<span class="ok">{t('release.done.rerun_ok')}</span>{:else}<span class="bad">{t('release.done.rerun_failed', { message: shown(r.problem) ?? '' })}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
    <div class="controls">
      <button type="button" onclick={() => app.showRelease()}>{t('release.done.another')}</button>
      <button type="button" onclick={() => app.hideRelease()}>{t('release.done.leave')}</button>
    </div>
  {/if}

  {#if command && (w.stage === 'review' || w.stage === 'confirm')}
    <h3>{t('release.command.heading')}</h3>
    <p class="muted">{t('release.command.note')}</p>
    <CopyText text={command} label={t('release.command.label')} {t} />
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
    font-size: 1.05rem;
    margin: 1rem 0 0.5rem;
  }
  h3 {
    font-size: 0.95rem;
    margin: 0.9rem 0 0.3rem;
  }
  h2:focus {
    outline: none;
  }
  .muted {
    color: var(--muted);
  }
  .bad {
    color: var(--bad);
  }
  .ok {
    color: var(--ok);
  }
  .mono {
    font-family: var(--mono);
  }
  .sha {
    overflow-wrap: anywhere;
  }
  .stages {
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 1rem;
    padding: 0;
    margin: 0.5rem 0;
    counter-reset: stage;
    color: var(--muted);
  }
  .stages li {
    counter-increment: stage;
  }
  .stages li::before {
    content: counter(stage) '. ';
  }
  .stages li.current {
    color: var(--text);
    font-weight: 700;
    border-bottom: 2px solid var(--accent);
  }
  .stages li.done {
    color: var(--text);
  }
  .choices,
  .plain {
    list-style: none;
    padding: 0;
    margin: 0.25rem 0;
    display: grid;
    gap: 0.25rem;
  }
  .choice {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.75rem;
    align-items: baseline;
    width: 100%;
    text-align: left;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.4rem 0.6rem;
    font: inherit;
    cursor: pointer;
  }
  .choice:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .choice:disabled {
    cursor: default;
    color: var(--muted);
  }
  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .tag {
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    padding: 0 0.4rem;
    font-size: 0.85em;
  }
  .tag.red {
    border-color: var(--bad);
    color: var(--bad);
  }
  .tag.ok {
    border-color: var(--ok);
    color: var(--ok);
  }
  .warnings {
    margin: 0.5rem 0;
    padding: 0.5rem 0.75rem 0.5rem 1.75rem;
    border: 1px solid var(--warn);
    border-radius: var(--radius);
    color: var(--warn);
  }
  .note {
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 0.5rem 0.75rem;
  }
  .files li {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.5rem;
    align-items: baseline;
    overflow-wrap: anywhere;
  }
  .files li.flagged {
    font-weight: 600;
  }
  .status {
    min-width: 5rem;
  }
  .looked {
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
    margin: 0.9rem 0 0.25rem;
  }
  .controls {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    align-items: center;
    margin: 0.75rem 0;
  }
  .controls button {
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0.25rem 0.75rem;
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
  .controls button.primary:not(:disabled) {
    background: var(--accent);
    color: var(--accent-text);
    border-color: var(--accent);
  }
  .controls button.primary:hover:not(:disabled) {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }
</style>
