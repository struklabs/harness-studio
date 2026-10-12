<script lang="ts">
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

  // Manual load settings were used but not saved: asked once when the user leaves the screen,
  // switches model, stops the server or closes the app (see `unsavedLoad`).

  let busy = $state(false);
  let never = $state(false);
  let entry = $derived(unsavedLoad.asking);

  $effect(() => {
    if (entry) never = false;
  });

  async function rememberChoice() {
    if (!never || !app.settings) return;
    unsavedLoad.disabled = true;
    try {
      const next = { ...$state.snapshot(app.settings), no_save_load_prompt: true } as AppSettings;
      app.setSettings((await invoke<AppSettings>('save_app_settings', { settings: next })) ?? next);
    } catch {
      // The choice still holds for this session.
    }
  }

  async function answer(save: boolean) {
    if (busy || !entry) return;
    busy = true;
    try {
      if (save) {
        try {
          await entry.save();
          toast.success(t('savedefault.saved', { alias: entry.alias }));
        } catch (e) {
          toast.error(t('savedefault.failed', { error: localizeError(e) }));
        }
      }
      await rememberChoice();
    } finally {
      busy = false;
      unsavedLoad.done();
    }
  }
</script>

<Dialog.Root
  open={!!entry}
  onOpenChange={(next) => {
    if (!next && entry) answer(false);
  }}
>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('savedefault.title')}</Dialog.Title>
      <Dialog.Description>{t('savedefault.body', { alias: entry?.alias ?? '' })}</Dialog.Description>
    </Dialog.Header>
    <div class="flex items-center gap-2">
      <Checkbox id="savedefault-never" checked={never} onCheckedChange={(v) => (never = !!v)} disabled={busy} />
      <Label for="savedefault-never" class="text-sm font-normal">{t('savedefault.never')}</Label>
    </div>
    <Dialog.Footer class="gap-2">
      <Button variant="outline" onclick={() => answer(false)} disabled={busy}>{t('savedefault.skip')}</Button>
      <Button onclick={() => answer(true)} disabled={busy}>{t('savedefault.save')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
