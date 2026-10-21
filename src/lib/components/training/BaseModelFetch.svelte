<script lang="ts">
  // Downloads a safetensors Hub repo (weights, config, tokenizer) into the library as a training
  // base model, with its progress. The wizard's model list shows it when the download is done.
  import { untrack } from 'svelte';
  import { Download } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { errorText } from '#lib/errors.js';
  import { formatBytes } from '#lib/format.js';
  import { downloads } from '#lib/stores/downloads.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { progressPercent, statusError, statusKey } from '#lib/components/hub/downloadQueue.svelte.js';

  interface Props {
    /** Called with the repo id when its download completed and the library was refreshed. */
    onReady?: (repoId: string) => void;
  }
  let { onReady }: Props = $props();

  let repo = $state('');
  let jobId = $state<number | null>(null);
  let startedRepo = $state('');
  let starting = $state(false);
  let error = $state<string | null>(null);
  let reported = false;

  let job = $derived(jobId === null ? null : (downloads.jobs.find((j) => j.id === jobId) ?? null));
  let status = $derived(job ? statusKey(job.status) : null);
  let failure = $derived(job ? statusError(job.status) : null);
  let valid = $derived(/^[\w.-]+\/[\w.-]+$/.test(repo.trim()));

  async function start() {
    starting = true;
    error = null;
    reported = false;
    try {
      const id = repo.trim();
      jobId = await invoke<number>('training_download_base', { repoId: id });
      startedRepo = id;
      await downloads.refresh();
    } catch (e) {
      error = errorText(e);
    } finally {
      starting = false;
    }
  }

  $effect(() => {
    if (status !== 'Completed' || reported) return;
    reported = true;
    untrack(() => {
      void library.refresh().then(() => onReady?.(startedRepo));
    });
  });
</script>

<div class="flex flex-col gap-1 text-sm">
  <label for="training-fetch-repo" class="font-medium">{t('training.base_download_title')}</label>
  <div class="flex gap-2">
    <Input id="training-fetch-repo" bind:value={repo} placeholder={t('training.base_download_placeholder')} onkeydown={(e) => e.key === 'Enter' && valid && !starting && start()} />
    <Button type="button" variant="outline" disabled={!valid || starting || status === 'Downloading' || status === 'Queued'} onclick={start}>
      <Download class="size-4" aria-hidden="true" />{t('training.base_download_button')}
    </Button>
  </div>
  <p class="text-xs text-muted-foreground">{t('training.base_download_hint')}</p>
  {#if job && status !== 'Completed' && !failure}
    <Progress value={progressPercent(job)} />
    <p class="text-xs text-muted-foreground">{formatBytes(job.downloaded_bytes, 1)} / {formatBytes(job.total_bytes, 1)}</p>
  {:else if status === 'Completed'}
    <p class="text-xs text-emerald-700 dark:text-emerald-400" role="status">{t('training.base_download_done')}</p>
  {/if}
  {#if failure}<p class="text-xs text-destructive" role="alert">{failure}</p>{/if}
  {#if error}<p class="text-xs text-destructive" role="alert">{error}</p>{/if}
</div>
