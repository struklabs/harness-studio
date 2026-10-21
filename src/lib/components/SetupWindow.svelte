<script lang="ts">
  // First-run Setup: Welcome, Choose, Install, Done. Same look as the uninstall window
  // (src/routes/uninstall). It installs through the app's own functions: the managed engine
  // (`runtimeInstall.ts`), the download queue (`downloads`), the modules and the integrations
  // registry; nothing here talks to the network or the disk on its own.
  import { onMount, untrack } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import Bot from '@lucide/svelte/icons/bot';
  import CheckCircle from '@lucide/svelte/icons/circle-check';
  import Circle from '@lucide/svelte/icons/circle';
  import Cpu from '@lucide/svelte/icons/cpu';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Loader from '@lucide/svelte/icons/loader-circle';
  import MinusCircle from '@lucide/svelte/icons/circle-minus';
  import Package from '@lucide/svelte/icons/package';
  import Plug from '@lucide/svelte/icons/plug';
  import Terminal from '@lucide/svelte/icons/terminal';
  import Warning from '@lucide/svelte/icons/triangle-alert';
  import XCircle from '@lucide/svelte/icons/circle-x';
  import { invoke } from '#lib/ipc.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { dialog } from '#lib/actions/dialog.js';
  import { formatBytes } from '#lib/format.js';
  import { localizeError } from '#lib/network.js';
  import { backendName } from '#lib/runtimeBackend.js';
  import { installManagedRuntime, previewManagedRuntime } from '#lib/runtimeInstall.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { downloads } from '#lib/stores/downloads.svelte.js';
  import { getStarted } from '#lib/stores/getStarted.svelte.js';
  import { runtimesStore } from '#lib/stores/runtimes.svelte.js';
  import { statusError, statusKey, progressPercent } from '#lib/components/hub/downloadQueue.svelte.js';
  import { applyIntegration } from '#lib/components/connections/integrations.js';
  import { quantBits } from '#lib/components/hub/recommend.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import BrandLogo from './common/BrandLogo.svelte';
  import type { AppSettings, HfModelVariant, SystemHardwareInfo } from '#lib/types.js';
  import {
    CATEGORY_ITEMS,
    CATEGORY_ORDER,
    INTEGRATION_REGISTRY_ID,
    MODEL_TIERS,
    agentsFit,
    defaultSelection,
    cancelPending,
    failedItems,
    initialRun,
    isFinished,
    modelTier,
    orderedSelection,
    patchRun,
    profileOf,
    recommendedModelTier,
    retryIds,
    runProgress,
    totalDownload,
    type ModelTierId,
    type RunItem,
    type SetupCategoryId,
    type SetupFacts,
    type SetupItemId,
  } from '#lib/setup.js';

  interface Props {
    settings: AppSettings;
    hardware: SystemHardwareInfo | null;
    /** `dashboard`: the Done screen's button, to land on the Get Started checklist. */
    onComplete: (updated: AppSettings, opts?: { dashboard?: boolean }) => void;
    /** Present when Setup was reopened from Settings: leaves without finishing. */
    onClose?: () => void;
  }
  let { settings, hardware, onComplete, onClose }: Props = $props();

  type Screen = 'welcome' | 'choose' | 'install' | 'done';
  interface Context { skip_first_run: boolean; data_dir: string; data_dir_fixed: boolean }
  interface StorageDir { path: string; is_primary: boolean; free_bytes: number }
  interface ModuleListing {
    manifest: { id: string; components: { download_bytes: number }[] };
    state: unknown;
    incompatibility: string | null;
    progress: { detail: string; downloaded_bytes: number; total_bytes: number } | null;
  }
  interface ClientStatus { id: string; installed: boolean }

  const firstRun = untrack(() => !settings.privacy.onboarding_completed && !onClose);
  const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const dur = (ms: number) => (reduced ? 0 : ms);

  let screen = $state<Screen>('welcome');
  let ctx = $state<Context | null>(null);
  let loadError = $state<string | null>(null);
  let ready = $state(false);
  let busy = $state(false);
  let level = $state<'simple' | 'advanced'>(untrack(() => (firstRun ? 'simple' : (settings.ui_level ?? 'advanced'))));

  // What the machine has and what this window may offer.
  const profile = $derived(profileOf(hardware));
  let engineInstalled = $state(false);
  let engineBytes = $state<number | null>(null);
  let engineBackend = $state<string | null>(null);
  let agentsInstalled = $state(false);
  let agentsPlatformOk = $state(true);
  let agentsBytes = $state<number | null>(null);
  let clients = $state<Record<string, boolean>>({});
  let modelsDir = $state(untrack(() => settings.models_dir));
  let freeDiskBytes = $state<number | null>(null);
  let storageError = $state<string | null>(null);
  let tier = $state<ModelTierId | null>(null);
  let tierBytes = $state<Partial<Record<ModelTierId, number>>>({});
  let selected = $state(new Set<SetupItemId>());
  let touched = false;

  const activeTier = $derived(tier ?? recommendedModelTier(profile));
  const backend = $derived(engineBackend ?? profile.backend);
  const unavailable = $derived<Partial<Record<SetupItemId, boolean>>>({
    agents: !agentsPlatformOk || !agentsFit(profile),
    opencode: !clients['opencode'],
    zed: !clients['zed'],
    vscode: !clients['vscode-copilot'],
  });
  const facts = $derived<SetupFacts>({
    installed: { engine: engineInstalled, agents: agentsInstalled },
    unavailable: { agents: unavailable.agents },
    freeDiskBytes,
  });
  const sizes = $derived<Partial<Record<SetupItemId, number>>>({
    engine: engineBytes ?? 0,
    model: tierBytes[activeTier] ?? modelTier(activeTier).approxBytes,
    agents: agentsBytes ?? 0,
  });
  const download = $derived(totalDownload(selected, sizes));

  // Until the user changes a tick, the defaults follow what is detected (hardware arrives late).
  $effect(() => {
    const next = defaultSelection(profile, facts, activeTier);
    if (!touched && ready) selected = next;
  });

  function toggle(id: SetupItemId, on: boolean) {
    touched = true;
    const next = new Set(selected);
    if (on) next.add(id);
    else next.delete(id);
    selected = next;
  }
  const selectRecommended = () => {
    touched = false;
    selected = defaultSelection(profile, facts, activeTier);
  };
  const selectNone = () => {
    touched = true;
    selected = new Set();
  };
  function chooseTier(id: ModelTierId) {
    tier = id;
    // Picking a size is picking the model: the card gets ticked.
    if (!facts.unavailable.model) toggle('model', true);
    void loadTierSize(id);
  }

  // ---- loading ----

  /** The file of a tier's repo: Q4_K_M when there is one, else the smallest file of 4 bits or more. */
  function pickVariant(variants: HfModelVariant[], quant: string): HfModelVariant | null {
    const sized = variants.filter((v) => v.total_size_bytes > 0 && !v.not_runnable);
    const exact = sized.find((v) => v.quantization.toUpperCase() === quant.toUpperCase());
    if (exact) return exact;
    const q4 = sized.filter((v) => quantBits(v.quantization) >= 4).sort((a, b) => a.total_size_bytes - b.total_size_bytes);
    return q4[0] ?? sized[0] ?? null;
  }

  async function tierVariant(id: ModelTierId): Promise<HfModelVariant | null> {
    const spec = modelTier(id);
    const variants = await invoke<HfModelVariant[]>('get_hf_model_variants', { repoId: spec.repo });
    return pickVariant(variants, spec.quant);
  }

  async function loadTierSize(id: ModelTierId) {
    if (tierBytes[id]) return;
    try {
      const variant = await tierVariant(id);
      if (variant) tierBytes = { ...tierBytes, [id]: variant.total_size_bytes };
    } catch {
      // offline: the rough size stays
    }
  }

  /** The window opens on the context alone; the rest fills in as each answer arrives (the module
   *  list can take a while to measure), and the defaults follow it until the user changes a tick. */
  async function loadFacts() {
    ctx = await invoke<Context>('setup_context');
  }

  function loadMore() {
    invoke<{ managedInstalled: boolean; systemDetected: boolean; activeMode: string }>('get_runtime_detection_info')
      .then((r) => (engineInstalled = r.managedInstalled || (r.systemDetected && r.activeMode === 'Custom')))
      .catch(() => {});
    invoke<ModuleListing[]>('list_modules')
      .then((list) => {
        const agents = list.find((m) => m.manifest.id === 'agents');
        agentsPlatformOk = !!agents && !agents.incompatibility;
        agentsBytes = agents?.manifest.components.reduce((n, c) => n + c.download_bytes, 0) ?? null;
        agentsInstalled = !!agents && agents.state === 'installed';
      })
      .catch(() => {});
    invoke<ClientStatus[]>('detect_installed_clients')
      .then((found) => (clients = Object.fromEntries(found.map((c) => [c.id, c.installed]))))
      .catch(() => {});
    invoke<StorageDir[]>('get_storage_directories')
      .then((dirs) => {
        const primary = dirs.find((d) => d.is_primary);
        freeDiskBytes = primary && primary.free_bytes > 0 ? primary.free_bytes : null;
      })
      .catch(() => {});
  }

  /** The silent installer's marker: nothing is shown; the app opens straight into its usual start. */
  async function finishQuietly() {
    const fresh = await invoke<AppSettings>('get_app_settings');
    const updated: AppSettings = { ...fresh, privacy: { ...fresh.privacy, onboarding_completed: true } };
    await invoke('save_app_settings', { settings: updated });
    onComplete(updated);
  }

  onMount(() => {
    void (async () => {
      try {
        await loadFacts();
        if (firstRun && ctx?.skip_first_run) {
          await finishQuietly();
          return;
        }
      } catch (e) {
        loadError = localizeError(e);
      }
      ready = true;
      if (!loadError) loadMore();
      // Sizes of the engine build and the model file come from the network: they fill in later.
      previewManagedRuntime()
        .then((r) => {
          engineBytes = r.size + r.extra_assets.reduce((n, a) => n + a.size, 0);
          engineBackend = r.backend ?? null;
        })
        .catch(() => {});
      // Every card shows the size of the file it would download, not the rough tier size.
      for (const option of MODEL_TIERS) void loadTierSize(option.id);
    })();
  });

  // The recommended tier is read once the hardware is known.
  $effect(() => {
    if (ready) void loadTierSize(activeTier);
  });

  // ---- storage ----

  async function pickModelsFolder() {
    storageError = null;
    try {
      const picked = await invoke<string | null>('pick_directory_dialog');
      if (!picked) return;
      const updated = await invoke<AppSettings>('set_primary_storage_directory', { dirPath: picked });
      modelsDir = updated.models_dir;
      const dirs = await invoke<StorageDir[]>('get_storage_directories');
      const primary = dirs.find((d) => d.is_primary);
      freeDiskBytes = primary && primary.free_bytes > 0 ? primary.free_bytes : null;
    } catch (e) {
      storageError = t('setup.storage_failed', { error: localizeError(e) });
    }
  }

  // ---- install ----

  let items = $state<RunItem[]>([]);
  let running = $state(false);
  let cancelled = $state(false);
  let currentModelJob: number | null = null;
  const failed = $derived(failedItems(items));
  const finished = $derived(isFinished(items));

  const itemName = (id: SetupItemId): string => {
    if (id === 'engine') return t('setup.item_engine', { backend: backendName(backend) });
    if (id === 'model') return t('setup.item_model', { model: modelLabel(activeTier), quant: modelTier(activeTier).quant });
    return t(`setup.item_${id}` as TKey);
  };
  const modelLabel = (id: ModelTierId) => modelTier(id).repo.split('/')[1].replace(/-GGUF$/i, '');

  const patch = (id: SetupItemId, p: Partial<Omit<RunItem, 'id'>>) => (items = patchRun(items, id, p));

  function sleep(ms: number) {
    return new Promise((r) => setTimeout(r, ms));
  }

  async function runEngine() {
    const result = await installManagedRuntime(hardware, {
      onPhase: (phase) => {
        if (phase.kind === 'checking') patch('engine', { detail: t('setup.detail_checking') });
        else if (phase.kind === 'downloading') patch('engine', { detail: t('setup.detail_downloading', { version: phase.version }) });
        else if (phase.kind === 'fallback') patch('engine', { detail: t('setup.detail_fallback') });
        else patch('engine', { detail: t('setup.detail_activating'), fraction: 1 });
      },
      onProgress: (p) => {
        if (!p || p.total_bytes <= 0) return;
        patch('engine', {
          fraction: Math.min(1, p.downloaded_bytes / p.total_bytes),
          detail: t('setup.detail_bytes', { done: formatBytes(p.downloaded_bytes, 1), total: formatBytes(p.total_bytes, 1) }),
        });
      },
    });
    if (result === null) throw new Error(t('setup.detail_declined'));
    engineInstalled = true;
    await runtimesStore.load(true);
  }

  async function runModel() {
    patch('model', { detail: t('setup.detail_picking') });
    const variant = await tierVariant(activeTier);
    if (!variant) throw new Error(t('setup.detail_no_variant'));
    const spec = modelTier(activeTier);
    const id = await downloads.enqueueVariant(spec.repo, variant);
    currentModelJob = id;
    try {
      for (;;) {
        await downloads.refresh();
        const job = downloads.jobs.find((j) => j.id === id);
        if (!job) throw new Error(t('setup.state_cancelled'));
        const key = statusKey(job.status);
        if (key === 'Completed') return;
        if (key === 'Error') throw new Error(statusError(job.status) ?? t('setup.state_failed'));
        patch('model', {
          fraction: progressPercent(job) / 100,
          detail: t('setup.detail_bytes', { done: formatBytes(job.downloaded_bytes, 1), total: formatBytes(job.total_bytes, 1) }),
        });
        if (cancelled) throw new Error(t('setup.state_cancelled'));
        await sleep(500);
      }
    } finally {
      currentModelJob = null;
    }
  }

  async function runAgents() {
    patch('agents', { detail: t('setup.detail_module') });
    const poll = setInterval(async () => {
      try {
        const list = await invoke<ModuleListing[]>('list_modules');
        const p = list.find((m) => m.manifest.id === 'agents')?.progress;
        if (p) {
          patch('agents', {
            detail: p.detail || t('setup.detail_module'),
            fraction: p.total_bytes > 0 ? Math.min(1, p.downloaded_bytes / p.total_bytes) : null,
          });
        }
      } catch {
        // progress is best-effort
      }
    }, 1000);
    try {
      await invoke('install_module', { id: 'agents' });
      agentsInstalled = true;
    } finally {
      clearInterval(poll);
    }
  }

  /** `done`, or `skipped` with the reason as the item's detail. */
  async function runIntegration(id: SetupItemId): Promise<'done' | 'skipped'> {
    const registryId = INTEGRATION_REGISTRY_ID[id];
    const defs = domain.definitions;
    if (!registryId || !app.gatewayRunning || defs.length === 0) {
      patch(id, { detail: t('setup.detail_gateway_off') });
      return 'skipped';
    }
    const def = defs[0];
    await applyIntegration(registryId, {
      baseUrl: app.inferenceUrl,
      apiKey: null,
      modelId: def.alias,
      contextSize: Number(def.load?.context_size) || 32768,
    });
    patch(id, { detail: t('setup.detail_applied') });
    return 'done';
  }

  async function runItem(id: SetupItemId): Promise<'done' | 'skipped'> {
    if (id === 'engine') await runEngine();
    else if (id === 'model') await runModel();
    else if (id === 'agents') await runAgents();
    else if (id === 'cli') {
      await invoke('cli_path_install', { userPath: null });
      patch('cli', { detail: t('setup.detail_cli_added') });
    } else return runIntegration(id);
    return 'done';
  }

  async function runAll(ids: SetupItemId[]) {
    running = true;
    cancelled = false;
    for (const id of ids) {
      if (cancelled) break;
      patch(id, { state: 'running', fraction: null, detail: null });
      try {
        const result = await runItem(id);
        // Transient progress text is dropped; a result text (a skip reason, "added") stays.
        patch(id, { state: result, fraction: 1, ...(id === 'engine' || id === 'model' || id === 'agents' ? { detail: null } : {}) });
      } catch (e) {
        const text = e instanceof Error ? e.message : localizeError(e);
        patch(id, cancelled ? { state: 'cancelled', detail: null } : { state: 'failed', detail: text, fraction: null });
      }
    }
    items = cancelPending(items);
    running = false;
    if (!cancelled && failedItems(items).length === 0) {
      await sleep(dur(700));
      if (screen === 'install') screen = 'done';
    }
  }

  function start() {
    const ids = orderedSelection(selected);
    items = initialRun(ids);
    screen = 'install';
    if (ids.length === 0) {
      screen = 'done';
      return;
    }
    void runAll(ids);
  }

  function retry() {
    const ids = retryIds(items);
    items = items.map((i) => (ids.includes(i.id) ? { ...i, state: 'pending' as const, detail: null, fraction: null } : i));
    void runAll(ids);
  }

  function cancel() {
    cancelled = true;
    if (currentModelJob !== null) void downloads.cancel(currentModelJob);
    if (items.some((i) => i.id === 'agents' && i.state === 'running')) void invoke('cancel_module_install', { id: 'agents' }).catch(() => {});
  }

  // ---- finishing ----

  let finishError = $state<string | null>(null);

  async function finish(dashboard: boolean) {
    busy = true;
    finishError = null;
    try {
      // Install steps changed settings in the backend: start from those, not from the prop.
      const fresh = await invoke<AppSettings>('get_app_settings');
      const updated: AppSettings = {
        ...fresh,
        ui_level: level,
        privacy: { ...fresh.privacy, onboarding_completed: true },
      };
      await invoke('save_app_settings', { settings: updated });
      await runtimesStore.load(true);
      if (dashboard) getStarted.show();
      onComplete(updated, { dashboard });
    } catch (e) {
      finishError = t('setup.finish_failed', { error: localizeError(e) });
      busy = false;
    }
  }

  const screens: { id: Screen; label: TKey }[] = [
    { id: 'welcome', label: 'setup.step_welcome' },
    { id: 'choose', label: 'setup.step_choose' },
    { id: 'install', label: 'setup.step_install' },
    { id: 'done', label: 'setup.step_done' },
  ];
  const screenIndex = $derived(screens.findIndex((s) => s.id === screen));

  const categoryIcons = { engine: Cpu, model: Package, agents: Bot, server: Terminal, integrations: Plug } as const;
  const sizeText = (bytes: number | null | undefined) => (bytes ? formatBytes(bytes, 1) : t('setup.size_unknown'));
  const gpuName = $derived.by(() => {
    const g = hardware?.gpus[hardware.primary_gpu_index ?? 0] ?? hardware?.gpus[0];
    return g && profile.accelerator !== 'cpu' ? g.name : t('setup.hw_cpu_only');
  });
  const installedAll = $derived(selected.size === 0);
  const doneItems = $derived(items.filter((i) => i.state === 'done').length);
</script>

<!-- Below the title bar (h-11), which stays usable: the window is frameless. -->
<div
  class="fixed inset-x-0 top-11 bottom-0 z-50 flex min-h-0 flex-col bg-background text-foreground"
  role="dialog"
  aria-modal="true"
  aria-labelledby="setup-title"
  use:dialog={onClose && !running ? { onEscape: onClose } : undefined}
>
  <header class="border-b border-border px-6 pt-5 pb-4">
    <div class="mx-auto flex w-full max-w-3xl flex-col gap-3">
      <h1 id="setup-title" class="text-lg font-semibold tracking-tight">{t('setup.window_title')}</h1>
      <ol class="flex items-center gap-2 text-xs" aria-label={t('setup.window_title')}>
        {#each screens as s, i (s.id)}
          <li class="flex items-center gap-2" aria-current={i === screenIndex ? 'step' : undefined}>
            <span
              class="flex size-5 items-center justify-center rounded-full text-[0.65rem] font-semibold transition-colors duration-300 {i <= screenIndex ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'}"
            >{i + 1}</span>
            <span class={i === screenIndex ? 'font-medium text-foreground' : 'text-muted-foreground'}>{t(s.label)}</span>
            {#if i < screens.length - 1}<span class="mx-1 h-px w-6 bg-border" aria-hidden="true"></span>{/if}
          </li>
        {/each}
      </ol>
    </div>
  </header>

  <main class="min-h-0 flex-1 overflow-y-auto px-6 py-5">
    <div class="mx-auto w-full max-w-3xl">
      {#if !ready}
        <div class="flex h-48 items-center justify-center text-muted-foreground"><Loader class="size-5 animate-spin" /></div>
      {:else if loadError}
        <div class="flex flex-col gap-2" role="alert">
          <h2 class="text-base font-semibold">{t('setup.load_error')}</h2>
          <p class="text-sm break-words text-destructive">{loadError}</p>
        </div>
      {:else if screen === 'welcome'}
        <section class="flex flex-col gap-5" in:fly={{ x: 16, duration: dur(200) }}>
          <div class="flex flex-col gap-3">
            <BrandLogo size={28} />
            <h2 class="text-xl font-semibold tracking-tight">{t('setup.welcome_title')}</h2>
            <p class="text-sm leading-relaxed text-muted-foreground">{t('setup.welcome_desc')}</p>
          </div>

          <div class="flex flex-col gap-1.5 rounded-xl border border-border bg-card p-4">
            <h3 class="flex items-center gap-2 text-sm font-semibold"><Cpu class="size-4 text-primary" />{t('setup.hw_title')}</h3>
            {#if hardware}
              <p class="text-sm break-words" data-testid="setup-hw">
                {t('setup.hw_summary', { gpu: gpuName, ram: formatBytes(hardware.total_ram_bytes, 0), backend: backendName(profile.backend) })}
              </p>
            {:else}
              <p class="flex items-center gap-2 text-sm text-muted-foreground"><Loader class="size-4 animate-spin" />{t('setup.hw_detecting')}</p>
            {/if}
          </div>

          <ul class="flex flex-col gap-2 text-sm text-muted-foreground">
            {#each ['setup.point_local', 'setup.point_minimal', 'setup.point_later'] as key (key)}
              <li class="flex items-start gap-2"><CheckCircle class="mt-0.5 size-4 shrink-0 text-primary" />{t(key as TKey)}</li>
            {/each}
          </ul>

          <div class="flex flex-col gap-2">
            <h3 class="text-sm font-semibold">{t('setup.level_title')}</h3>
            <div class="grid gap-3 sm:grid-cols-2" role="radiogroup" aria-label={t('setup.level_title')}>
              {#each [
                { id: 'simple', title: 'setup.level_simple', desc: 'setup.level_simple_desc' },
                { id: 'advanced', title: 'setup.level_advanced', desc: 'setup.level_advanced_desc' },
              ] as option (option.id)}
                <label
                  class="flex cursor-pointer flex-col gap-1 rounded-xl border p-3 transition-all duration-200 has-[:focus-visible]:ring-3 has-[:focus-visible]:ring-ring/50 {level === option.id ? 'border-primary bg-primary/5' : 'border-border bg-card hover:border-foreground/30'}"
                >
                  <input type="radio" name="level" class="sr-only" value={option.id} checked={level === option.id} onchange={() => (level = option.id as 'simple' | 'advanced')} />
                  <span class="text-sm font-semibold">{t(option.title as TKey)}</span>
                  <span class="text-xs leading-relaxed text-muted-foreground">{t(option.desc as TKey)}</span>
                </label>
              {/each}
            </div>
          </div>
        </section>
      {:else if screen === 'choose'}
        <section class="flex flex-col gap-4" in:fly={{ x: 16, duration: dur(200) }}>
          <div>
            <h2 class="text-base font-semibold">{t('setup.choose_title')}</h2>
            <p class="mt-1 text-sm text-muted-foreground">{t('setup.choose_desc')}</p>
          </div>
          <div class="flex items-center justify-end gap-1">
            <Button variant="ghost" size="xs" onclick={selectRecommended}>{t('setup.select_defaults')}</Button>
            <Button variant="ghost" size="xs" onclick={selectNone}>{t('setup.select_none')}</Button>
          </div>

          {#each CATEGORY_ORDER as cat (cat)}
            {@const Icon = categoryIcons[cat]}
            <div class="flex flex-col gap-2 rounded-xl border border-border bg-card p-4" data-testid="setup-cat-{cat}">
              <div>
                <h3 class="flex items-center gap-2 text-sm font-semibold"><Icon class="size-4 text-primary" />{t(`setup.cat_${cat}` as TKey)}</h3>
                <p class="mt-0.5 text-xs text-muted-foreground">{t(`setup.cat_${cat}_desc` as TKey)}</p>
              </div>

              {#if cat === 'model'}
                <div class="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label={t('setup.cat_model')}>
                  {#each MODEL_TIERS as option (option.id)}
                    <label
                      class="flex cursor-pointer flex-col gap-0.5 rounded-lg border p-2.5 transition-colors has-[:focus-visible]:ring-3 has-[:focus-visible]:ring-ring/50 {activeTier === option.id ? 'border-primary bg-primary/5' : 'border-border hover:border-foreground/30'}"
                    >
                      <input type="radio" name="tier" class="sr-only" checked={activeTier === option.id} onchange={() => chooseTier(option.id)} />
                      <span class="flex items-center justify-between gap-2 text-sm font-medium">
                        {t(`setup.tier_${option.id}` as TKey)}
                        <span class="text-xs tabular-nums text-muted-foreground">{sizeText(tierBytes[option.id] ?? option.approxBytes)}</span>
                      </span>
                      <span class="text-xs text-muted-foreground">{t(`setup.tier_${option.id}_desc` as TKey)}</span>
                      {#if option.id === recommendedModelTier(profile)}
                        <Badge variant="secondary" class="mt-1 self-start">{t('setup.tier_recommended_for')}</Badge>
                      {/if}
                    </label>
                  {/each}
                </div>
              {/if}

              <ul class="flex flex-col gap-1.5">
                {#each CATEGORY_ITEMS[cat] as id (id)}
                  {@const done = !!facts.installed[id]}
                  {@const blocked = !!unavailable[id]}
                  {@const off = done || blocked}
                  <li>
                    <label
                      class="flex items-start gap-3 rounded-lg border p-3 transition-colors duration-150 has-[:focus-visible]:ring-3 has-[:focus-visible]:ring-ring/50 {off ? 'cursor-default border-transparent opacity-70' : selected.has(id) ? 'cursor-pointer border-primary/40 bg-primary/5' : 'cursor-pointer border-border hover:border-foreground/30'}"
                    >
                      <input
                        type="checkbox"
                        class="mt-0.5 size-4 accent-[var(--primary)]"
                        data-testid="setup-item-{id}"
                        checked={selected.has(id) && !off}
                        disabled={off}
                        onchange={(e) => toggle(id, e.currentTarget.checked)}
                      />
                      <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                        <span class="flex flex-wrap items-center gap-2 text-sm font-medium">
                          {itemName(id)}
                          {#if done}
                            <Badge variant="outline" class="text-success">{t('setup.badge_installed')}</Badge>
                          {:else if id === 'engine'}
                            <Badge variant="secondary">{t('setup.badge_recommended')}</Badge>
                          {:else if cat === 'integrations'}
                            <Badge variant="outline">{blocked ? t('setup.badge_not_found') : t('setup.badge_detected')}</Badge>
                          {:else if id === 'agents' || id === 'cli'}
                            <Badge variant="outline">{t('setup.badge_optional')}</Badge>
                          {/if}
                        </span>
                        <span class="text-xs text-muted-foreground">
                          {#if id === 'engine'}{t(`setup.engine_reason_${profile.accelerator}` as TKey)}
                          {:else if id === 'cli'}{t('setup.item_cli_desc')}
                          {:else if id === 'agents' && blocked}{agentsPlatformOk ? t('setup.agents_low_memory') : t('setup.agents_platform')}
                          {/if}
                        </span>
                      </span>
                      {#if id === 'engine' || id === 'model' || id === 'agents'}
                        <span class="shrink-0 text-xs tabular-nums text-muted-foreground">
                          {#if done}{t('setup.badge_installed')}{:else}{sizeText(sizes[id])}{/if}
                        </span>
                      {/if}
                    </label>
                  </li>
                {/each}
              </ul>
              {#if cat === 'integrations'}
                <p class="text-xs text-muted-foreground">{t('setup.integrations_note')}</p>
              {/if}
            </div>
          {/each}

          <div class="flex flex-col gap-2 rounded-xl border border-border bg-card p-4">
            <h3 class="flex items-center gap-2 text-sm font-semibold"><FolderOpen class="size-4 text-primary" />{t('setup.cat_storage')}</h3>
            <div class="flex items-start gap-2">
              <div class="flex min-w-0 flex-1 flex-col gap-0.5">
                <span class="text-xs text-muted-foreground">{t('setup.storage_models')}{freeDiskBytes ? ` · ${t('setup.storage_free', { size: formatBytes(freeDiskBytes, 1) })}` : ''}</span>
                <output class="font-mono text-xs break-all" data-testid="setup-models-dir">{modelsDir}</output>
              </div>
              <Button variant="outline" size="sm" onclick={pickModelsFolder}>{t('setup.storage_change')}</Button>
            </div>
            {#if ctx}
              <div class="flex flex-col gap-0.5 border-t border-border pt-2">
                <span class="text-xs text-muted-foreground">{t('setup.storage_data')}{ctx.data_dir_fixed ? ` · ${t('setup.storage_data_fixed')}` : ''}</span>
                <output class="font-mono text-xs break-all" data-testid="setup-data-dir">{ctx.data_dir}</output>
              </div>
            {/if}
            {#if storageError}<p class="text-xs text-destructive" role="alert">{storageError}</p>{/if}
          </div>
        </section>
      {:else if screen === 'install'}
        <section class="flex flex-col gap-4" in:fly={{ x: 16, duration: dur(200) }}>
          <div>
            <h2 class="text-base font-semibold">{t('setup.progress_title')}</h2>
            <p class="mt-1 text-sm text-muted-foreground">{t('setup.progress_desc')}</p>
          </div>
          <Progress value={Math.round(runProgress(items) * 100)} max={100} class="h-2" aria-label={t('setup.progress_title')} />
          <ul class="flex flex-col gap-2" aria-live="polite">
            {#each items as item (item.id)}
              <li class="flex flex-col gap-1.5 rounded-lg border p-3 {item.state === 'failed' ? 'border-destructive/30 bg-destructive/5' : 'border-border bg-card'}" data-testid="setup-run-{item.id}" data-state={item.state}>
                <div class="flex items-center gap-3 text-sm">
                  {#if item.state === 'done'}<CheckCircle class="size-4 text-primary" />
                  {:else if item.state === 'running'}<Loader class="size-4 animate-spin text-primary" />
                  {:else if item.state === 'failed'}<Warning class="size-4 text-destructive" />
                  {:else if item.state === 'skipped'}<MinusCircle class="size-4 text-muted-foreground" />
                  {:else if item.state === 'cancelled'}<XCircle class="size-4 text-muted-foreground" />
                  {:else}<Circle class="size-4 text-muted-foreground/50" />{/if}
                  <span class="min-w-0 flex-1 truncate {item.state === 'pending' ? 'text-muted-foreground' : 'font-medium'}">{itemName(item.id)}</span>
                  <span class="text-xs text-muted-foreground">{t(`setup.state_${item.state}` as TKey)}</span>
                </div>
                {#if item.state === 'running' && item.fraction !== null}
                  <Progress value={Math.round(item.fraction * 100)} max={100} class="h-1" aria-label={itemName(item.id)} />
                {/if}
                {#if item.detail}
                  <p class="text-xs break-words {item.state === 'failed' ? 'text-destructive' : 'text-muted-foreground'}" role={item.state === 'failed' ? 'alert' : undefined}>{item.detail}</p>
                {/if}
              </li>
            {/each}
          </ul>
          {#if finished && cancelled}
            <p class="text-sm text-muted-foreground" role="status">{t('setup.progress_cancelled')}</p>
          {/if}
        </section>
      {:else}
        <section class="flex flex-col gap-4" in:fly={{ x: 16, duration: dur(200) }}>
          <div class="flex items-start gap-3">
            {#if failed.length}<Warning class="mt-0.5 size-6 text-destructive" />{:else}<CheckCircle class="mt-0.5 size-6 text-primary" />{/if}
            <div>
              <h2 class="text-base font-semibold">{failed.length ? t('setup.done_partial_title') : t('setup.done_title')}</h2>
              <p class="mt-1 text-sm text-muted-foreground">
                {#if failed.length}{t('setup.done_partial_desc')}
                {:else if items.length === 0}{t('setup.done_nothing')}
                {:else}{t('setup.done_desc')}{/if}
              </p>
            </div>
          </div>
          {#if items.length > 0}
            <ul class="flex flex-col gap-1.5 rounded-xl border border-border bg-card p-4 text-sm" in:fade={{ duration: dur(150) }}>
              {#each items as item (item.id)}
                <li class="flex items-center justify-between gap-3">
                  <span>{itemName(item.id)}</span>
                  <span class="text-xs {item.state === 'failed' ? 'text-destructive' : 'text-muted-foreground'}">{t(`setup.state_${item.state}` as TKey)}</span>
                </li>
              {/each}
            </ul>
          {/if}
          {#if finishError}<p class="text-xs text-destructive" role="alert">{finishError}</p>{/if}
        </section>
      {/if}
    </div>
  </main>

  <footer class="border-t border-border px-6 py-3">
    <div class="mx-auto flex w-full max-w-3xl items-center justify-between gap-2">
      {#if !ready || loadError}
        {#if onClose}<Button variant="ghost" onclick={onClose}>{t('setup.btn_close')}</Button>{:else}<span></span>{/if}
        <span></span>
      {:else if screen === 'welcome'}
        {#if onClose}
          <Button variant="ghost" onclick={onClose}>{t('setup.btn_close')}</Button>
        {:else}
          <Button variant="ghost" onclick={() => finish(false)} disabled={busy}>{t('setup.btn_skip')}</Button>
        {/if}
        <Button data-autofocus onclick={() => (screen = 'choose')}>{t('setup.btn_start')}</Button>
      {:else if screen === 'choose'}
        <Button variant="ghost" onclick={() => (screen = 'welcome')}>{t('setup.btn_back')}</Button>
        <div class="flex items-center gap-3">
          <span class="text-xs text-muted-foreground" data-testid="setup-total">
            {installedAll ? t('setup.total_none') : t('setup.total', { count: selected.size, size: download > 0 ? formatBytes(download, 1) : '0 B' })}
          </span>
          <Button data-autofocus onclick={start} data-testid="setup-install">
            {installedAll ? t('setup.btn_install_none') : t('setup.btn_install', { count: selected.size })}
          </Button>
        </div>
      {:else if screen === 'install'}
        {#if running}
          <Button variant="ghost" onclick={cancel} disabled={cancelled}>{t('setup.btn_cancel')}</Button>
          <span class="text-xs text-muted-foreground">{doneItems} / {items.length}</span>
        {:else}
          <Button variant="ghost" onclick={() => (screen = 'choose')}>{t('setup.btn_back')}</Button>
          <div class="flex items-center gap-2">
            <Button variant="outline" onclick={retry} disabled={retryIds(items).length === 0}>{t('setup.btn_retry')}</Button>
            <Button onclick={() => (screen = 'done')}>{failed.length ? t('setup.btn_continue_anyway') : t('setup.btn_continue')}</Button>
          </div>
        {/if}
      {:else}
        <Button variant="ghost" onclick={() => finish(false)} disabled={busy}>{t('setup.btn_finish')}</Button>
        <Button data-autofocus onclick={() => finish(true)} disabled={busy} data-testid="setup-checklist">{t('setup.btn_checklist')}</Button>
      {/if}
    </div>
  </footer>
</div>
