<script lang="ts">
  import { Play, Square, RotateCcw, PackageOpen, Trash2 } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { training } from '#lib/stores/training.svelte.js';
  import type { TrainingJob } from '#lib/training-types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { STATUS_CLASS, STATUS_KEY, canExport, elapsedSeconds, fmtDuration, fmtLoss } from './trainingFormat';
  import type { TKey } from '#lib/i18n.svelte.js';

  interface Props {
    jobs: TrainingJob[];
    now: number;
    onExport: (job: TrainingJob) => void;
  }
  let { jobs, now, onExport }: Props = $props();

  let removing = $state<number | null>(null);

  const canCancel = (j: TrainingJob) => j.status === 'draft' || j.status === 'queued' || j.status === 'running';
  const canResume = (j: TrainingJob) => (j.status === 'failed' || j.status === 'cancelled') && j.checkpoint_step !== null;
  const percent = (j: TrainingJob) => (j.total_steps > 0 ? Math.min(100, (j.step / j.total_steps) * 100) : 0);
</script>

<ul class="flex flex-col gap-2" aria-label={t('training.jobs_title')}>
  {#each jobs as job (job.id)}
    {@const elapsed = elapsedSeconds(job, now)}
    <li>
      <div
        role="button"
        tabindex="0"
        class="flex w-full cursor-pointer flex-col gap-2 rounded-lg border p-3 text-left transition-colors hover:bg-muted/40 {training.selectedId === job.id ? 'border-primary' : 'border-border'}"
        aria-pressed={training.selectedId === job.id}
        onclick={() => training.select(job.id)}
        onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && e.target === e.currentTarget && (e.preventDefault(), training.select(job.id))}
      >
        <div class="flex flex-wrap items-center gap-2">
          <span class="min-w-0 flex-1 truncate text-sm font-medium">{job.config.name}</span>
          <span class="rounded-full px-2 py-0.5 text-xs font-medium {STATUS_CLASS[job.status]}">{t(STATUS_KEY[job.status])}</span>
        </div>
        <p class="text-xs text-muted-foreground">
          {t(`training.method_${job.config.method}` as TKey)} · {t(`training.device_${job.config.device}` as TKey)}
          {#if elapsed !== null}· {t('training.elapsed', { time: fmtDuration(elapsed) })}{/if}
          {#if job.resumed}· {t('training.resumed')}{/if}
        </p>

        {#if job.status !== 'draft'}
          <div class="flex flex-col gap-1">
            <Progress value={percent(job)} />
            <div class="flex flex-wrap gap-x-4 text-xs text-muted-foreground">
              <span>{t('training.step_of', { step: job.step, total: job.total_steps })}</span>
              <span>{t('training.loss')}: {fmtLoss(job.loss)}</span>
              <span>{t('training.eval_loss')}: {fmtLoss(job.eval_loss)}</span>
              {#if job.status === 'running' && !job.training}<span>{t('training.loading_model')}</span>{/if}
            </div>
          </div>
        {/if}

        {#if job.error}
          <p class="text-xs text-destructive" role="alert">{errorText(job.error)}</p>
        {/if}

        {#if job.export}
          <div class="rounded-md bg-muted/50 p-2 text-xs">
            <p class="font-medium">
              {t('training.export_status')}: {t(`training.export_${job.export.status}` as TKey)}
            </p>
            {#if job.export.dir}<p class="break-all text-muted-foreground">{t('training.export_folder')}: {job.export.dir}</p>{/if}
            {#if job.export.gguf}<p class="break-all text-muted-foreground">{t('training.export_gguf_file')}: {job.export.gguf}</p>{/if}
            {#if job.export.warning}<p class="text-amber-700 dark:text-amber-400">{errorText(job.export.warning)}</p>{/if}
            {#if job.export.error}<p class="text-destructive">{errorText(job.export.error)}</p>{/if}
          </div>
        {/if}

        <div class="flex flex-wrap gap-1" role="group" aria-label={t('training.actions')}>
          {#if job.status === 'draft'}
            <Button size="sm" disabled={!training.envReady} title={training.envReady ? undefined : t('training.env_required')} onclick={(e) => { e.stopPropagation(); void training.startJob(job.id); }}>
              <Play class="size-3.5" aria-hidden="true" />{t('training.start')}
            </Button>
          {/if}
          {#if canCancel(job)}
            <Button size="sm" variant="outline" onclick={(e) => { e.stopPropagation(); void training.cancel(job.id); }}>
              <Square class="size-3.5" aria-hidden="true" />{t('training.cancel_job')}
            </Button>
          {/if}
          {#if canResume(job)}
            <Button size="sm" disabled={!training.envReady} onclick={(e) => { e.stopPropagation(); void training.resume(job.id); }}>
              <RotateCcw class="size-3.5" aria-hidden="true" />{t('training.resume')}
            </Button>
          {/if}
          {#if canExport(job)}
            <Button size="sm" variant="secondary" onclick={(e) => { e.stopPropagation(); onExport(job); }}>
              <PackageOpen class="size-3.5" aria-hidden="true" />{t('training.export_to_library')}
            </Button>
          {/if}
          {#if removing === job.id}
            <Button size="sm" variant="destructive" onclick={(e) => { e.stopPropagation(); removing = null; void training.remove(job.id); }}>{t('training.delete_confirm')}</Button>
            <Button size="sm" variant="ghost" onclick={(e) => { e.stopPropagation(); removing = null; }}>{t('training.cancel')}</Button>
          {:else if job.status !== 'running' && job.status !== 'queued'}
            <Button size="sm" variant="ghost" onclick={(e) => { e.stopPropagation(); removing = job.id; }}>
              <Trash2 class="size-3.5" aria-hidden="true" />{t('training.delete')}
            </Button>
          {/if}
        </div>
      </div>
    </li>
  {/each}
</ul>
