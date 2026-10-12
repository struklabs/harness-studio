<script lang="ts">
  // The safetensors to GGUF conversions of the Model Hub, listed next to the download queue:
  // each one shows its stage (environment, download, convert, quantize, finalize) and progress.
  import { formatBytes } from '$lib/format';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import type { ConvertJob } from '$lib/types';
  import { converts, isConvertActive } from '$lib/stores/converts.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Progress } from '$lib/components/ui/progress';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { cn } from '$lib/utils';
  import { CircleAlert, CircleCheck, X } from '@lucide/svelte';

  const uid = $props.id();

  let finished = $derived(converts.jobs.filter((j) => !isConvertActive(j)));

  function stageLabel(job: ConvertJob): string {
    if (job.status === 'failed') return t('cvt.status_failed');
    if (job.status === 'cancelled') return t('cvt.status_cancelled');
    return t(`cvt.stage_${job.stage}` as TKey);
  }

  function tone(job: ConvertJob): string {
    if (job.status === 'failed') return 'text-destructive';
    if (job.status === 'cancelled') return 'text-warning';
    if (job.status === 'completed') return 'text-success';
    return job.status === 'queued' ? 'text-muted-foreground' : 'text-primary';
  }

  function percent(job: ConvertJob): number {
    return Math.round((job.progress ?? 0) * 100);
  }
</script>

{#if converts.jobs.length > 0}
  <section class="flex min-w-0 shrink-0 flex-col rounded-lg border border-border bg-card" aria-labelledby="{uid}-title">
    <header class="flex items-center gap-2 px-3 py-2">
      <h2 id="{uid}-title" class="text-sm font-semibold text-foreground">{t('cvt.queue_title')}</h2>
      {#if converts.activeCount > 0}
        <span class="font-num text-xs text-primary">{t('hub.queue_pending_count', { count: converts.activeCount })}</span>
      {/if}
      <span class="flex-1"></span>
      {#if finished.length > 0}
        <Button variant="ghost" size="xs" onclick={() => converts.clearFinished()}>{t('hub.queue_clear_finished')}</Button>
      {/if}
    </header>

    {#if converts.actionError}
      <div class="mx-3 mb-2 flex items-center justify-between gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive" role="alert">
        <span class="flex min-w-0 items-center gap-1.5"><CircleAlert class="size-3.5 shrink-0" /><span class="break-words">{converts.actionError}</span></span>
        <Button variant="ghost" size="icon-xs" aria-label={t('hub.close')} onclick={() => (converts.actionError = null)}><X /></Button>
      </div>
    {/if}

    <ul class="flex max-h-64 flex-col divide-y divide-border overflow-y-auto border-t border-border">
      {#each converts.jobs as job (job.id)}
        <li class="flex flex-col gap-1.5 px-3 py-2">
          <div class="flex items-center justify-between gap-3">
            <div class="flex min-w-0 flex-col">
              <span class="truncate font-mono text-xs font-medium" title={job.repo_id}>{job.repo_id}</span>
              <span class="truncate text-xs text-muted-foreground">GGUF · {job.quant}</span>
            </div>
            <div class="flex shrink-0 items-center gap-2">
              <span class={cn('flex items-center gap-1 text-xs', tone(job))}>
                {#if job.status === 'completed'}<CircleCheck class="size-3.5" />{/if}{stageLabel(job)}
              </span>
              <Tooltip.Root>
                <Tooltip.Trigger>
                  {#snippet child({ props })}
                    <Button
                      {...props}
                      variant="ghost"
                      size="icon-sm"
                      class="min-h-7 min-w-7"
                      aria-label={isConvertActive(job) ? t('cvt.cancel') : t('cvt.remove')}
                      onclick={() => (isConvertActive(job) ? converts.cancel(job.id) : converts.remove(job.id))}
                    >
                      <X class="size-4" />
                    </Button>
                  {/snippet}
                </Tooltip.Trigger>
                <Tooltip.Content>{isConvertActive(job) ? t('cvt.cancel') : t('cvt.remove')}</Tooltip.Content>
              </Tooltip.Root>
            </div>
          </div>

          {#if job.status === 'running'}
            <Progress value={percent(job)} max={100} aria-label={`${job.repo_id}: ${stageLabel(job)}`} class={cn(job.progress === null && 'animate-pulse')} />
            <div class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-0.5 font-num text-xs text-muted-foreground">
              {#if job.stage === 'downloading'}
                <span>{formatBytes(job.downloaded_bytes)} / {formatBytes(job.total_bytes)}</span>
              {/if}
              {#if job.progress !== null}<span>{percent(job)}%</span>{/if}
              {#if job.detail}<span class="min-w-0 truncate font-mono" title={job.detail}>{job.detail}</span>{/if}
            </div>
          {/if}

          {#if job.status === 'failed' && job.error}
            <p class="text-xs break-words text-destructive">{localizeError(job.error)}</p>
          {/if}
          {#if job.status === 'completed' && job.output}
            <p class="truncate text-xs text-muted-foreground" title={job.output}>{t('cvt.output', { path: job.output })}</p>
          {/if}
          {#if job.warning}
            <p class="text-xs break-words text-warning">{t('cvt.warning_label')}: {localizeError(job.warning)}</p>
          {/if}
          {#if (job.status === 'failed' || job.status === 'cancelled') && job.kept_bytes > 0}
            <p class="text-xs text-muted-foreground">{t('cvt.kept', { size: formatBytes(job.kept_bytes, 1) })}</p>
          {/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}
