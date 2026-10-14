<script lang="ts">
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { formatBytes } from '#lib/format.js';
  import { t } from '#lib/i18n.svelte.js';
  import type { DownloadProgress } from '#lib/types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { cn } from '#lib/utils.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { loadAndChat, modelOfJob } from '#lib/stores/downloads.svelte.js';
  import { isChatLoadable } from '#lib/downloadFollowUp.js';
  import { ArrowUpToLine, ChevronDown, ChevronUp, CircleAlert, CircleCheck, MessageSquare, Pause, Play, X } from '@lucide/svelte';
  import {
    DownloadQueue,
    etaSeconds,
    formatDuration,
    isRunning,
    progressPercent,
    statusError,
    statusKey,
    type StatusKey,
  } from './downloadQueue.svelte';

  interface Props {
    queue: DownloadQueue;
  }

  let { queue }: Props = $props();

  const uid = $props.id();

  let collapsed = $state(false);
  /** Finished downloads fold into one summary row; this opens it. */
  let showCompleted = $state(false);
  let activeJobs = $derived(queue.jobs.filter((j) => j.status !== 'Completed'));
  let completedJobs = $derived(queue.jobs.filter((j) => j.status === 'Completed'));
  $effect(() => {
    if (completedJobs.length === 0) showCompleted = false;
  });
  /** Job whose inline remove confirmation is open. */
  let confirmRemoveId = $state<number | null>(null);
  let deletePartial = $state(false);

  const STATUS_LABEL: Record<StatusKey, Parameters<typeof t>[0]> = {
    Idle: 'hub.status_idle',
    Queued: 'hub.status_queued',
    Downloading: 'hub.status_downloading',
    Verifying: 'hub.status_verifying',
    Paused: 'hub.status_paused',
    Completed: 'hub.status_completed',
    Error: 'hub.status_error',
  };

  const STATUS_TONE: Record<StatusKey, string> = {
    Idle: 'text-muted-foreground',
    Queued: 'text-muted-foreground',
    Downloading: 'text-primary',
    Verifying: 'text-info',
    Paused: 'text-warning',
    Completed: 'text-success',
    Error: 'text-destructive',
  };

  /** The finished job's chat-capable library file (null for embeddings, speech, projectors, ...). */
  function chatModelOf(job: DownloadProgress) {
    const model = modelOfJob(job, library.models);
    return model && isChatLoadable(model) ? model : null;
  }

  function jobTitle(job: DownloadProgress): string {
    return job.variant_name || job.current_shard_name;
  }

  function repoLabel(job: DownloadProgress): string {
    return job.repo_id === 'custom_url' ? t('hub.custom_url_repo') : job.repo_id;
  }

  function askRemove(job: DownloadProgress) {
    if (job.status === 'Completed') {
      queue.remove(job.id, false);
      return;
    }
    deletePartial = false;
    confirmRemoveId = job.id;
  }

  async function confirmRemove(job: DownloadProgress) {
    const del = deletePartial;
    confirmRemoveId = null;
    await queue.remove(job.id, del);
  }
</script>

{#snippet iconAction(label: string, onclick: () => void, icon: typeof Play, primary = false)}
  {@const Icon = icon}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant={primary ? 'secondary' : 'ghost'} size="icon-sm" class="min-h-7 min-w-7" aria-label={label} title={label} {onclick}>
          <Icon class="size-4" />
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

<div class="sr-only" role="status" aria-live="polite">{queue.announcement}</div>

{#if queue.jobs.length > 0}
  <section class="flex min-w-0 shrink-0 flex-col rounded-lg border border-border bg-card" aria-labelledby="{uid}-title">
    <header class="flex items-center gap-2 px-3 py-2">
      <h2 id="{uid}-title" class="text-sm font-semibold text-foreground">{t('hub.queue_title')}</h2>
      {#if queue.pendingCount > 0}
        <span class="font-num text-xs text-primary">{t('hub.queue_pending_count', { count: queue.pendingCount })}</span>
      {/if}
      <span class="flex-1"></span>
      {#if activeJobs.length > 0}
      <Button
        variant="ghost"
        size="icon-xs"
        aria-expanded={!collapsed}
        aria-controls="{uid}-list"
        aria-label={collapsed ? t('hub.queue_expand') : t('hub.queue_collapse')}
        onclick={() => (collapsed = !collapsed)}
      >
        {#if collapsed}<ChevronUp />{:else}<ChevronDown />{/if}
      </Button>
      {/if}
    </header>

    {#if queue.actionError}
      <div class="mx-3 mb-2 flex items-center justify-between gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive" role="alert">
        <span class="flex min-w-0 items-center gap-1.5"><CircleAlert class="size-3.5 shrink-0" /><span class="break-words">{queue.actionError}</span></span>
        <Button variant="ghost" size="icon-xs" aria-label={t('hub.close')} onclick={() => (queue.actionError = null)}><X /></Button>
      </div>
    {/if}

    {#if !collapsed && activeJobs.length > 0}
      <ul class="flex max-h-56 flex-col divide-y divide-border overflow-y-auto border-t border-border" id="{uid}-list">
        {#each activeJobs as job (job.id)}
          {@const key = statusKey(job.status)}
          {@const pct = progressPercent(job)}
          {@const eta = etaSeconds(job)}
          {@const err = statusError(job.status)}
          <li class="flex flex-col gap-1.5 px-3 py-2">
            <div class="flex items-center justify-between gap-3">
              <div class="flex min-w-0 flex-col">
                <span class="truncate font-mono text-xs font-medium" title={job.current_shard_name}>{jobTitle(job)}</span>
                <span class="truncate text-xs text-muted-foreground">{repoLabel(job)}</span>
              </div>

              <div class="flex shrink-0 items-center gap-2">
                <span class={cn('text-xs', STATUS_TONE[key])}>{t(STATUS_LABEL[key])}</span>
                {#if confirmRemoveId !== job.id}
                  <div class="flex items-center">
                    {#if key === 'Queued'}
                      {@render iconAction(t('hub.queue_prioritize'), () => queue.prioritize(job.id), ArrowUpToLine)}
                      {@render iconAction(t('hub.cancel'), () => queue.cancel(job.id), X)}
                    {:else if isRunning(job.status)}
                      {@render iconAction(t('hub.queue_pause_title'), () => queue.cancel(job.id), Pause)}
                    {:else if key === 'Paused' || key === 'Error'}
                      {@render iconAction(key === 'Error' ? t('hub.retry_button') : t('hub.queue_resume'), () => queue.resume(job.id), Play, true)}
                      {@render iconAction(t('hub.queue_remove'), () => askRemove(job), X)}
                    {:else if key === 'Completed'}
                      {@render iconAction(t('hub.queue_remove'), () => askRemove(job), X)}
                    {/if}
                  </div>
                {/if}
              </div>
            </div>

            {#if key !== 'Completed'}
              <Progress
                value={pct}
                max={100}
                aria-label={t('hub.progress_label', { name: jobTitle(job) })}
                class={cn(key === 'Error' && '[&>div]:bg-destructive', key === 'Paused' && '[&>div]:bg-warning')}
              />
            {/if}

            <div class="flex flex-wrap items-center gap-x-3 gap-y-0.5 font-num text-xs text-muted-foreground">
              <span>{formatBytes(job.downloaded_bytes)} / {formatBytes(job.total_bytes)}</span>
              {#if key !== 'Completed'}<span>{pct.toFixed(1)}%</span>{/if}
              {#if job.status === 'Downloading'}<span>{formatBytes(job.speed_bytes_per_sec)}/s</span>{/if}
              {#if eta !== null}<span>{t('hub.eta', { time: formatDuration(eta) })}</span>{/if}
              {#if job.total_shards > 1}
                <span>{t('hub.shard_progress', { index: Math.max(1, job.current_shard_index), total: job.total_shards })}</span>
              {/if}
              {#if key === 'Completed'}<span class="font-sans">{t('hub.completed_hint')}</span>{/if}
            </div>

            {#if err}
              <p class="text-xs break-words text-destructive">{err}</p>
            {/if}

            {#if confirmRemoveId === job.id}
              <div class="flex flex-wrap items-center gap-3 text-xs" role="group" aria-label={t('hub.queue_remove_confirm')}>
                <span>{t('hub.queue_remove_confirm')}</span>
                <div class="flex items-center gap-1.5">
                  <FieldHelp text={t('help.delete_partial')}><Checkbox id="{uid}-del-{job.id}" bind:checked={deletePartial} /></FieldHelp>
                  <Label for="{uid}-del-{job.id}" class="text-xs font-normal">{t('hub.queue_delete_partial')}</Label>
                </div>
                <Button variant="destructive" size="xs" onclick={() => confirmRemove(job)}>{t('hub.queue_remove')}</Button>
                <Button variant="ghost" size="xs" onclick={() => (confirmRemoveId = null)}>{t('hub.cancel')}</Button>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <!-- Completed downloads never pile up: one row with a count, opened on demand. -->
    {#if completedJobs.length > 0}
      <div class="flex items-center gap-2 border-t border-border px-3 py-1.5 text-xs">
        <button
          type="button"
          class="flex min-w-0 flex-1 items-center gap-1.5 rounded-sm text-left text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          aria-expanded={showCompleted}
          aria-controls="{uid}-done"
          onclick={() => (showCompleted = !showCompleted)}
        >
          <CircleCheck class="size-3.5 shrink-0 text-success" />
          <span class="truncate">{t('ui.dl_completed', { count: completedJobs.length })}</span>
          <ChevronDown class={cn('size-3.5 shrink-0 transition-transform motion-reduce:transition-none', showCompleted && 'rotate-180')} />
        </button>
        <Button variant="ghost" size="xs" onclick={() => queue.clearFinished()}>{t('hub.queue_clear_finished')}</Button>
      </div>
      {#if showCompleted}
        <ul class="flex max-h-40 flex-col overflow-y-auto border-t border-border py-1" id="{uid}-done">
          {#each completedJobs as job (job.id)}
            {@const chatModel = chatModelOf(job)}
            <li class="flex items-center gap-2 px-3 py-1 text-xs">
              <span class="min-w-0 flex-1 truncate font-mono" title={`${repoLabel(job)} · ${job.current_shard_name}`}>{jobTitle(job)}</span>
              <span class="shrink-0 font-num text-muted-foreground">{formatBytes(job.total_bytes)}</span>
              {#if chatModel}
                <Button variant="secondary" size="xs" class="shrink-0" onclick={() => loadAndChat(chatModel)}>
                  <MessageSquare />{t('hub.load_and_chat')}
                </Button>
              {/if}
              {@render iconAction(t('hub.queue_remove'), () => askRemove(job), X)}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </section>
{/if}
