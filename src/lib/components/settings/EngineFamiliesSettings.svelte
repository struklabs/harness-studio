<script lang="ts">
  // Settings > Installed Runtime > Other Engines: the engine families a model can need besides
  // llama.cpp (speech, images, voices, ONNX, Lemonade, mistral.rs, the Python engine), each with its install state
  // and a one-click install button. The same installer as the "needs an engine" hints.
  import { onMount } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { ENGINES, engineDescKey, engineLabel, onnxUnsupportedText } from '#lib/engines.js';
  import { errorText } from '#lib/errors.js';
  import { isLinux, isMac } from '#lib/platform.js';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { formatBytes } from '#lib/format.js';
  import { LoaderCircle, Download, Square, Trash2 } from '@lucide/svelte';
  import SettingGroup from './SettingGroup.svelte';
  import CrispasrHelpers from './CrispasrHelpers.svelte';
  import SwitchRow from './SwitchRow.svelte';
  import { invoke } from '#lib/ipc.js';
  import type { AppSettings } from '#lib/types.js';

  let { settings, onSettingsUpdated }: {
    settings: AppSettings;
    onSettingsUpdated: (updated: AppSettings) => void;
  } = $props();
  let confirmAudio = $state(false);
  let saving = $state(false);
  let saveError = $state<string | null>(null);

  async function saveAudio(enabled: boolean) {
    saving = true;
    saveError = null;
    try {
      const next = await invoke<AppSettings>('set_audio_cpp_enabled', { enabled, acknowledged: enabled && confirmAudio });
      onSettingsUpdated(next);
      confirmAudio = false;
      await engineInstall.refreshAvailability();
    } catch (e) {
      saveError = errorText(e);
    } finally {
      saving = false;
    }
  }

  onMount(() => void engineInstall.refresh());

  // ONNX Runtime stays listed where it has no build at all, and a Python engine whose wheels need
  // a newer OS (MLX on macOS 13), so the page says why instead of hiding it.
  let engines = $derived(
    ENGINES.filter((e) => engineInstall.offered(e.family) || engineInstall.unsupported(e.family) || engineInstall.pyRefusal(e.family)),
  );
</script>

<SettingGroup title={t('eng.group_title')} hint={t('eng.group_desc')}>
  <SwitchRow id="audio-cpp-enabled" label={t('audio_cpp.experimental')}
    note={t('audio_cpp.switch_hint')} checked={settings.audio_cpp_enabled ?? false}
    disabled={saving} onchange={(on) => { if (on) confirmAudio = true; else void saveAudio(false); }} />
  {#if confirmAudio}
    <div class="flex flex-col gap-2 px-4 py-3" role="alert">
      <h3 class="text-sm font-medium">{t('audio_cpp.confirm')}</h3>
      <p class="text-xs text-warning">{t('audio_cpp.risk')}</p>
      <div class="flex gap-2">
        <Button size="sm" disabled={saving} onclick={() => saveAudio(true)}>{t('audio_cpp.enable')}</Button>
        <Button size="sm" variant="outline" disabled={saving} onclick={() => (confirmAudio = false)}>{t('settings.cancel')}</Button>
      </div>
    </div>
  {/if}
  {#if saveError}<p class="px-4 text-xs text-destructive" role="alert">{saveError}</p>{/if}
  <div class="flex flex-col divide-y divide-border">
    {#each engines as e (e.family)}
      {@const st = engineInstall.state(e.family)}
      {@const installed = engineInstall.installed(e.family)}
      {@const unsupported = engineInstall.unsupported(e.family)}
      {@const refusal = engineInstall.pyRefusal(e.family)}
      <div class="flex flex-col gap-1 px-4 py-3">
        <div class="flex min-w-0 items-center justify-between gap-3">
          <div class="flex min-w-0 flex-col gap-0.5">
            <span class="flex min-w-0 items-center gap-1.5 text-sm font-medium">
              <span class="truncate font-mono">{engineLabel(e.family)}</span>
              <Badge variant={installed ? 'secondary' : 'outline'} class="text-xs">
                {installed ? t('eng.state_installed') : unsupported || refusal ? t('ort.state_unsupported') : t('eng.state_missing')}
              </Badge>
            </span>
            <span class="text-xs text-muted-foreground">{t(engineDescKey(e, isMac, isLinux, engineInstall.pyEnv(e.family)?.name), { size: formatBytes(engineInstall.pyEnv(e.family)?.estimated_size_bytes ?? 0, 1) })}</span>
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            {#if unsupported || refusal}
              <!-- nothing to install: the line below says why -->
            {:else if e.method === 'pyenv' && st.busy}
              <Button size="sm" variant="outline" onclick={() => engineInstall.cancel(e.family)}>
                <Square />{t('eng.cancel')}
              </Button>
              <span class="flex items-center gap-1 text-xs text-muted-foreground"><LoaderCircle class="size-3.5 animate-spin" />{t(st.pct === null ? 'eng.installing' : 'eng.installing_pct', { runtime: engineLabel(e.family), pct: st.pct ?? 0 })}</span>
            {:else if e.method === 'pyenv' && engineInstall.pyEnv(e.family)?.installed}
              {#if !installed}
                <Button size="sm" disabled={engineInstall.anyBusy} onclick={() => engineInstall.install(e.family)}><Download />{t('eng.install', { runtime: engineLabel(e.family) })}</Button>
              {/if}
              <Button size="sm" variant="outline" disabled={engineInstall.anyBusy} onclick={() => engineInstall.remove(e.family)}>
                <Trash2 />{t('eng.remove', { size: formatBytes(engineInstall.pyEnv(e.family)?.size_bytes ?? 0, 1) })}
              </Button>
            {:else if !installed && engineInstall.canInstall(e.family)}
              <Button size="sm" disabled={engineInstall.anyBusy} onclick={() => engineInstall.install(e.family)}>
                {#if st.busy}<LoaderCircle class="animate-spin" />{:else}<Download />{/if}
                {st.busy ? t(st.pct === null ? 'eng.installing' : 'eng.installing_pct', { runtime: engineLabel(e.family), pct: st.pct ?? 0 }) : t('eng.install', { runtime: engineLabel(e.family) })}
              </Button>
            {:else if installed && e.family === 'crispasr'}
              <Button variant="outline" size="sm" onclick={() => security.openSettings('runtime-registry')}>{t('eng.manage_crispasr')}</Button>
            {:else if !installed && uiLevel.advanced}
              <Button variant="outline" size="sm" onclick={() => security.openSettings('runtime-registry')}>{t('eng.open_registry')}</Button>
            {/if}
          </div>
        </div>
        {#if unsupported}
          {@const msg = onnxUnsupportedText(unsupported)}
          <span class="text-xs text-warning" role="status">{t(msg.key, msg.params)}</span>
        {:else if refusal}
          <span class="text-xs text-warning" role="status">{errorText(refusal)}</span>
        {:else if !installed && !engineInstall.canInstall(e.family)}
          <span class="text-xs text-muted-foreground">{t('eng.manual', { runtime: engineLabel(e.family) })}</span>
        {/if}
        {#if st.problem === 'unverified'}
          <span class="text-xs text-warning" role="alert">{t('eng.unverified', { runtime: engineLabel(e.family) })}</span>
        {:else if st.problem === 'failed' && st.error}
          <span class="text-xs text-destructive [overflow-wrap:anywhere]" role="alert">{t('eng.failed', { runtime: engineLabel(e.family), error: st.error })}</span>
        {/if}
      </div>
      {#if e.family === 'crispasr' && installed}
        <CrispasrHelpers />
      {/if}
    {/each}
  </div>
</SettingGroup>
