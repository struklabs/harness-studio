<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { toast } from 'svelte-sonner';
  import { invoke } from '$lib/ipc';
  import type { AppSettings } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { app } from '$lib/stores/app.svelte';
  import { unsavedLoad } from '$lib/stores/unsavedLoad.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import { Label } from '$lib/components/ui/label';
  import * as Dialog from '$lib/components/ui/dialog';

  // Closing the window while `startup.close_action` is "ask" (the default): hide to the tray or
  // quit, like Ollama / LM Studio. The backend holds the window open until this is answered.

  let open = $state(false);
  let remember = $state(true);
  let busy = $state(false);

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen('close-requested', async () => {
      // Manual load settings that were used but never saved are asked about first.
      await unsavedLoad.flush();
      remember = true;
      busy = false;
      open = true;
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function answer(action: 'tray' | 'quit' | 'cancel') {
    if (busy) return;
    busy = true;
    try {
      const updated = await invoke<AppSettings>('resolve_window_close', {
        action,
        remember: remember && action !== 'cancel',
      });
      app.setSettings(updated);
      open = false;
    } catch (e) {
      toast.error(t('close.failed', { error: localizeError(e) }));
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root
  {open}
  onOpenChange={(next) => {
    if (!next && open) answer('cancel');
  }}
>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('close.title')}</Dialog.Title>
      <Dialog.Description>{t('close.body')}</Dialog.Description>
    </Dialog.Header>
    {#if app.runningCount > 0}
      <p class="text-xs text-muted-foreground">{t('close.running', { count: app.runningCount })}</p>
    {/if}
    <div class="flex flex-col gap-1">
      <div class="flex items-center gap-2">
        <Checkbox id="close-remember" checked={remember} onCheckedChange={(v) => (remember = !!v)} disabled={busy} />
        <Label for="close-remember" class="text-sm font-normal">{t('close.remember')}</Label>
      </div>
      <p class="pl-6 text-xs text-muted-foreground">{t('close.remember_hint')}</p>
    </div>
    <Dialog.Footer class="gap-2">
      <Button variant="outline" onclick={() => answer('cancel')} disabled={busy}>{t('close.cancel')}</Button>
      <Button variant="secondary" onclick={() => answer('quit')} disabled={busy}>{t('close.quit')}</Button>
      <Button onclick={() => answer('tray')} disabled={busy}>{t('close.tray')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
