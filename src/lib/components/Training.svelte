<script lang="ts">
  // Training: LoRA / QLoRA fine-tuning of a library model on a local JSONL dataset. The wizard
  // checks and estimates a job, the backend runs it in the managed Python environment, and a
  // finished adapter is exported into the library (merged safetensors, optionally GGUF).
  import './common/polished-pages.css';
  import { onMount } from 'svelte';
  import { Plus, GraduationCap } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { training } from '#lib/stores/training.svelte.js';
  import type { TrainingJob } from '#lib/training-types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import EnvCard from './training/EnvCard.svelte';
  import Wizard from './training/Wizard.svelte';
  import JobList from './training/JobList.svelte';
  import LossChart from './training/LossChart.svelte';
  import CompatCard from './training/CompatCard.svelte';
  import LogTail from './training/LogTail.svelte';
  import ExportDialog from './training/ExportDialog.svelte';
  import { showsTorchEnv } from './training/wizardConfig';
  import { formatBytes } from '#lib/format.js';

  let wizardOpen = $state(false);
  let exporting = $state<TrainingJob | null>(null);
  let now = $state(Math.floor(Date.now() / 1000));

  // Newest first.
  let jobs = $derived([...training.jobs].sort((a, b) => b.id - a.id));
  let selected = $derived(training.selected);
  let memory = $derived(training.metrics.findLast(m => m.vram_bytes != null));
  let samples = $derived(training.metrics.filter(m => m.samples?.length));

  onMount(() => {
    void training.refresh();
    void training.refreshEnv();
    void training.loadCompat();
    training.startPolling();
    const tick = setInterval(() => (now = Math.floor(Date.now() / 1000)), 1000);
    return () => {
      training.stopPolling();
      clearInterval(tick);
    };
  });

  // The selected job's series is read once when the job changes state, then polled while it runs.
  $effect(() => {
    void training.selectedId;
    void selected?.status;
    void training.loadMetrics();
  });
</script>

<div class="h-full overflow-y-auto">
  <div class="mx-auto flex max-w-5xl flex-col gap-4 p-4">
    <header class="flex flex-wrap items-center gap-3">
      <GraduationCap class="size-5 text-muted-foreground" aria-hidden="true" />
      <div class="min-w-0 flex-1">
        <h2 class="text-lg font-semibold">{t('training.title')}</h2>
        <p class="text-xs text-muted-foreground">{t('training.subtitle')}</p>
      </div>
      <Button onclick={() => (wizardOpen = true)}><Plus class="size-4" aria-hidden="true" />{t('training.new_job')}</Button>
    </header>

    {#if showsTorchEnv(training.compat?.devices, training.env, training.installing)}<EnvCard />{/if}

    {#if training.actionError}
      <p class="text-xs text-destructive" role="alert">{training.actionError}</p>
    {/if}

    <section class="flex flex-col gap-2" aria-labelledby="training-jobs">
      <h3 id="training-jobs" class="text-sm font-semibold">{t('training.jobs_title')}</h3>
      {#if jobs.length === 0}
        <p class="rounded-md border border-dashed border-border p-6 text-center text-sm text-muted-foreground">{t('training.jobs_empty')}</p>
      {:else}
        <JobList {jobs} {now} onExport={(job) => (exporting = job)} />
      {/if}
    </section>

    <section class="flex flex-col gap-3 rounded-lg border border-border p-4" aria-labelledby="training-chart">
      <h3 id="training-chart" class="text-sm font-semibold">
        {t('training.chart_title')}{#if selected}: <span class="font-normal text-muted-foreground">{selected.config.name}</span>{/if}
      </h3>
      {#if selected}
        <LossChart metrics={training.metrics} totalSteps={selected.total_steps} />
        <p class="text-xs text-muted-foreground">{t('training.live_vram')}: {memory?.vram_bytes != null ? formatBytes(memory.vram_bytes) : t('training.memory_unavailable')}{#if memory?.vram_total_bytes} / {formatBytes(memory.vram_total_bytes)}{/if}</p>
        <section class="flex flex-col gap-2" aria-label={t('training.checkpoint_samples')}>
          <h4 class="text-sm font-semibold">{t('training.checkpoint_samples')}</h4>
          {#if !samples.length}<p class="text-xs text-muted-foreground">{t('training.samples_empty')}</p>{/if}
          {#each samples as metric (metric.step)}
            <details class="rounded-md border p-2 text-sm">
              <summary class="cursor-pointer">{t('training.step_of', { step: metric.step, total: selected.total_steps })}</summary>
              {#each metric.samples ?? [] as sample}
                <p class="mt-3 whitespace-pre-wrap font-medium">{sample.prompt}</p>
                <p class="mt-1 whitespace-pre-wrap">{sample.text}</p>
              {/each}
            </details>
          {/each}
        </section>
        <LogTail job={selected} />
      {:else}
        <p class="text-sm text-muted-foreground">{t('training.chart_select')}</p>
      {/if}
    </section>

    <CompatCard />
  </div>
</div>

<Wizard bind:open={wizardOpen} />
<ExportDialog job={exporting} onClose={() => (exporting = null)} />
