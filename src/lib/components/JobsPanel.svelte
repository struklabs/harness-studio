<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  type Job = {
    id: number;
    label: string;
    priority: 'interactive' | 'agent' | 'background';
    status: 'queued' | 'loading' | 'running' | 'cancelling';
    memory_bytes: number | null;
    queue_wait_ms: number;
    load_ms: number | null;
  };
  let jobs = $state<Job[]>([]);
  let failed = $state(false);
  onMount(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function refresh() {
      try {
        const result = await invoke<Job[]>('get_jobs');
        if (active) { jobs = result; failed = false; }
      } catch { if (active) failed = true; }
      if (active) timer = setTimeout(refresh, 1500);
    }
    void refresh();
    return () => { active = false; clearTimeout(timer); };
  });
  async function cancel(id: number) {
    try { await invoke('cancel_job', { id }); }
    catch { failed = true; }
  }
</script>

<section class="rounded-md border border-border p-3" aria-label={t('jobs.title')}>
  <h2 class="mb-2 text-sm font-semibold">{t('jobs.title')}</h2>
  {#if failed}<p role="status" class="text-xs text-destructive">{t('jobs.failed')}</p>{/if}
  {#if jobs.length === 0}
    <p class="text-xs text-muted-foreground">{t('jobs.empty')}</p>
  {:else}
    <ul class="flex max-h-48 flex-col gap-2 overflow-y-auto">
      {#each jobs as job (job.id)}
        <li class="flex items-center justify-between gap-2 text-xs">
          <div class="min-w-0">
            <p class="truncate" title={job.label}>{job.label}</p>
            <p class="text-muted-foreground">{t(`jobs.status.${job.status}`)} · {t(`jobs.priority.${job.priority}`)}</p>
            <p class="text-muted-foreground">{job.memory_bytes !== null ? `${Math.ceil(job.memory_bytes / 1048576)} MiB` : t('jobs.unknown')} · {t('jobs.wait', { ms: job.queue_wait_ms })}</p>
          </div>
          <button class="rounded border border-border px-2 py-1 disabled:opacity-50" disabled={job.status === 'cancelling'} onclick={() => cancel(job.id)}>{t('jobs.cancel')}</button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
