<script lang="ts">
  import { onDestroy } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { training, isTrainingActive } from '#lib/stores/training.svelte.js';
  import type { TrainingJob } from '#lib/training-types.js';

  let { job }: { job: TrainingJob } = $props();

  let open = $state(false);
  let text = $state('');
  let failure = $state<string | null>(null);
  let timer: ReturnType<typeof setInterval> | null = null;
  let pre = $state<HTMLPreElement | null>(null);

  async function load(id: number) {
    try {
      const next = await training.readLog(id);
      failure = null;
      if (next !== text) {
        const pinned = !pre || pre.scrollHeight - pre.scrollTop - pre.clientHeight < 24;
        text = next;
        if (pinned) queueMicrotask(() => pre && (pre.scrollTop = pre.scrollHeight));
      }
    } catch (e) {
      failure = errorText(e);
    }
  }

  // Read while open, again every 2 s while the job is active, and when another job is picked.
  let jobId = $derived(job.id);
  let live = $derived(isTrainingActive(job));
  $effect(() => {
    const id = jobId;
    const active = live;
    if (timer) clearInterval(timer);
    timer = null;
    if (!open) return;
    text = '';
    void load(id);
    if (active) timer = setInterval(() => void load(id), 2000);
  });

  onDestroy(() => timer && clearInterval(timer));
</script>

<details class="rounded-lg border border-border p-3" ontoggle={(e) => (open = (e.currentTarget as HTMLDetailsElement).open)}>
  <summary class="cursor-pointer text-sm font-semibold">{t('training.log_title')}</summary>
  {#if open}
    {#if failure}
      <p class="mt-2 text-xs text-destructive" role="alert">{failure}</p>
    {/if}
    <pre bind:this={pre} class="mt-2 max-h-64 overflow-auto rounded-md bg-muted/50 p-2 font-mono text-xs whitespace-pre-wrap" aria-label={t('training.log_title')}>{text || t('training.log_empty')}</pre>
  {/if}
</details>
