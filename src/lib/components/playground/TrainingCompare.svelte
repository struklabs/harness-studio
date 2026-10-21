<script lang="ts">
  import { training } from '#lib/stores/training.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { canCompare, comparePrompts, compareRun } from '../training/runActions';
  import type { TrainingComparisonRow } from '#lib/training-types.js';

  let job = $derived(training.jobs.find(j => j.id === training.compareJobId));
  let prompts = $state('');
  let rows = $state<TrainingComparisonRow[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let initializedId: number | null = null;
  $effect(() => {
    const id = job?.id ?? null;
    if (id === initializedId) return;
    initializedId = id;
    prompts = job?.config.sample_prompts?.join('\n') ?? '';
    rows = [];
    error = null;
  });
  async function run() {
    const list = comparePrompts(prompts);
    if (!job || !list || busy) return;
    const id = job.id;
    busy = true;
    rows = [];
    error = null;
    try {
      if (!(await training.ensureEnv(job.config.device))) throw new Error(training.installError ?? t('training.env_required'));
      const result = await compareRun(list, request => invoke<TrainingComparisonRow[]>('training_compare', { jobId: id, request }));
      if (id === training.compareJobId) rows = result;
    } catch (e) { if (id === training.compareJobId) error = errorText(e); }
    finally { busy = false; }
  }
</script>

{#if job}
  <section class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4" aria-label={t('training.compare_title')}>
    <div class="flex items-center justify-between gap-2">
      <h2 class="text-lg font-semibold">{t('training.compare_title')}: {job.config.name}</h2>
      <Button variant="outline" disabled={busy} onclick={() => training.compareJobId = null}>{t('training.compare_close')}</Button>
    </div>
    <p class="text-sm text-muted-foreground">{t('training.compare_hint')}</p>
    <label class="flex flex-col gap-1 text-sm">{t('training.sample_prompts')}
      <textarea class="min-h-24 rounded-md border bg-background p-2" bind:value={prompts} disabled={busy}></textarea>
      <span class="text-xs text-muted-foreground">{t('training.sample_prompts_hint')}</span>
    </label>
    <Button class="self-start" disabled={busy || !canCompare(job) || !comparePrompts(prompts)} onclick={run}>{busy ? t('training.compare_running') : t('training.compare_start')}</Button>
    {#if error}<p class="text-sm text-destructive" role="alert">{error}</p>{/if}
    {#each rows as row}
      <article class="rounded-lg border p-3">
        <p class="mb-3 whitespace-pre-wrap font-medium">{row.prompt}</p>
        <div class="grid gap-4 md:grid-cols-2">
          <div><h3 class="mb-2 text-sm font-semibold">{t('training.compare_base')}</h3><p class="whitespace-pre-wrap text-sm">{row.base}</p></div>
          <div><h3 class="mb-2 text-sm font-semibold">{t('training.compare_trained')}</h3><p class="whitespace-pre-wrap text-sm">{row.trained}</p></div>
        </div>
      </article>
    {/each}
  </section>
{/if}
