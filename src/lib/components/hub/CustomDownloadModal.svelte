<script lang="ts">
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  import { toast } from 'svelte-sonner';
  import { t } from '$lib/i18n.svelte';
  import { downloads } from '$lib/stores/downloads.svelte';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import { AlertCircle, LoaderCircle } from '@lucide/svelte';

  interface Props {
    open: boolean;
  }

  let { open = $bindable() }: Props = $props();

  const uid = $props.id();

  let url = $state('');
  let filename = $state('');
  let error = $state<string | null>(null);
  let starting = $state(false);

  $effect(() => {
    if (open) error = null;
  });

  async function submit(e?: Event) {
    e?.preventDefault();
    if (!url.trim() || starting) return;
    starting = true;
    error = null;
    try {
      await downloads.enqueueUrl(url.trim(), filename.trim() || null);
      toast.success(t('hub.announce_queued', { name: filename.trim() || url.trim() }));
      open = false;
      url = '';
      filename = '';
    } catch (err) {
      error = t('hub.download_start_failed', { error: String(err) });
    } finally {
      starting = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('hub.modal_title')}</Dialog.Title>
      <Dialog.Description>{t('hub.modal_desc')}</Dialog.Description>
    </Dialog.Header>

    <form class="flex flex-col gap-3" onsubmit={submit}>
      <div class="flex flex-col gap-1.5">
        <Label for="{uid}-url" class="text-xs">{t('hub.url_label')}</Label>
        <FieldHelp text={t('help.download_url')}><Input
          id="{uid}-url"
          type="url"
          class="font-mono text-xs"
          placeholder="https://huggingface.co/.../model-q4_k_m.gguf"
          bind:value={url}
        /></FieldHelp>
      </div>

      <div class="flex flex-col gap-1.5">
        <Label for="{uid}-name" class="text-xs">{t('hub.filename_label')}</Label>
        <FieldHelp text={t('help.filename')}><Input id="{uid}-name" class="font-mono text-xs" placeholder={t('hub.filename_placeholder')} bind:value={filename} /></FieldHelp>
      </div>

      {#if error}
        <div class="flex gap-2 rounded-lg border border-destructive/30 bg-destructive/10 p-2.5 text-xs text-destructive" role="alert">
          <AlertCircle class="mt-px size-3.5 shrink-0" /><span class="min-w-0 break-words">{error}</span>
        </div>
      {/if}

      <Dialog.Footer>
        <Button type="button" variant="ghost" onclick={() => (open = false)}>{t('hub.cancel')}</Button>
        <Button type="submit" disabled={!url.trim() || starting}>
          {#if starting}<LoaderCircle class="animate-spin" />{/if}
          {starting ? t('hub.adding_to_queue') : t('hub.add_to_queue_button')}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
