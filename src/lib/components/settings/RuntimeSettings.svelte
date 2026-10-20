<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import HelpPopover from '../common/HelpPopover.svelte';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { onMount, onDestroy, untrack } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { Search, Check, X, Compass, TriangleAlert } from '@lucide/svelte';
  import type {
    AppSettings,
    SystemHardwareInfo,
    ReleaseAssetInfo,
    CustomBinaryValidation,
    RuntimeDownloadProgress,
    RuntimeAutoUpdateStatus,
    BackendStatus,
  } from '#lib/types.js';
  import { BACKEND_TOKEN, backendFallbackText, backendName, loadBackendFallback, normBackend, runtimeInstallErrorText } from '#lib/runtimeBackend.js';
  import { formatBytes, formatBytesPairText } from '#lib/format.js';
  import { errorCode } from '#lib/errors.js';
  import { localizeError } from '#lib/network.js';
  import { downloadRelease } from '#lib/stores/security.svelte.js';
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import * as RadioGroup from '#lib/components/ui/radio-group/index.js';
  import { cn } from '#lib/utils.js';
  import type { SettingsDraft } from './form.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import SwitchRow from './SwitchRow.svelte';
  import RuntimeRegistrySettings from './RuntimeRegistrySettings.svelte';
  import OnnxRuntimeSettings from './OnnxRuntimeSettings.svelte';
  import LemonadeSettings from './LemonadeSettings.svelte';
  import ConvertEnvSettings from './ConvertEnvSettings.svelte';
  import RuntimeCurrentState from './RuntimeCurrentState.svelte';
  import EngineFamiliesSettings from './EngineFamiliesSettings.svelte';
  import { pathExample } from '#lib/platform.js';

  interface Props {
    page?: string;
    settings: AppSettings;
    hardware: SystemHardwareInfo | null;
    form: SettingsDraft;
    onSettingsUpdated: (updated: AppSettings) => void;
    onReopenWizard?: () => void;
  }

  let { page = 'runtime-installed', settings, hardware, form, onSettingsUpdated, onReopenWizard }: Props = $props();

  const fmt = (b: number) => formatBytes(b, 1);
  const detach = <T,>(v: T): T => structuredClone($state.snapshot(v) as T);

  let mode = $derived(form.get('runtime_mode'));
  let installed = $derived(settings.managed_runtime.installed_version);

  // --- Backend of the managed build vs. the recommendation --------------------
  // The current runtime is never switched on its own: a different backend is only
  // downloaded when the user asks for it, and verified (stage 1) before activation.
  let backendStatus = $state<BackendStatus | null>(null);
  let recommendedToken = $derived(backendStatus ? BACKEND_TOKEN[backendStatus.advice.recommended] : null);
  let installedBackend = $derived(normBackend(backendStatus?.installed));
  let backendMatches = $derived(!!installedBackend && installedBackend === recommendedToken);
  /** A CPU-only build on hardware with a usable GPU: the servers don't use the graphics card. */
  let cpuOnGpu = $derived(installedBackend === 'cpu' && !!recommendedToken && recommendedToken !== 'cpu');
  /** Builds offered for a switch: the recommended one, and for a CPU-only build the GPU alternatives too. */
  let switchChoices = $derived.by(() => {
    if (!backendStatus || !installedBackend || !recommendedToken) return [];
    if (backendStatus.advice.reason_key === 'backend.reason_nvidia_driver_old') {
      return [recommendedToken, 'cuda'].filter((b) => b !== installedBackend);
    }
    if (backendMatches) return [];
    const alternatives = cpuOnGpu
      ? backendStatus.advice.alternatives
          .map((b) => BACKEND_TOKEN[b])
          .filter((b) => b !== 'cpu' && b !== recommendedToken && b !== installedBackend)
      : [];
    return [recommendedToken, ...alternatives];
  });
  let manualBackend = $state('');
  let officialChoices = $derived(
    backendStatus?.official.map((b) => BACKEND_TOKEN[b]).filter((b) => b !== installedBackend) ?? [],
  );
  let rocmWarning = $derived(
    manualBackend === 'rocm' ? backendStatus?.rocm_unsupported_gpu : null,
  );
  /** The wizard's note on why it installed another build than the recommended one. */
  let backendFallback = $derived.by(() => {
    void backendStatus;
    const note = loadBackendFallback();
    return note && note.installed === installedBackend && note.recommended === recommendedToken && note.version === installed
      ? note
      : null;
  });

  // Re-read whenever the installed build changes (the setup wizard, an update or a rollback
  // may install one while this page is open).
  $effect(() => {
    void installed;
    loadBackendStatus();
  });

  async function loadBackendStatus() {
    try {
      const s = await invoke<BackendStatus>('get_backend_status');
      if (!destroyed) backendStatus = s;
    } catch {
      backendStatus = null;
    }
  }

  // --- Managed runtime: check / download / activate / roll back -------------
  let isCheckingUpdate = $state(false);
  let updateInfo = $state<ReleaseAssetInfo | null>(null);
  let status = $state<{ tone: 'info' | 'ok' | 'error'; text: string; code?: string | null } | null>(null);
  let isStaging = $state(false);
  let isStaged = $state(false);
  let isActivating = $state(false);
  let runtimeProgress = $state<RuntimeDownloadProgress | null>(null);
  let hasBackup = $state(false);
  let confirmRollback = $state(false);
  let isRollingBack = $state(false);
  let progressTimer: ReturnType<typeof setInterval> | null = null;
  let destroyed = false;

  const progressPct = $derived(
    runtimeProgress && runtimeProgress.total_bytes > 0
      ? Math.min(100, (runtimeProgress.downloaded_bytes / runtimeProgress.total_bytes) * 100)
      : 0,
  );

  function stopProgressPolling() {
    if (progressTimer) clearInterval(progressTimer);
    progressTimer = null;
  }

  function startProgressPolling() {
    stopProgressPolling();
    progressTimer = setInterval(async () => {
      try {
        const p = await invoke<RuntimeDownloadProgress | null>('get_runtime_download_progress');
        if (!destroyed) runtimeProgress = p;
      } catch {
        /* progress is best-effort */
      }
    }, 500);
  }

  async function refreshBackupState() {
    try {
      hasBackup = await invoke<boolean>('has_runtime_backup');
    } catch {
      hasBackup = false;
    }
  }

  /** `backend` null keeps the installed build's backend (an update never switches it). */
  async function checkUpdate(backend: string | null = null) {
    isCheckingUpdate = true;
    if (backend) manualBackend = backend;
    isStaged = false;
    status = null;
    updateInfo = null;
    try {
      const res: ReleaseAssetInfo = await invoke('check_runtime_update', { backend });
      if (backend && normBackend(res.backend) !== backend) {
        // The release has no build of this backend; the query fell back to another one.
        status = {
          tone: 'error',
          text: t('settings.backend_unavailable', { backend: backendName(backend), found: res.asset_name }),
        };
        return;
      }
      updateInfo = res;
      const totalSize = res.size + res.extra_assets.reduce((sum, a) => sum + a.size, 0);
      const extras = res.extra_assets.length > 0 ? ` + ${res.extra_assets.map((a) => a.name).join(', ')}` : '';
      status =
        installed === res.tag_name && (!installedBackend || normBackend(res.backend) === installedBackend)
          ? { tone: 'ok', text: t('settings.already_latest', { version: res.tag_name }) }
          : {
              tone: 'info',
              text: t(uiLevel.advanced ? 'settings.latest_version_info' : 'settings.latest_version_simple', {
                version: res.tag_name,
                asset: res.asset_name,
                extras,
                size: fmt(totalSize),
              }),
            };
    } catch (e) {
      status = { tone: 'error', text: t('settings.update_check_failed', { error: String(e) }) };
    } finally {
      isCheckingUpdate = false;
    }
  }

  async function downloadAndStage() {
    if (!updateInfo || isStaging) return;
    isStaging = true;
    status = { tone: 'info', text: t('settings.downloading_verifying') };
    startProgressPolling();
    try {
      const version = await downloadRelease(updateInfo);
      if (version === null) {
        status = { tone: 'error', text: t('sec.integrity_cancelled') };
        return;
      }
      isStaged = true;
      status = { tone: 'ok', text: t('settings.stage1_success', { version }) };
    } catch (e) {
      status = { tone: 'error', text: t('settings.verification_error', { error: runtimeInstallErrorText(e) }), code: errorCode(e) };
    } finally {
      stopProgressPolling();
      runtimeProgress = null;
      isStaging = false;
    }
  }

  async function activateRuntime() {
    if (isActivating) return;
    isActivating = true;
    try {
      const updated: AppSettings = await invoke('activate_managed_runtime', {
        version: updateInfo?.tag_name ?? null,
      });
      onSettingsUpdated(detach(updated));
      isStaged = false;
      updateInfo = null;
      status = { tone: 'ok', text: t('settings.activation_success') };
      toast.success(t('settings.activation_success'));
      await refreshBackupState();
      await loadBackendStatus();
    } catch (e) {
      status = { tone: 'error', text: t('settings.activation_failed', { error: String(e) }) };
    } finally {
      isActivating = false;
    }
  }

  async function rollbackRuntime() {
    if (isRollingBack) return;
    isRollingBack = true;
    try {
      const updated: AppSettings = await invoke('rollback_managed_runtime');
      onSettingsUpdated(detach(updated));
      confirmRollback = false;
      status = { tone: 'ok', text: t('settings.rollback_success') };
      toast.success(t('settings.rollback_success'));
      await refreshBackupState();
      await loadBackendStatus();
    } catch (e) {
      status = { tone: 'error', text: t('settings.rollback_failed', { error: String(e) }) };
    } finally {
      isRollingBack = false;
    }
  }

  // --- Custom binary --------------------------------------------------------
  let customPath = $state(untrack(() => settings.custom_binary.binary_path || ''));
  let isValidatingCustom = $state(false);
  let customValidation = $state<CustomBinaryValidation | null>(null);
  let isAutoDetecting = $state(false);

  async function validateCustom() {
    const path = customPath.trim();
    if (!path || isValidatingCustom) return;
    isValidatingCustom = true;
    customValidation = null;
    let res: CustomBinaryValidation;
    try {
      res = await invoke<CustomBinaryValidation>('validate_custom_binary', { path });
    } catch (e) {
      res = { is_valid: false, version: null, supports_metrics: false, error: String(e) };
    }
    customValidation = res;
    form.set('custom_binary', {
      binary_path: path,
      version: res.version,
      supports_metrics: res.supports_metrics,
      is_valid: res.is_valid,
    });
    isValidatingCustom = false;
  }

  async function autoDetectSystemRuntime() {
    if (isAutoDetecting) return;
    isAutoDetecting = true;
    try {
      const updated: AppSettings = await invoke('auto_configure_system_runtime');
      // The backend already saved these two fields; drop local edits of them and follow the saved values.
      form.discard('runtime_mode', 'custom_binary');
      customPath = updated.custom_binary.binary_path || '';
      customValidation = {
        is_valid: updated.custom_binary.is_valid,
        version: updated.custom_binary.version,
        supports_metrics: updated.custom_binary.supports_metrics,
        error: null,
      };
      onSettingsUpdated(detach(updated));
    } catch (e) {
      customValidation = { is_valid: false, version: null, supports_metrics: false, error: String(e) };
    } finally {
      isAutoDetecting = false;
    }
  }

  // --- Runtime auto-update --------------------------------------------------
  let autoUpdateStatus = $state<RuntimeAutoUpdateStatus | null>(null);
  let autoUpdateError = $state<string | null>(null);
  let autoUpdateTimer: ReturnType<typeof setInterval> | null = null;

  const isAutoUpdateBusy = (s: RuntimeAutoUpdateStatus | null) =>
    s?.phase === 'checking' || s?.phase === 'downloading';

  function stopAutoUpdatePolling() {
    if (autoUpdateTimer) clearInterval(autoUpdateTimer);
    autoUpdateTimer = null;
  }

  function startAutoUpdatePolling() {
    if (autoUpdateTimer) return;
    autoUpdateTimer = setInterval(loadAutoUpdateStatus, 2000);
  }

  function applyAutoUpdateStatus(s: RuntimeAutoUpdateStatus) {
    if (destroyed) return;
    autoUpdateStatus = s;
    if (isAutoUpdateBusy(s)) startAutoUpdatePolling();
    else stopAutoUpdatePolling();
  }

  async function loadAutoUpdateStatus() {
    try {
      applyAutoUpdateStatus(await invoke<RuntimeAutoUpdateStatus>('get_runtime_auto_update_status'));
    } catch {
      stopAutoUpdatePolling();
    }
  }

  async function runAutoUpdateNow() {
    autoUpdateError = null;
    try {
      applyAutoUpdateStatus(await invoke<RuntimeAutoUpdateStatus>('run_runtime_auto_update'));
    } catch (e) {
      autoUpdateError = t('settings.auto_update_check_failed', { error: String(e) });
    }
  }

  const autoUpdatePhaseText = (s: RuntimeAutoUpdateStatus) => t(`settings.auto_update_phase_${s.phase}` as TKey);

  onMount(() => {
    refreshBackupState();
    loadAutoUpdateStatus();
  });

  onDestroy(() => {
    destroyed = true;
    stopProgressPolling();
    stopAutoUpdatePolling();
  });

  const statusTone = {
    info: 'text-muted-foreground',
    ok: 'text-success',
    error: 'text-destructive',
  } as const;
</script>

<div class="flex flex-col gap-6">
<div hidden={page !== 'runtime-installed'} class="flex flex-col gap-3">
  <SettingGroup title={t('settings.group_engine')}>
    <AdvancedOnly>
      <div class="p-3">
        <RadioGroup.Root
          class="grid grid-cols-2 gap-2 max-[1100px]:grid-cols-1"
          aria-label={t('settings.group_engine')}
          bind:value={() => mode, (v) => form.set('runtime_mode', v as 'Managed' | 'Custom')}
        >
          {#each [{ v: 'Managed', label: t('settings.managed_runtime_label'), desc: t('settings.managed_runtime_desc') }, { v: 'Custom', label: t('settings.custom_binary_label'), desc: t('settings.custom_binary_desc') }] as opt (opt.v)}
            <label
              for="runtime-mode-{opt.v}"
              class={cn(
                'flex min-w-0 cursor-pointer items-start gap-2.5 rounded-md border p-3 transition-colors hover:bg-muted/50 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring/50',
                mode === opt.v ? 'border-primary/40 bg-primary/5' : 'border-border',
              )}
            >
              <RadioGroup.Item id="runtime-mode-{opt.v}" value={opt.v} class="mt-0.5" />
              <span class="flex min-w-0 flex-col gap-0.5">
                <span class="flex items-center gap-1 text-sm font-medium">{opt.label}<HelpPopover text={opt.desc} label={opt.label} /></span>
              </span>
            </label>
          {/each}
        </RadioGroup.Root>
      </div>
    </AdvancedOnly>
    <RuntimeCurrentState {settings} {form} {onSettingsUpdated} />
    {#if onReopenWizard}
      <SettingRow label={t('settings.setup_label')} hint={t('settings.setup_hint')}>
        <Button variant="outline" size="sm" onclick={onReopenWizard}>
          <Compass /> {t('settings.setup_btn')}
        </Button>
      </SettingRow>
    {/if}
  </SettingGroup>

  {#if mode === 'Managed'}
    <SettingGroup title={t('settings.group_managed')}>
      <SettingRow
        label={t('settings.installed_version_label')}
        note={t('settings.recommended_backend_hint', {
          backend: backendName(recommendedToken ?? BACKEND_TOKEN[hardware?.recommended_backend ?? 'Vulkan']),
        })}
      >
        <span class="font-num text-sm">{installed || t('settings.not_downloaded_yet')}</span>
      </SettingRow>

      {#if backendStatus}
        {@const advice = backendStatus.advice}
        <SettingRow
          label={t('settings.backend_label')}
          note={installedBackend
            ? t('settings.backend_installed', { backend: backendName(installedBackend) })
            : t('settings.backend_none')}
        >
          {#snippet extra()}
            <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-xs" role="status">
              <span class="font-medium">
                {t('settings.backend_recommended', { backend: backendName(recommendedToken) })}<AdvancedOnly>{advice.gpu_name ? ` · ${advice.gpu_name}` : ''}</AdvancedOnly>
              </span>
              <AdvancedOnly><span class="text-muted-foreground">{t(advice.reason_key as TKey, { driver: hardware?.driver_info.cuda_driver_version ?? '?' })}</span></AdvancedOnly>
              <AdvancedOnly>
                {#if advice.alternatives.length > 0}
                  <span class="text-muted-foreground">
                    {t('settings.backend_alternatives', {
                      list: advice.alternatives.map((b) => backendName(BACKEND_TOKEN[b])).join(', '),
                    })}
                  </span>
                {/if}
              </AdvancedOnly>
              {#if !installedBackend}
                <span class="text-muted-foreground">{t('settings.backend_can_install')}</span>
              {:else if backendMatches}
                <span class="text-success">{t('settings.backend_matches')}</span>
              {:else}
                {#if cpuOnGpu}
                  <span class="text-warning" role="alert">{t('settings.backend_cpu_on_gpu')}</span>
                {/if}
                <span class="text-muted-foreground">
                  {#if backendFallback}
                    {backendFallbackText(backendFallback)}
                  {:else}
                    {t('settings.backend_differs', { installed: backendName(installedBackend), recommended: backendName(recommendedToken) })}
                  {/if}
                </span>
                <span class="text-muted-foreground">{t('settings.backend_switch_note')}</span>
              {/if}
            </div>
          {/snippet}
          <AdvancedOnly>
            {#each switchChoices as choice (choice)}
              <Button
                variant="outline"
                size="sm"
                onclick={() => checkUpdate(choice)}
                disabled={isCheckingUpdate || isStaging}
              >
                {t('settings.backend_switch', { backend: backendName(choice) })}
              </Button>
            {/each}
          </AdvancedOnly>
        </SettingRow>
        {#if officialChoices.length > 0}
          <SettingRow advanced label={t('settings.backend_other_build')} for="runtime-other-backend" help={t('settings.backend_switch_note')}>
            {#snippet extra()}
              {#if rocmWarning}
                <p class="text-xs text-warning" role="status">
                  {t('settings.backend_rocm_unsupported', { gpu: rocmWarning })}
                </p>
              {/if}
            {/snippet}
            <select
              id="runtime-other-backend"
              class="h-8 rounded-lg border border-input bg-transparent px-2 text-sm dark:bg-input/30 [&>option]:bg-popover"
              bind:value={manualBackend}
              disabled={isCheckingUpdate || isStaging}
            >
              <option value="">{t('settings.backend_choose_build')}</option>
              {#each officialChoices as choice (choice)}
                <option value={choice}>{backendName(choice)}</option>
              {/each}
            </select>
            <Button variant="outline" size="sm" onclick={() => checkUpdate(manualBackend)} disabled={!officialChoices.includes(manualBackend) || isCheckingUpdate || isStaging}>
              {t('settings.backend_install_build')}
            </Button>
          </SettingRow>
        {/if}
      {/if}

      <SettingRow label={t('settings.check_updates')} hint={t('settings.check_updates_hint')}>
        {#snippet extra()}
          {#if runtimeProgress}
            <div class="flex flex-col gap-1.5">
              <Progress value={progressPct} aria-label={runtimeProgress.current_asset} />
              <span class="truncate font-num text-xs text-muted-foreground">
                {#if uiLevel.advanced}
                  {runtimeProgress.current_asset}: {runtimeProgress.total_bytes > 0
                    ? `${formatBytesPairText(runtimeProgress.downloaded_bytes, runtimeProgress.total_bytes)} (${progressPct.toFixed(0)}%)`
                    : fmt(runtimeProgress.downloaded_bytes)}
                {:else}
                  {progressPct.toFixed(0)}%
                {/if}
              </span>
            </div>
          {/if}
          {#if status}
            <p class={cn('text-xs break-words', statusTone[status.tone])} role={status.tone === 'error' ? 'alert' : 'status'}>
              {status.text}
            </p>
            {#if status.code === 'runtime.backend_no_device' && backendStatus?.official.includes('Vulkan') && installedBackend !== 'vulkan'}
              <Button variant="outline" size="sm" onclick={() => checkUpdate('vulkan')} disabled={isCheckingUpdate || isStaging}>
                {t('settings.backend_switch_vulkan')}
              </Button>
            {/if}
          {/if}
        {/snippet}
        <Button variant="outline" size="sm" onclick={() => checkUpdate()} disabled={isCheckingUpdate || isStaging}>
          {isCheckingUpdate ? t('settings.checking') : t('settings.check_updates_btn')}
        </Button>
        {#if updateInfo && !isStaged && (updateInfo.tag_name !== installed || (installedBackend && normBackend(updateInfo.backend) !== installedBackend))}
          <Button size="sm" onclick={downloadAndStage} disabled={isStaging}>
            {isStaging ? t('settings.staging') : t('settings.download_and_test', { version: updateInfo.tag_name })}
          </Button>
        {/if}
        {#if isStaged}
          <Button size="sm" onclick={activateRuntime} disabled={isActivating}>
            {t('settings.activate_new_version')}
          </Button>
        {/if}
      </SettingRow>

      <SwitchRow
        id="runtime-auto-update"
        label={t('settings.runtime_auto_update_label')}
        hint={t('settings.runtime_auto_update_desc')}
        checked={form.get('auto_update')}
        dirty={form.isDirty('auto_update')}
        onchange={(v) => form.set('auto_update', v)}
      />

      {#if settings.managed_runtime.auto_update}
        <div class="flex items-center justify-between gap-4 bg-muted/30 px-4 py-2.5">
          <div class="flex min-w-0 flex-col gap-0.5 text-xs text-muted-foreground" role="status" aria-live="polite">
            {#if autoUpdateStatus}
              <span class="text-foreground">{autoUpdatePhaseText(autoUpdateStatus)}</span>
              <span class="truncate">
                {autoUpdateStatus.last_check
                  ? t('settings.auto_update_last_check', {
                      time: new Date(autoUpdateStatus.last_check * 1000).toLocaleString(locale()),
                    })
                  : t('settings.auto_update_never_checked')}
                {#if autoUpdateStatus.latest_version}
                  · {t('settings.auto_update_latest', { version: autoUpdateStatus.latest_version })}
                {/if}
              </span>
              {#if autoUpdateStatus.message}<span
                  class={cn(
                    'break-words',
                    autoUpdateStatus.phase === 'kept_current' && 'text-warning',
                    autoUpdateStatus.phase === 'error' && 'text-destructive',
                  )}>{localizeError(autoUpdateStatus.message)}</span
                >{/if}
            {/if}
            {#if autoUpdateError}<span class="text-destructive" role="alert">{autoUpdateError}</span>{/if}
          </div>
          <Button
            variant="outline"
            size="sm"
            onclick={runAutoUpdateNow}
            disabled={isAutoUpdateBusy(autoUpdateStatus) || !installed}
          >
            {isAutoUpdateBusy(autoUpdateStatus) ? t('settings.checking') : t('settings.auto_update_check_now')}
          </Button>
        </div>
      {/if}
    </SettingGroup>

    <SettingGroup title={t('settings.group_danger')} danger>
      <SettingRow label={t('settings.rollback_button')} note={hasBackup ? t('settings.rollback_hint') : t('settings.no_backup_available')}>
        {#if confirmRollback}
          <Button variant="destructive" size="sm" onclick={rollbackRuntime} disabled={isRollingBack}>
            {t('settings.confirm_rollback_yes', {
              version: settings.managed_runtime.previous_version || t('settings.previous_version_fallback'),
            })}
          </Button>
          <Button variant="ghost" size="sm" onclick={() => (confirmRollback = false)} disabled={isRollingBack}>
            {t('settings.cancel')}
          </Button>
        {:else}
          <Button variant="destructive" size="sm" disabled={!hasBackup} onclick={() => (confirmRollback = true)}>
            {t('settings.rollback_btn')}
          </Button>
        {/if}
      </SettingRow>
    </SettingGroup>
  {:else}
    <SettingGroup advanced title={t('settings.group_custom')}>
      <SettingRow label={t('settings.local_binary_path_label')} for="custom-path" note={t('settings.custom_binary_note')} help={t('settings.custom_binary_help')} dirty={form.isDirty('custom_binary')}>
        {#snippet extra()}
          <div class="flex min-w-0 items-center gap-2">
            <Input
              id="custom-path"
              class="h-8 min-w-0 flex-1 font-mono text-xs"
              placeholder={pathExample('C:\\...\\llama-server.exe', '~/llama.cpp/build/bin/llama-server')}
              bind:value={customPath}
              onkeydown={(e) => {
                if (e.key === 'Enter') validateCustom();
              }}
            />
            <Button variant="outline" size="sm" onclick={validateCustom} disabled={isValidatingCustom || !customPath.trim()}>
              {isValidatingCustom ? t('settings.validating') : t('settings.validate')}
            </Button>
            <Button variant="outline" size="sm" onclick={autoDetectSystemRuntime} disabled={isAutoDetecting}>
              <Search /> {isAutoDetecting ? t('settings.searching') : t('settings.auto_detect_btn')}
            </Button>
          </div>
          {#if customValidation}
            <div
              class={cn(
                'flex items-start gap-2 rounded-md border px-2.5 py-2 text-xs',
                customValidation.is_valid
                  ? 'border-success/30 bg-success/10 text-success'
                  : 'border-destructive/30 bg-destructive/10 text-destructive',
              )}
              role={customValidation.is_valid ? 'status' : 'alert'}
            >
              {#if customValidation.is_valid}
                <Check class="mt-0.5 size-3.5 shrink-0" />
                <div class="flex min-w-0 flex-col gap-0.5">
                  <span class="font-medium break-words">
                    {t('settings.binary_working_label')}
                    <span class="font-num">{customValidation.version || t('settings.unknown_version')}</span>
                  </span>
                  <span class="text-muted-foreground">
                    {customValidation.supports_metrics ? t('settings.metrics_supported') : t('settings.metrics_not_supported')}
                  </span>
                </div>
              {:else}
                <X class="mt-0.5 size-3.5 shrink-0" />
                <span class="break-words">{t('settings.validation_failed_label')} {localizeError(customValidation.error)}</span>
              {/if}
            </div>
          {:else if settings.custom_binary.binary_path && !form.isDirty('custom_binary')}
            <p class="flex items-center gap-1.5 text-xs text-muted-foreground">
              {#if settings.custom_binary.is_valid}
                <Check class="size-3.5 text-success" />
                <span class="font-num">{settings.custom_binary.version || t('settings.unknown_version')}</span>
              {:else}
                <TriangleAlert class="size-3.5 text-warning" /> {t('settings.custom_unvalidated')}
              {/if}
            </p>
          {/if}
        {/snippet}
      </SettingRow>
    </SettingGroup>
  {/if}

  <EngineFamiliesSettings {settings} {onSettingsUpdated} />
</div>
<div hidden={page !== 'runtime-registry'} class="flex flex-col gap-3">
  <RuntimeRegistrySettings />
  <OnnxRuntimeSettings />
  <LemonadeSettings />
  <ConvertEnvSettings />
</div>
</div>
