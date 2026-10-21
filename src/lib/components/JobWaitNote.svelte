<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { formatBytes } from '#lib/format.js';
  import { queuedJobFor, waitLine, type Job } from './jobWait';

  // While a studio request runs: why the model's start still waits in the job queue.
  let { model }: { model: string } = $props();
  let job = $state<Job | undefined>(undefined);
  onMount(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function refresh() {
      try {
        const jobs = await invoke<Job[]>('get_jobs');
        if (active) job = queuedJobFor(jobs, model);
      } catch { /* the request's own error says what failed */ }
      if (active) timer = setTimeout(refresh, 1500);
    }
    void refresh();
    return () => { active = false; clearTimeout(timer); };
  });
  let line = $derived(job ? waitLine(job) : null);
</script>

{#if line}
  <p class="text-xs text-muted-foreground" role="status">
    {t(line.key, { need: line.need === null ? t('jobs.unknown') : formatBytes(line.need, 1), free: formatBytes(line.free, 1), capacity: formatBytes(line.capacity, 1) })}
  </p>
{/if}
