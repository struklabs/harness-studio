<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { training } from '#lib/stores/training.svelte.js';
  import type { TrainingJob } from '#lib/training-types.js';
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
  let busy = $state(false);

  // A newly opened dialog starts from the job's name.
  $effect(() => {
    if (job) {
      name = job.config.name;
      gguf = false;
    }
  });

  async function run() {
    if (!job) return;
    busy = true;
    const done = await training.exportJob(job.id, name.trim() || null, gguf);
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
        <input type="checkbox" class="mt-1" bind:checked={gguf} disabled={busy} />
        <span>
          {t('training.export_gguf')}
          <span class="block text-xs text-muted-foreground">{t('training.export_gguf_hint')}</span>
        </span>
      </label>
      {#if training.actionError}<p class="text-xs text-destructive" role="alert">{training.actionError}</p>{/if}
    </div>
    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={onClose}>{t('training.cancel')}</Button>
      <Button disabled={busy || !name.trim()} onclick={run}>{t('training.export_start')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
