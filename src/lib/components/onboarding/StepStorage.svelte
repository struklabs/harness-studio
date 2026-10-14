<script lang="ts">
  import { invoke } from '#lib/ipc.js';
  import { FolderOpen, Lightbulb } from '@lucide/svelte';
  import type { AppSettings } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  interface Props {
    modelsDir: string;
    /** Called with the settings the backend returns after the folder changed. */
    onChanged: (updated: AppSettings) => void;
  }

  let { modelsDir, onChanged }: Props = $props();

  let busy = $state(false);
  let message = $state<{ ok: boolean; text: string } | null>(null);

  async function pickFolder() {
    if (busy) return;
    busy = true;
    message = null;
    try {
      const picked: string | null = await invoke('pick_directory_dialog');
      if (picked) {
        const updated: AppSettings = await invoke('set_primary_storage_directory', { dirPath: picked });
        onChanged(updated);
        message = { ok: true, text: t('onboarding.wizard_storage_updated') };
      }
    } catch (e) {
      message = { ok: false, text: t('onboarding.wizard_folder_failed', { error: String(e) }) };
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-4">
  <div class="flex flex-col gap-2 rounded-lg border border-border bg-card p-3">
    <Label for="models-dir-display" class="text-xs text-muted-foreground">{t('onboarding.wizard_models_dir_label')}</Label>
    <div class="flex min-w-0 items-center gap-2">
      <Input id="models-dir-display" readonly class="h-8 min-w-0 flex-1 font-mono text-xs" value={modelsDir} title={modelsDir} />
      <Button variant="outline" size="sm" onclick={pickFolder} disabled={busy}>
        <FolderOpen /> {t('onboarding.wizard_change_dir')}
      </Button>
    </div>
    {#if message}
      <p class="text-xs break-words {message.ok ? 'text-success' : 'text-destructive'}" role={message.ok ? 'status' : 'alert'}>{message.text}</p>
    {/if}
  </div>

  <p class="flex items-start gap-2 text-xs text-muted-foreground">
    <Lightbulb class="mt-0.5 size-3.5 shrink-0" /> <span>{t('onboarding.wizard_dir_tip')}</span>
  </p>
</div>
