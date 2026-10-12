<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';

  // Settings > Runtime: what the official runtime really runs (managed / custom / a system
  // llama-server found on PATH or WinGet), whether that is a fallback for the configured
  // mode, and a staged managed build waiting to be activated.
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import type { AppSettings } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { runtimesStore, runtimeBuildLabel } from '$lib/stores/runtimes.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { TriangleAlert } from '@lucide/svelte';
  import type { SettingsDraft } from './form.svelte';
  import SettingRow from './SettingRow.svelte';

  interface Props {
    /** Saved settings (the state is re-read when the saved runtime settings change). */
    settings: AppSettings;
    form: SettingsDraft;
    onSettingsUpdated: (updated: AppSettings) => void;
  }

  let { settings, form, onSettingsUpdated }: Props = $props();

  $effect(() => {
    void settings.runtime_mode;
    void settings.custom_binary.binary_path;
    void settings.managed_runtime.installed_version;
    void runtimesStore.load(true);
  });

  let view = $derived(runtimesStore.get('official'));
  let off = $derived(view?.official ?? null);
  let stagedLabel = $derived(off?.staged?.build != null ? `b${off.staged.build}` : (off?.staged?.version ?? ''));
  let activating = $state(false);

  const SOURCE_KEY = {
    managed: 'rt.source_managed',
    custom: 'rt.source_custom',
    system: 'rt.source_system',
  } as const;

  async function activateStaged() {
    activating = true;
    try {
      const updated = await invoke<AppSettings>('activate_staged_runtime');
      onSettingsUpdated(updated);
      toast.success(t('rt.activated', { version: updated.managed_runtime.installed_version ?? stagedLabel }));
      await runtimesStore.load(true);
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      activating = false;
    }
  }
</script>

<SettingRow label={t('rt.current_title')} hint={t('rt.current_hint')}>
  {#snippet extra()}
    {#if view}
      <div class="flex flex-col gap-1.5 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-xs" role="status">
        {#if view.available && view.binary_path}
          <span class="flex flex-wrap items-center gap-1.5">
            <AdvancedOnly>{#if off?.source}<Badge variant="secondary" class="text-xs">{t(SOURCE_KEY[off.source])}</Badge>{/if}</AdvancedOnly>
            <span class="font-num">{runtimeBuildLabel(view)}</span>
          </span>
          <AdvancedOnly><span class="font-mono text-xs break-all text-muted-foreground">{view.binary_path}</span></AdvancedOnly>
        {:else}
          <span class="text-destructive">{view.error ? localizeError(view.error) : t('rt.none_found')}</span>
        {/if}
        {#if off?.fallback}
          <span class="flex items-start gap-1.5 text-warning">
            <TriangleAlert class="mt-0.5 size-3.5 shrink-0" />
            {off.configured_mode === 'managed' ? t('rt.fallback_managed') : t('rt.fallback_custom')}
          </span>
        {/if}
        {#if off?.staged}
          <span class="flex flex-wrap items-center gap-2">
            <span>{t('rt.staged', { version: stagedLabel })}</span>
            <Button size="xs" disabled={activating} onclick={activateStaged}>
              {activating ? t('rt.activating') : t('rt.activate_staged', { version: stagedLabel })}
            </Button>
          </span>
        {/if}
        {#if form.get('runtime_mode') === 'Custom' && (off?.managed_installed || off?.staged)}
          <span class="flex flex-wrap items-center gap-2 text-muted-foreground">
            <Button size="xs" variant="outline" onclick={() => form.set('runtime_mode', 'Managed')}>{t('rt.switch_managed')}</Button>
            {t('rt.switch_managed_hint')}
          </span>
        {/if}
      </div>
    {/if}
  {/snippet}
</SettingRow>
