<script lang="ts">
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { connections } from '../connections.svelte.ts';
  import { HarnessError, jobs, type Job } from '../harness.ts';
  import { bytes, errorText } from '../text.ts';
  import HarnessPicker from './HarnessPicker.svelte';

  let selected = $state(connections.harness.find((c) => c.apiUrl)?.id ?? connections.harness[0]?.id ?? '');
  let list = $state<Job[] | null>(null);
  let error = $state<string | null>(null);
  const connection = $derived(connections.harness.find((c) => c.id === selected) ?? null);

  // The GPU queue changes quickly: refresh every 2 s while the screen is open.
  $effect(() => {
    const c = connection;
    list = null;
    error = null;
    if (!c) return;
    let stopped = false;
    const tick = async () => {
      try {
        const next = await jobs(c);
        if (!stopped) {
          list = next;
          error = null;
        }
      } catch (e) {
        if (!stopped) error = e instanceof HarnessError ? e.key : 'mobile.transport';
      }
    };
    void tick();
    const timer = setInterval(tick, 2000);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  });
</script>

<div class="space-y-3 p-4">
  <HarnessPicker bind:selected />
  {#if !connection}
    <p class="text-sm text-muted-foreground">{t('mobile.needs_harness')}</p>
  {:else if error}
    <p class="text-sm text-destructive" role="alert">{t('jobs.failed')} {errorText(error)}</p>
  {:else if list === null}
    <p class="text-sm text-muted-foreground">{t('mobile.loading')}</p>
  {:else if !list.length}
    <p class="text-sm text-muted-foreground" data-testid="jobs-empty">{t('jobs.empty')}</p>
  {:else}
    {#each list as job (job.id)}
      <div class="rounded-lg border border-border bg-card p-3" data-testid="job">
        <div class="flex items-center justify-between gap-2">
          <span class="truncate font-medium">{job.label}</span>
          <Badge variant={job.status === 'running' ? 'default' : 'secondary'}>{t(`jobs.status.${job.status}` as TKey)}</Badge>
        </div>
        <div class="mt-1 flex flex-wrap gap-x-3 text-xs text-muted-foreground">
          <span>{t(`jobs.priority.${job.priority}` as TKey)}</span>
          <span>{t('jobs.wait', { ms: job.queue_wait_ms })}</span>
          {#if job.load_ms !== null}<span>{t('mobile.where.load', { ms: job.load_ms })}</span>{/if}
          <span>{job.memory_bytes === null ? t('jobs.unknown') : bytes(job.memory_bytes)}</span>
        </div>
      </div>
    {/each}
  {/if}
</div>
