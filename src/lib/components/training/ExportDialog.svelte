<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { training } from '#lib/stores/training.svelte.js';
  import type { TrainingJob, TrainingQuant } from '#lib/training-types.js';
  import { formatBytes } from '#lib/format.js';
  import { estimateGgufBytes } from './trainingFormat';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  interface Props {
    job: TrainingJob | null;
    onClose: () => void;
  }
  let { job, onClose }: Props = $props();

  let name = $state('');
  let gguf = $state(false);
  let quant = $state<TrainingQuant>('Q4_K_M');
  let adapterOnly = $state(false);
  let busy = $state(false);

  // A newly opened dialog starts from the job's name.
  $effect(() => {
    if (job) {
      name = job.config.name;
      gguf = false;
      quant = 'Q4_K_M';
      adapterOnly = false;
    }
  });

  async function run() {
    if (!job) return;
    busy = true;
    const done = await training.exportJob(job.id, { name: name.trim() || null, gguf, quant, adapter_only: adapterOnly });
    busy = false;
    if (done) onClose();
  }
</script>

<Dialog.Root open={!!job} onOpenChange={(v) => { if (!v && !busy) onClose(); }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('training.export_title')}</Dialog.Title>
      <Dialog.Description>{t('training.export_description')}</Dialog.Description>
    </Dialog.Header>
    <div class="flex flex-col gap-3 text-sm">
      <label class="flex flex-col gap-1">{t('training.export_name')}<Input bind:value={name} maxlength={80} disabled={busy} /></label>
      <label class="flex items-start gap-2">
        <input type="checkbox" class="mt-1" bind:checked={adapterOnly} disabled={busy} />
        <span>{t('training.export_adapter')}<span class="block text-xs text-muted-foreground">{t('training.export_adapter_hint')}</span></span>
      </label>
      <label class="flex items-start gap-2">
        <input type="checkbox" class="mt-1" bind:checked={gguf} disabled={busy} />
        <span>
          {t('training.export_gguf')}
          <span class="block text-xs text-muted-foreground">{t('training.export_gguf_hint')}</span>
        </span>
      </label>
      {#if gguf && !adapterOnly}
        <label class="flex flex-col gap-1">{t('training.export_quant')}
          <select class="rounded-md border bg-background p-2" bind:value={quant} disabled={busy}>
            {#each ['Q4_K_M', 'Q5_K_M', 'Q8_0', 'F16'] as q}<option value={q}>{q}</option>{/each}
          </select>
        </label>
        <p class="text-xs text-muted-foreground">{t('training.export_size', { size: formatBytes(estimateGgufBytes(job?.estimate.params ?? 0, quant), 1) })}</p>
      {/if}
      {#if training.actionError}<p class="text-xs text-destructive" role="alert">{training.actionError}</p>{/if}
    </div>
    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={onClose}>{t('training.cancel')}</Button>
      <Button disabled={busy || !name.trim()} onclick={run}>{t('training.export_start')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
