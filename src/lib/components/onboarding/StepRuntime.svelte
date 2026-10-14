<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { rawError } from '#lib/errors.js';
  import { Check, Package, Cpu } from '@lucide/svelte';
  import type { BackendStatus, SystemHardwareInfo, RuntimeDetectionInfo, RuntimeDownloadProgress, ReleaseAssetInfo } from '#lib/types.js';
  import { BACKEND_TOKEN, backendFallbackText, loadBackendFallback, missingBackendLibraries, backendName, normBackend, saveBackendFallback, type BackendFallbackNote } from '#lib/runtimeBackend.js';
  import { formatBytes } from '#lib/format.js';
  import { localizeError } from '#lib/network.js';
  import { downloadRelease } from '#lib/stores/security.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { cn } from '#lib/utils.js';

  interface Props {
    /** True while a download is running; the wizard blocks navigation meanwhile. */
    busy?: boolean;
    /** Detected hardware (step 2); its recommended backend names the build that gets downloaded. */
    hardware?: SystemHardwareInfo | null;
  }

  let { busy = $bindable(false), hardware = null }: Props = $props();

  const backendLabel = $derived(backendName(BACKEND_TOKEN[hardware?.recommended_backend ?? 'Cpu']));

  let runtimeInfo = $state<RuntimeDetectionInfo | null>(null);
  let isChecking = $state(true);
  let usingSystem = $state(false);
  let installProgress = $state<RuntimeDownloadProgress | null>(null);
  let message = $state<{ tone: 'info' | 'ok' | 'error'; text: string } | null>(null);
  let fallbackNote = $state<BackendFallbackNote | null>(null);
  let pollInterval: ReturnType<typeof setInterval> | null = null;
  let destroyed = false;

  const pct = $derived(
    installProgress && installProgress.total_bytes > 0
      ? Math.min(100, (installProgress.downloaded_bytes / installProgress.total_bytes) * 100)
      : 0,
  );

  function stopPolling() {
    if (pollInterval) clearInterval(pollInterval);
    pollInterval = null;
  }

  async function refreshInfo() {
    const info = await invoke<RuntimeDetectionInfo>('get_runtime_detection_info');
    if (destroyed) return;
    runtimeInfo = info;
    usingSystem = info.systemDetected && info.activeMode === 'Custom';
  }

  onMount(async () => {
    try {
      await refreshInfo();
      const saved = loadBackendFallback();
      if (saved && saved.version === runtimeInfo?.managedVersion) fallbackNote = saved;
    } catch (e) {
      if (!destroyed) message = { tone: 'error', text: t('onboarding.wizard_detect_failed', { error: String(e) }) };
    } finally {
      if (!destroyed) isChecking = false;
    }
  });

  onDestroy(() => {
    destroyed = true;
    stopPolling();
  });

  async function useSystemRuntime() {
    if (busy) return;
    busy = true;
    message = null;
    try {
      // The backend saves the new runtime mode itself; the wizard re-reads settings when it finishes.
      await invoke('auto_configure_system_runtime');
      await refreshInfo();
      message = { tone: 'ok', text: t('onboarding.wizard_using_system_banner') };
    } catch (e) {
      message = { tone: 'error', text: t('onboarding.wizard_use_system_failed', { error: String(e) }) };
    } finally {
      busy = false;
    }
  }

  async function installManaged() {
    if (busy) return;
    busy = true;
    fallbackNote = null;
    message = { tone: 'info', text: t('onboarding.wizard_runtime_status_checking') };
    try {
      // The recommended build first (ROCm/HIP or SYCL on some GPUs). Those depend on the local
      // driver stack; when the staged build sees no device, the portable Vulkan build is used.
      let releaseInfo: ReleaseAssetInfo = await invoke('check_runtime_update', { backend: null });
      // The recommendation, to tell later (Settings > Runtime) why another build got installed.
      const recommended = await invoke<BackendStatus>('get_backend_status')
        .then((s) => BACKEND_TOKEN[s.advice.recommended])
        .catch(() => BACKEND_TOKEN[hardware?.recommended_backend ?? 'Cpu']);
      let fallbackReason: BackendFallbackNote['reason'] | null =
        recommended && normBackend(releaseInfo.backend) !== recommended ? 'unavailable' : null;
      let missingLibraries: string | undefined;
      stopPolling();
      pollInterval = setInterval(async () => {
        try {
          const p = await invoke<RuntimeDownloadProgress | null>('get_runtime_download_progress');
          if (!destroyed) installProgress = p;
        } catch {
          /* progress is best-effort */
        }
      }, 500);

      let staged: string | null;
      const assignNote = () => {
        const installed = normBackend(releaseInfo.backend);
        fallbackNote = recommended && fallbackReason && installed
          ? { recommended, installed, version: releaseInfo.tag_name, reason: fallbackReason, missingLibraries }
          : null;
      };

      try {
        if (fallbackReason) assignNote();
        message = { tone: 'info', text: fallbackReason
          ? t('onboarding.wizard_fallback_downloading_unavailable', { recommended: backendName(recommended), installed: backendName(releaseInfo.backend) })
          : t('onboarding.wizard_downloading_version', { version: releaseInfo.tag_name }) };
        staged = await downloadRelease(releaseInfo);
      } catch (e) {
        if (!rawError(e).includes('runtime.backend_no_device') || releaseInfo.backend === 'vulkan') throw e;
        const attemptedBackend = normBackend(releaseInfo.backend);
        missingLibraries = missingBackendLibraries(e);
        releaseInfo = await invoke('check_runtime_update', { backend: 'vulkan' });
        fallbackReason = 'no_device';
        assignNote();
        message = { tone: 'info', text: t('onboarding.wizard_fallback_downloading_no_device', { recommended: backendName(attemptedBackend), installed: backendName(releaseInfo.backend) }) };
        staged = await downloadRelease(releaseInfo);
      }
      if (staged === null) {
        message = { tone: 'error', text: t('sec.integrity_cancelled') };
        return;
      }

      message = { tone: 'info', text: t('onboarding.wizard_activating') };
      await invoke('activate_managed_runtime', { version: releaseInfo.tag_name });
      saveBackendFallback(fallbackNote);
      await refreshInfo();
      message = { tone: 'ok', text: t('onboarding.wizard_runtime_ready') };
    } catch (e) {
      message = { tone: 'error', text: t('onboarding.wizard_install_failed', { error: localizeError(e) }) };
    } finally {
      stopPolling();
      installProgress = null;
      busy = false;
    }
  }

  const tones = { info: 'text-muted-foreground', ok: 'text-success', error: 'text-destructive' } as const;
</script>

<div class="flex flex-col gap-3">
  {#if isChecking}
    <div class="flex items-center gap-2.5 rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground" role="status">
      <span class="size-4 animate-spin rounded-full border-2 border-border border-t-primary" aria-hidden="true"></span>
      {t('onboarding.wizard_runtime_status_checking')}
    </div>
  {:else}
    {#if runtimeInfo?.systemDetected}
      <div class={cn('flex flex-col gap-2.5 rounded-lg border bg-card p-3', usingSystem ? 'border-primary/40' : 'border-border')}>
        <div class="flex items-start gap-2.5">
          <Cpu class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <div class="flex min-w-0 flex-1 flex-col gap-0.5">
            <span class="text-sm font-medium">{t('onboarding.wizard_sys_found_title')}</span>
            <span class="text-xs text-muted-foreground">{t('onboarding.wizard_sys_found_desc')}</span>
          </div>
          {#if usingSystem}
            <Badge variant="outline" class="shrink-0 text-success"><Check /> {t('onboarding.wizard_system_active')}</Badge>
          {/if}
        </div>
        <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
          <dt class="text-muted-foreground">{t('onboarding.wizard_sys_path')}</dt>
          <dd class="truncate font-mono" title={runtimeInfo.systemPath ?? ''}>{runtimeInfo.systemPath}</dd>
          {#if runtimeInfo.systemVersion}
            <dt class="text-muted-foreground">{t('onboarding.wizard_sys_version')}</dt>
            <dd class="font-num">{runtimeInfo.systemVersion}</dd>
          {/if}
        </dl>
        {#if !usingSystem}
          <div>
            <Button variant="secondary" size="sm" onclick={useSystemRuntime} disabled={busy}>{t('onboarding.wizard_btn_use_system')}</Button>
          </div>
        {/if}
      </div>
    {/if}

    <div class={cn('flex flex-col gap-2.5 rounded-lg border bg-card p-3', runtimeInfo?.managedInstalled && !usingSystem ? 'border-primary/40' : 'border-border')}>
      <div class="flex items-start gap-2.5">
        {#if runtimeInfo?.managedInstalled}
          <Check class="mt-0.5 size-4 shrink-0 text-success" />
        {:else}
          <Package class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        {/if}
        <div class="flex min-w-0 flex-1 flex-col gap-0.5">
          <span class="text-sm font-medium">
            {runtimeInfo?.managedInstalled ? t('onboarding.wizard_managed_installed_title') : t('onboarding.wizard_managed_title')}
          </span>
          <span class="text-xs text-muted-foreground">{t('onboarding.wizard_install_managed_desc', { backend: backendLabel })}</span>
        </div>
      </div>

      {#if runtimeInfo?.managedInstalled}
        <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
          <dt class="text-muted-foreground">{t('onboarding.wizard_version')}</dt>
          <dd class="font-num">{runtimeInfo.managedVersion || t('onboarding.wizard_up_to_date')}</dd>
          <dt class="text-muted-foreground">{t('onboarding.wizard_location')}</dt>
          <dd class="truncate font-mono" title={runtimeInfo.managedPath ?? ''}>{runtimeInfo.managedPath}</dd>
        </dl>
      {:else}
        <div>
          <Button variant={runtimeInfo?.systemDetected ? 'outline' : 'secondary'} size="sm" onclick={installManaged} disabled={busy}>
            {busy ? t('onboarding.wizard_runtime_downloading') : t('onboarding.wizard_install_managed_btn')}
          </Button>
        </div>
      {/if}

      {#if installProgress}
        <div class="flex flex-col gap-1.5">
          <Progress value={pct} aria-label={t('onboarding.wizard_runtime_downloading')} />
          <span class="font-num text-xs text-muted-foreground">
            {formatBytes(installProgress.downloaded_bytes, 1)} / {formatBytes(installProgress.total_bytes, 1)}
          </span>
        </div>
      {/if}
    </div>

    {#if fallbackNote}
      <p class="text-xs break-words text-muted-foreground" role="note">
        {backendFallbackText(fallbackNote, true)} {t('onboarding.wizard_fallback_settings')}
      </p>
    {/if}
    {#if message}
      <p class={cn('text-xs break-words', tones[message.tone])} role={message.tone === 'error' ? 'alert' : 'status'}>{message.text}</p>
    {/if}
  {/if}
</div>
