<script lang="ts">
  import { recommendedChatContext } from './dashboard/contextRecommendation';
  import type { PreparePlan } from './connections/agentTuning';
  import HelpPopover from '#lib/components/common/HelpPopover.svelte';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { countHiddenParams } from './dashboard/hiddenParams';
  import { classifyLevel } from './dashboard/logFilter';
  import AdvancedOnly from '#lib/components/common/AdvancedOnly.svelte';
  import type { HardwareFitResult, LocalModel, ServerStatus, AppSettings, ModelPreset, StorageDirInfo, SystemHardwareInfo, LaunchConfig, ServerProfile, ModelDefinition, DefinitionImport, ModelfileImport, RecoveryOption } from '#lib/types.js';
  import { onMount, untrack } from 'svelte';
  import { definitionsEpoch, invoke, invokeShared } from '#lib/ipc.js';
  import { estimateLaunchFit, toHardwareFit, vramSpillRisk } from '#lib/fit.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import CoachMarks from './CoachMarks.svelte';
  import ModelSelector from './dashboard/ModelSelector.svelte';
  import { isSpeechDefinition, isOnnxCtcDefinition } from './playground/transcribe';
  import LaunchParams from './dashboard/LaunchParams.svelte';
  import AdvancedParams from './dashboard/AdvancedParams.svelte';
  import MetadataFixNotice from './dashboard/MetadataFixNotice.svelte';
  import RouterModels from './servers/RouterModels.svelte';
  import RuntimePicker from './dashboard/RuntimePicker.svelte';
  import EngineParams from './dashboard/EngineParams.svelte';
  import { engineLabel } from '#lib/engines.js';
  import ServerActions from './dashboard/ServerActions.svelte';
  import TelemetryPanel from './dashboard/TelemetryPanel.svelte';
  import EffectiveValues from './dashboard/EffectiveValues.svelte';
  import LoraLiveScales from './dashboard/LoraLiveScales.svelte';
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import { runtimesStore } from '#lib/stores/runtimes.svelte.js';
  import LogConsole from './dashboard/LogConsole.svelte';
  import { serverLogs } from '#lib/stores/serverLogs.svelte.js';
  import { logFilter } from '#lib/stores/serverFilter.svelte.js';
  import MigrationModal from './dashboard/MigrationModal.svelte';
  import SaveProfileModal from './dashboard/SaveProfileModal.svelte';
  import ImportModal from './ImportModal.svelte';
  import type { LocalModelExt } from '#lib/import-types.js';
  import { initialParams, paramsFromPreset, presetFromParams, type LaunchParamsForm } from './dashboard/params';
  import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
  import { holdReleaseWhile, onWindowActive, windowActive } from '#lib/windowActivity.js';
  import { listen } from '@tauri-apps/api/event';
  import { library, normPath } from '#lib/stores/library.svelte.js';
  import { isAutoPort, makeProfileId, portOwner, savedPortText } from './servers/utils';
  import AdvancedPageNotice from './common/AdvancedPageNotice.svelte';
  import { pageLevel } from '#lib/uiLevel.js';
  import SectionSidebar from './common/SectionSidebar.svelte';
  import { DASHBOARD_GROUPS } from './common/screenSections';
  import { SectionSelection, requestSection } from '#lib/stores/sectionPage.svelte.js';
  const section = new SectionSelection('dashboard', DASHBOARD_GROUPS.flatMap(g => g.pages.map(p => p.id)), undefined, undefined, DASHBOARD_GROUPS);

  import VerticalSplit from './common/VerticalSplit.svelte';
  import { panes, viewportWidth } from '#lib/stores/panes.svelte.js';
  import Field from './dashboard/parts/Field.svelte';
  import { Input } from '#lib/components/ui/input/index.js';
  import { toast } from 'svelte-sonner';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Plus } from '@lucide/svelte';
  import { cn } from '#lib/utils.js';
  import { domain, samePath, type DefinitionAction } from '#lib/stores/domain.svelte.js';
  import { unsavedLoad } from '#lib/stores/unsavedLoad.svelte.js';
  import SpillConfirmDialog from './dashboard/SpillConfirmDialog.svelte';
  import DefinitionSaveBar from './dashboard/DefinitionSaveBar.svelte';
  import DefinitionDialog from './dashboard/DefinitionDialog.svelte';
  import DeleteDefinitionDialog from './dashboard/DeleteDefinitionDialog.svelte';
  import ImportDefinitionDialog from './dashboard/ImportDefinitionDialog.svelte';
  import ImportModelfileDialog from './dashboard/ImportModelfileDialog.svelte';
  import OllamaModelfilePicker from './dashboard/OllamaModelfilePicker.svelte';

  interface Props {
    settings: AppSettings;
    serverStatus: ServerStatus;
    hardware: SystemHardwareInfo | null;
    onGoToHub: () => void;
    onGoToServers: () => void;
    /** Opens the Connections tab (MCP servers are configured there). */
    onGoToConnections: () => void;
    /** The Dashboard is the shown tab (it stays mounted while hidden); the tour waits for this. */
    active?: boolean;
  }

  let { settings, serverStatus, hardware, onGoToHub, onGoToConnections, active = true }: Props = $props();

  let models = $state<LocalModel[]>([]);
  let selectedModelId = $state<string>('');
  /** Resident model servers, including ONNX CTC; job engines and companion files stay out. */
  let launchableModels = $derived(models.filter((m) => (m.kind === 'asr' && m.architecture === 'onnx') || !['asr', 'tts', 'image', 'adapter', 'mmproj'].includes(m.kind ?? '')));
  /** Single source of truth for every launch parameter (bound into LaunchParams/AdvancedParams). */
  let params = $state<LaunchParamsForm>(initialParams());
  let port = $state<number>(0);
  /** Harness picks the port at start (`port` = the last pick, 0 = none yet). */
  let portAuto = $state<boolean>(true);
  let routerMode = $state<boolean>(false);
  let host = $state<string>('127.0.0.1');
  /** Name / purpose of the server being configured ('' name = auto "<model> :<port>"). */
  let serverName = $state<string>('');
  let serverDesc = $state<string>('');
  let modelsLoaded = $state<boolean>(false);
  const nameId = $props.id();

  // ---- Model definitions ----
  /** Definition the form belongs to (null = a file without definition, or router mode). */
  let selectedDefinitionId = $state<string | null>(null);
  /** Model-level settings of the definition as loaded (for "unsaved changes"). */
  let baseline = $state<Record<string, string> | null>(null);
  let defBusy = $state(false);
  let editDef = $state<{ def: ModelDefinition; focusAlias: boolean } | null>(null);
  let deleteDef = $state<ModelDefinition | null>(null);
  let importPreview = $state<DefinitionImport | null>(null);
  let modelfilePreview = $state<ModelfileImport | null>(null);
  let ollamaPicker = $state(false);
  let selectedDefinition = $derived(routerMode ? undefined : domain.definition(selectedDefinitionId));

  /**
   * Servers whose log the console can show: every running one, plus the controlled one even
   * when it is stopped (its last run's lines are still in its buffer). The console's chips
   * narrow it down; "all" is the default.
   */
  let logSources = $derived.by(() => {
    const ids = app.activeServers.map((s) => s.id);
    return ids.includes(app.selectedServerId) ? ids : [...ids, app.selectedServerId];
  });
  let shownLogIds = $derived(logFilter.effective(logSources));
  let logs = $derived(serverLogs.merged(shownLogIds));
  // Only the shown servers are read (cursor-based, pushed); the others are dropped.
  $effect(() => {
    const ids = shownLogIds;
    untrack(() => serverLogs.want(ids));
  });
  let isScanning = $state<boolean>(false);
  let actionError = $state<string | null>(null);
  let showCoachMarks = $state<boolean>(false);
  let showSaveProfile = $state<boolean>(false);
  let profileConfig = $state<LaunchConfig | null>(null);
  let profileMsg = $state<string | null>(null);

  // Storage & Migration state
  let storageDirs = $state<StorageDirInfo[]>([]);
  let showMigrationModal = $state<boolean>(false);
  let showImportModal = $state<boolean>(false);

  // The tour opens the first time the Dashboard is shown after the first-run wizard is done. The
  // Dashboard is already mounted behind the wizard, and the wizard lands on Playground or Model
  // Hub, so a mount-time check never fired on first run (only on a later launch).
  let tourChecked = false;
  $effect(() => {
    if (tourChecked || !active || !settings.privacy.onboarding_completed) return;
    tourChecked = true;
    try {
      if (uiLevel.advanced && !localStorage.getItem('harness_tour_dismissed')) showCoachMarks = true;
    } catch {
      // storage unavailable
    }
  });

  // Runs once on mount (not on every settings change, which used to reset the port and rescan).
  onMount(() => {
    void domain.refreshPresets();
    refreshModels();
    port = settings.server.port || 0;
    portAuto = !settings.server.port;

    // New log lines are pushed (`server-logs`, coalesced); nothing is fetched while the window
    // is hidden, and everything missed is fetched when it shows again.
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<string[]>('server-logs', (e) => {
      if (windowActive()) void serverLogs.pull(e.payload);
    })
      .then((fn) => (disposed ? fn() : (unlisten = fn)))
      .catch(() => {});
    const offActive = onWindowActive(() => {
      if (windowActive()) void serverLogs.pullAll();
    });

    return () => {
      disposed = true;
      unlisten?.();
      offActive();
    };
  });

  // Switching the controlled server: drop the previous server's logs/telemetry and load
  // what we know about the new one (its profile's launch config, or its live port).
  let lastServerId: string | null = null;
  // Booleans, so the form reloads only when they flip: a later profile-list refresh or a
  // re-read of the Dashboard server's saved launch never overwrites what is being edited.
  let selectedHasProfile = $derived(app.profiles.some((p) => p.id === app.selectedServerId));
  let defaultServerKnown = $derived(app.defaultServer !== null);
  $effect(() => {
    const id = app.selectedServerId;
    void selectedHasProfile; // reload once the profile list arrives
    void defaultServerKnown; // ...and once the Dashboard server's saved launch arrives
    untrack(() => {
      if (lastServerId !== null && lastServerId !== id) {
        actionError = null;
        profileMsg = null;
      }
      lastServerId = id;
      loadServerIntoForm(id);
      // A running server wins over the saved profile / preset: show what it actually runs.
      if (models.length > 0) void syncFromRunning(id);
    });
  });

  // Settings > Storage can add, remove or change library folders while the Dashboard stays mounted
  // (tabs are kept alive). Without a rescan the move dialog kept offering the old folder list
  // ("no other storage folder found") right after a folder was added.
  let seenStorageKey: string | null = null;
  $effect(() => {
    const key = `${settings.models_dir}\n${settings.extra_models_dirs.join('\n')}`;
    const unchanged = seenStorageKey === null || key === seenStorageKey;
    seenStorageKey = key;
    // The first run is the mount (onMount already scans).
    if (unchanged) return;
    untrack(() => void refreshModels());
  });

  // A download finished (or files changed): rescan; the current selection is kept.
  let seenLibraryVersion = library.version;
  $effect(() => {
    const v = library.version;
    if (v === seenLibraryVersion) return;
    seenLibraryVersion = v;
    untrack(() => void refreshModels());
  });

  // Definition files can change through CLI refreshes or the definition editor.
  let seenDefinitionPaths: string | null = null;
  $effect(() => {
    const key = domain.definitions.flatMap((d) => d.base.kind === 'local' ? [d.base.path] : []).sort().join('\n');
    if (seenDefinitionPaths !== null && key !== seenDefinitionPaths) {
      seenDefinitionPaths = key;
      untrack(() => void refreshModels());
    } else if (seenDefinitionPaths === null) {
      seenDefinitionPaths = key;
    }
  });

  /** "Open in Dashboard": select the model, in a new server form when asked (a running server stays untouched). */
  async function selectPending(id: string) {
    const asNewServer = library.pendingNewServer;
    library.pendingNewServer = false;
    await selectModel(id);
    if (asNewServer) await createNewServer();
  }

  // "Open in Dashboard" pressed while the Dashboard is already showing.
  $effect(() => {
    const pending = library.pendingSelectId;
    if (pending && models.some((m) => m.id === pending)) {
      library.pendingSelectId = null;
      untrack(() => void selectPending(pending));
    }
  });

  /**
   * Loads the model and launch settings the server `id` is actually running (its launch config)
   * into the form. Returns false when that server is not running, so the caller can fall back to
   * the preset / first model.
   */
  async function syncFromRunning(id: string): Promise<boolean> {
    const active = app.servers.find((s) => s.id === id)?.active;
    if (!active) return false;
    let cfg: LaunchConfig | null = null;
    try {
      cfg = await invoke<LaunchConfig | null>('get_server_launch_config', { serverId: id });
    } catch {
      cfg = null;
    }
    if (!cfg || id !== app.selectedServerId) return false;
    const runPath = normPath(cfg.model_path);
    const model = models.find((m) => normPath(m.path) === runPath || m.shard_paths?.some((p) => normPath(p) === runPath));
    if (!cfg.router_mode && !model) return false;
    selectToken++; // drop an in-flight preset load for another model
    nglToken++; // and a pending -ngl recommendation
    routerMode = !!cfg.router_mode;
    if (model && !cfg.router_mode) selectedModelId = model.id;
    params = paramsFromPreset(cfg, model, settings);
    port = cfg.port;
    portAuto = isAutoPort(cfg);
    host = cfg.host || settings.server.host || '127.0.0.1';
    selectedDefinitionId = cfg.router_mode ? null : (cfg.definition_id ?? null);
    void loadBaseline(selectedDefinitionId);
    return true;
  }

  // The selected server (re)started with another port or model than the form shows (a start
  // from the CLI, a Playground/Library load, a retry): the form shows what really runs. Only a
  // new run triggers this, so a port typed for the next start is kept while the server runs.
  let runKey = $derived.by(() => {
    const a = app.selectedServer?.active;
    return a ? `${app.selectedServerId}|${a.port}|${a.router_mode}|${normPath(a.model_path)}` : '';
  });
  $effect(() => {
    if (!runKey) return;
    untrack(() => {
      const a = app.selectedServer?.active;
      if (!a || models.length === 0) return;
      const m = selectedModel;
      const sameModel = a.router_mode
        ? routerMode
        : !routerMode && !!m && (samePath(m.path, a.model_path) || !!m.shard_paths?.some((p) => samePath(p, a.model_path)));
      if (!sameModel || (!portAuto && a.port !== port)) void syncFromRunning(app.selectedServerId);
      // An automatic port: the form just shows the one picked for this run.
      else if (portAuto) port = a.port;
    });
  });

  // Runtime capabilities of the binary the form's runtime selects (probed once per binary):
  // the parameter catalog is gated by that build's flags.
  // A runtime that is installed, added or fixed after the first probe (first-run wizard) changes
  // the registry: re-probe then, instead of keeping a stale "could not read" result.
  $effect(() => {
    const id = app.selectedServerId;
    const runtime = params.runtime || 'official';
    void runtimesStore.list.map((r) => `${r.id}|${r.available}|${r.binary_path ?? ''}|${r.version?.build ?? ''}`).join(',');
    untrack(() => capsStore.load(id, false, runtime));
  });
  onMount(() => void runtimesStore.load());

  // The model's engine (ONNX, whisper.cpp, sd.cpp, python-hf, MLX...) decides which settings the
  // form shows: llama-server's catalog only for a model llama.cpp runs.
  $effect(() => {
    const path = routerMode ? null : (selectedModel?.path ?? null);
    untrack(() => capsStore.loadEngine(path));
  });

  // Feature levels depend on the model, the mmproj and what the last run verified.
  $effect(() => {
    const id = app.selectedServerId;
    const modelPath = routerMode ? null : (selectedModel?.path ?? null);
    const hasMmproj = !!params.mmproj_path;
    const running = serverStatus === 'Running';
    void capsStore.caps;
    untrack(() => capsStore.loadFeatures(id, modelPath, hasMmproj));
    if (!running) return;
    // Endpoint checks finish shortly after the server reports healthy.
    const timer = setTimeout(() => capsStore.loadFeatures(id, modelPath, hasMmproj), 4000);
    return () => clearTimeout(timer);
  });

  /** Library id of the model file a saved launch serves (the path itself when it is not scanned). */
  function modelIdFor(path: string): string {
    return models.find((m) => samePath(m.path, path) || m.shard_paths?.some((p) => samePath(p, path)))?.id ?? path;
  }

  /** Saved launch of a server: its profile, or the Dashboard server's last start. */
  function savedLaunchOf(id: string): LaunchConfig | null {
    const profile = app.profiles.find((p) => p.id === id);
    if (profile) return profile.config;
    if (id === DEFAULT_SERVER_ID && app.defaultServer?.saved) return app.defaultServer.config;
    return null;
  }

  /** The name Harness gives a server of this model: no port (it is shown next to the name). */
  function baseNameFor(modelId: string | null, router: boolean): string {
    return router ? 'Router' : (models.find((m) => m.id === modelId)?.name ?? 'Model');
  }

  /** Was the profile's name generated (model name, "Router", or those with a " (2)" number)? */
  function isAutoName(profile: ServerProfile, modelId: string | null, router: boolean): boolean {
    const base = baseNameFor(modelId ?? profile.model_id, router);
    const label = app.serverLabel(profile.id);
    return label === base || (label.startsWith(`${base} (`) && /^\(\d+\)$/.test(label.slice(base.length + 1)));
  }

  /** `base`, or `base (2)`... when another server already carries the name. */
  function uniqueName(base: string, exceptId: string | null): string {
    const taken = new Set(app.profiles.filter((p) => p.id !== exceptId).map((p) => app.serverLabel(p.id)));
    let name = base;
    for (let n = 2; taken.has(name); n++) name = `${base} (${n})`;
    return name;
  }

  /**
   * Loads what is saved for server `id` into the form: a profile (with its name and purpose),
   * or the Dashboard server's saved launch (its one source of truth: the "default" profile,
   * updated by every start). Only a server with nothing saved starts from the settings defaults.
   */
  function loadServerIntoForm(id: string) {
    const profile = app.profiles.find((p) => p.id === id);
    const cfg = savedLaunchOf(id);
    if (cfg) {
      selectToken++; // drop an in-flight preset load for another model
      nglToken++; // and a pending -ngl recommendation
      routerMode = !!cfg.router_mode;
      if (!cfg.router_mode) selectedModelId = profile?.model_id ?? modelIdFor(cfg.model_path);
      autoFilled = {};
      const formModel = models.find((m) => m.id === selectedModelId);
      params = paramsFromPreset(cfg, formModel, settings);
      port = cfg.port;
      portAuto = isAutoPort(cfg);
      host = cfg.host || settings.server.host || '127.0.0.1';
      // A saved launch that left the context or layer count automatic shows what Start loads
      // (the default-load rule), not the raw estimate; a running server keeps its own values.
      if (formModel && !cfg.router_mode && !app.servers.find((s) => s.id === id)?.active
        && (cfg.auto_fields?.includes('gpu_layers') || !cfg.context_size)) {
        recommendContext(formModel, true);
      }
      // A name Harness generated from the model keeps following the model; a typed one stays.
      serverName = profile && !isAutoName(profile, cfg.router_mode ? null : selectedModelId, !!cfg.router_mode) ? profile.name : '';
      serverDesc = profile?.description ?? '';
      selectedDefinitionId = cfg.router_mode ? null : (cfg.definition_id ?? null);
      void loadBaseline(selectedDefinitionId);
      return;
    }
    serverName = '';
    serverDesc = '';
    host = settings.server.host || '127.0.0.1';
    // Nothing saved: the Dashboard server starts from the settings default (automatic unless a
    // port was set there), any other server from an automatic port.
    const fallback = id === DEFAULT_SERVER_ID ? (app.defaultServer?.config ?? { port: settings.server.port }) : { port: 0 };
    portAuto = isAutoPort(fallback);
    const livePort = app.servers.find((s) => s.id === id)?.active?.port;
    port = livePort ?? fallback.port;
  }

  /**
   * First scan: restores the selected server's saved launch now that the model list is known.
   * False when nothing usable is saved (the caller then picks the first model).
   */
  async function restoreSaved(id: string): Promise<boolean> {
    if (id === DEFAULT_SERVER_ID) await app.defaultServerLoaded();
    if (id !== app.selectedServerId || !savedLaunchOf(id)) return false;
    loadServerIntoForm(id);
    return routerMode || models.some((m) => m.id === selectedModelId);
  }

  // The model page's own reads (registry record, runtime advice, metadata) wait until it was shown.
  let modelPageSeen = $state(false);
  $effect(() => {
    if (section.active === 'model') modelPageSeen = true;
  });

  // Directory changes can start scans back to back: only the newest one may set the list.
  let scanSeq = 0;
  async function refreshModels() {
    const seq = ++scanSeq;
    isScanning = true;
    try {
      const [res, dirs, definitions] = await Promise.all([
        invokeShared<LocalModel[]>('scan_local_models', undefined, library.version),
        invokeShared<StorageDirInfo[]>('get_storage_directories', undefined, library.version),
        domain.refreshDefinitions(),
      ]);
      seenDefinitionPaths = definitions.flatMap((d) => d.base.kind === 'local' ? [d.base.path] : []).sort().join('\n');
      const paths = definitions.flatMap((d) => {
        if (d.base.kind !== 'local') return [];
        const path = d.base.path;
        return res.some((m) => samePath(m.path, path) || m.shard_paths.some((p) => samePath(p, path))) ? [] : [path];
      });
      const outside = paths.length ? await invoke<LocalModel[]>('inspect_model_paths', { paths }) : [];
      if (seq !== scanSeq) return;
      const firstLoad = !modelsLoaded;
      models = [...res, ...outside.filter((m) => !res.some((known) => samePath(known.path, m.path)))
        .map((m) => ({ ...m, outside_scan: true }))];
      storageDirs = dirs;
      const pending = library.pendingSelectId;
      const pendingDef = library.pendingDefinitionId;
      if (pendingDef && domain.definition(pendingDef)) {
        // My Models: "default load settings" of a definition.
        library.pendingDefinitionId = null;
        library.pendingSelectId = null;
        section.select('model');
        await selectDefinition(pendingDef);
      } else if (pending && models.some((m) => m.id === pending)) {
        // "Open in Dashboard" from a finished download.
        library.pendingSelectId = null;
        section.select('model');
        await selectPending(pending);
      } else if (firstLoad && (await syncFromRunning(app.selectedServerId))) {
        // The selected server is running: the form already shows its model and settings.
      } else if (firstLoad && (await restoreSaved(app.selectedServerId))) {
        // The selected server's saved launch (after a restart too) is in the form.
      } else if (!selectedModelId) {
        // The first chat model the Dashboard can serve (embedding/reranker files only after).
        const first = launchableModels.find((m) => !m.kind || m.kind === 'llm') ?? launchableModels[0];
        if (first) await selectModel(first.id);
      }
    } catch (e: any) {
      toast.error(t('dash.n_scan_failed', { error: String(e) }));
    } finally {
      // A superseded scan leaves both to the newer one (`firstLoad` must still see the first).
      if (seq === scanSeq) {
        isScanning = false;
        modelsLoaded = true;
      }
    }
  }

  /** Removes a linked (Ollama / LM Studio) model from the list; throws the backend error (e.g. in use). */
  async function unlinkModel(modelId: string) {
    await invoke('unlink_external_model', { modelId });
    if (selectedModelId === modelId) selectedModelId = '';
    await refreshModels();
  }

  /** After a successful move: select the model under its new id and rescan. */
  async function handleModelMigrated(migratedId: string) {
    selectedModelId = migratedId;
    await refreshModels();
  }

  /** What the default load starts with (`automatic_launch_choice`, the launch's own rule). */
  interface AutomaticLaunch {
    context: number;
    layers: number;
    slots: number | null;
    cache_type_k: string | null;
    cache_type_v: string | null;
    flash_attn: boolean;
  }

  let selectToken = 0;

  /**
   * Settings whose current value was filled in by the form itself (the default-load rule, the
   * memory estimate), by form key. A start, profile or definition built from the form leaves
   * them automatic (`auto_fields`) while the value is still the one that was filled in; a value
   * the user changes by hand is theirs.
   */
  let autoFilled = $state<Record<string, number | string | boolean>>({});
  const AUTO_KEYS = ['context_size', 'gpu_layers', 'parallel_slots', 'cache_type_k', 'cache_type_v', 'flash_attn'] as const;
  function autoFieldsNow(): string[] {
    return AUTO_KEYS.filter((k) => k in autoFilled && (params as Record<string, unknown>)[k] === autoFilled[k]);
  }
  function markAuto(values: Record<string, number | string | boolean | null | undefined>) {
    const next = { ...autoFilled };
    for (const [k, v] of Object.entries(values)) if (v !== null && v !== undefined && v !== '') next[k] = v;
    autoFilled = next;
  }

  /** Applies a saved preset, or defaults (for every field) when there is none. */
  function applyPreset(preset: ModelPreset | null, model: LocalModel | undefined) {
    autoFilled = {};
    params = paramsFromPreset(preset, model, settings);
    // Linked Ollama / LM Studio models may come with a vision projector: suggest it.
    const mmproj = (model as LocalModelExt | undefined)?.mmproj_path;
    if (mmproj && !params.mmproj_path) params.mmproj_path = mmproj;
    else if (model && !params.mmproj_path) fillProjector(model);
    // No saved preset: start from the layer count that fits this model, not a fixed default.
    // (An integrated GPU where the CPU is faster recommends 0, not the default 33/99.) A value
    // typed while the estimate was on its way wins over it.
    if (preset?.gpu_layers == null && model) recommendNgl(model);
    if (preset?.context_size == null && model) recommendContext(model);
  }

  /**
   * A model with a matching projector file next to it (the library's pairing: same folder, same
   * basename or name) gets it in the empty projector field. A path typed or chosen meanwhile wins.
   */
  function fillProjector(model: LocalModel) {
    if (model.kind && model.kind !== 'llm') return;
    const token = selectToken;
    invoke<{ path: string } | null>('suggest_projector', { modelPath: model.path })
      .then((found) => {
        if (!found || token !== selectToken || selectedModelId !== model.id || userEdited || params.mmproj_path) return;
        params.mmproj_path = found.path;
      })
      .catch(() => {});
  }

  /** Saved contexts and edits made while the plan is pending always win. */
  function recommendContext(model: LocalModel, rebase = false) {
    if (model.kind && model.kind !== 'llm') return;
    const token = selectToken;
    const shown = params.context_size;
    const serverId = app.selectedServerId;
    const config = buildLaunchConfig(presetFromParams(params));
    const definition = domain.definitions.find(def => def.id === selectedDefinitionId);
    const layersSet = !!definition && definition.load.gpu_layers != null;
    const stale = () => token !== selectToken || selectedModelId !== model.id || serverId !== app.selectedServerId
      || params.context_size !== shown || userEdited;
    const fromPlan = () => invoke<PreparePlan>('plan_launch_context', { config, serverId }).then(plan => {
      if (stale()) return;
      params.context_size = recommendedChatContext(model.context_length, plan.choice?.context_size, shown);
      markAuto({ context_size: params.context_size });
      if (plan.choice) {
        if (!params.cache_type_k) {
          params.cache_type_k = plan.choice.cache_type_k;
          markAuto({ cache_type_k: params.cache_type_k });
        }
        if (!params.cache_type_v) {
          params.cache_type_v = plan.choice.cache_type_v;
          markAuto({ cache_type_v: params.cache_type_v });
        }
      }
      // Re-estimate offload for the newly recommended context, unless it was saved explicitly.
      if (!layersSet) recommendNgl(model, rebase);
      if (rebase) baseline = modelPart(params);
    }).catch(() => {});
    // The default load (context, layers and cache all automatic) starts with the core's rule:
    // the whole model on the GPU at a smaller context on a small card. Show exactly that.
    const automatic = layersSet || params.cache_type_k || params.cache_type_v
      ? Promise.resolve(null)
      : invoke<AutomaticLaunch | null>('automatic_launch_choice', { config, serverId }).catch(() => null);
    void automatic.then(choice => {
      if (!choice) return fromPlan();
      if (stale()) return;
      params.context_size = choice.context;
      params.gpu_layers = choice.layers;
      markAuto({ context_size: choice.context, gpu_layers: choice.layers });
      // The form shows what the automatic launch really uses: slots, KV cache types and the
      // flash attention they need. A value the user set is theirs and stays.
      if (choice.slots != null && !params.parallel_slots) {
        params.parallel_slots = choice.slots;
        markAuto({ parallel_slots: choice.slots });
      }
      if (choice.cache_type_k && choice.cache_type_v && !params.cache_type_k && !params.cache_type_v) {
        params.cache_type_k = choice.cache_type_k;
        params.cache_type_v = choice.cache_type_v;
        markAuto({ cache_type_k: choice.cache_type_k, cache_type_v: choice.cache_type_v });
      }
      if (choice.flash_attn && !params.flash_attn) {
        params.flash_attn = true;
        markAuto({ flash_attn: true });
      }
      if (rebase) baseline = modelPart(params);
    });
  }

  /** Replaces the default -ngl with the layer count that fits `model`, unless the user typed one meanwhile. */
  function recommendNgl(model: LocalModel, rebase = false) {
    const token = ++nglToken;
    const shown = params.gpu_layers;
    estimateLaunchFit(buildLaunchConfig(presetFromParams(params)), app.selectedServerId)
      .then((r) => {
        if (token === nglToken && selectedModelId === model.id && params.gpu_layers === shown) {
          params.gpu_layers = r.recommended_gpu_layers;
          markAuto({ gpu_layers: r.recommended_gpu_layers });
          // A loaded definition: the recommendation is its starting point, not an unsaved edit.
          if (rebase) baseline = modelPart(params);
        }
      })
      .catch(() => {});
  }

  let nglToken = 0;

  /** Selects a model file: its Dashboard definition when it has one, else the file alone. */
  async function selectModel(id: string) {
    if (id !== selectedModelId) await unsavedLoad.flush();
    userEdited = false;
    selectedModelId = id;
    const model = models.find((m) => m.id === id);
    if (!model) return;

    // Ignore responses for a model the user has already switched away from.
    const token = ++selectToken;
    let defId: string | null = null;
    try {
      defId = await invoke<string | null>('get_dashboard_definition', { modelId: model.id });
    } catch {
      defId = null;
    }
    if (token !== selectToken) return;
    if (defId) return selectDefinition(defId);
    let preset: ModelPreset | null = null;
    try {
      preset = await invoke<ModelPreset | null>('get_model_preset', { modelId: model.id });
    } catch {
      preset = null;
    }
    if (token === selectToken) {
      selectedDefinitionId = null;
      baseline = null;
      applyPreset(preset, model);
    }
  }

  // A definition made for the selected file after it was selected (a finished download makes
  // one): take it over while the form is untouched and no server runs from it.
  $effect(() => {
    void domain.definitions;
    untrack(() => {
      const id = selectedModelId;
      if (!id || selectedDefinitionId || userEdited || routerMode || app.selectedServer?.active) return;
      void invoke<string | null>('get_dashboard_definition', { modelId: id })
        .then((defId) => {
          if (defId && selectedModelId === id && !selectedDefinitionId && !userEdited) return selectDefinition(defId);
        })
        .catch(() => {});
    });
  });

  /** Loads a definition's settings into the form (its load + the file's Dashboard serve settings). */
  async function selectDefinition(id: string) {
    userEdited = false;
    const token = ++selectToken;
    let cfg: LaunchConfig;
    try {
      cfg = await invokeShared<LaunchConfig>('get_definition_form', { definitionId: id }, definitionsEpoch());
    } catch (e) {
      toast.error(String(e));
      return;
    }
    if (token !== selectToken) return;
    const model = models.find(
      (m) => samePath(m.path, cfg.model_path) || m.shard_paths?.some((p) => samePath(p, cfg.model_path))
    );
    if (!model) {
      toast.error(t('def.file_missing'));
      return;
    }
    selectedModelId = model.id;
    selectedDefinitionId = id;
    nglToken++; // the definition's own -ngl wins over a pending recommendation
    autoFilled = {};
    params = paramsFromPreset(cfg, model, settings);
    baseline = modelPart(params);
    // A definition without its own -ngl (e.g. made right after a download) gets the fitting one.
    if (domain.definitions.find((d) => d.id === id)?.load.gpu_layers == null) recommendNgl(model, true);
    if (domain.definitions.find((d) => d.id === id)?.load.context_size == null) recommendContext(model, true);
  }

  /**
   * The edited definition was saved. A form that belongs to a server (registered or running)
   * keeps that server's own values: only the definition's files and template are taken over
   * and the baseline follows. Without a server behind the form it is reloaded from the definition.
   */
  async function refreshAfterDefinitionSaved(id: string) {
    const serverBound = savedLaunch !== null || !!app.selectedServer?.active;
    if (!serverBound || selectedDefinitionId !== id) return selectDefinition(id);
    try {
      const cfg = await invokeShared<LaunchConfig>('get_definition_form', { definitionId: id }, definitionsEpoch());
      if (selectedDefinitionId !== id) return;
      const model = models.find((m) => samePath(m.path, cfg.model_path));
      const fresh = paramsFromPreset(cfg, model, settings);
      params.mmproj_path = fresh.mmproj_path;
      params.draft_model_path = fresh.draft_model_path;
      params.draft_gpu_layers = fresh.draft_gpu_layers;
      params.lora_path = fresh.lora_path;
      params.lora_scale = fresh.lora_scale;
      params.chat_template = fresh.chat_template;
      baseline = modelPart(fresh);
    } catch (e) {
      toast.error(String(e));
    }
  }

  /** Model-level settings of the form (what a definition stores), as comparable strings. */
  function modelPart(p: LaunchParamsForm): Record<string, string> {
    const preset = presetFromParams($state.snapshot(p) as LaunchParamsForm);
    const skip = new Set(['api_key', 'api_key_ref', 'mcp_enabled', 'params']);
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(preset)) {
      if (skip.has(k) || v === null || v === undefined || v === '') continue;
      out[k] = JSON.stringify(v);
    }
    for (const [k, v] of Object.entries(preset.params ?? {})) {
      const group = capsStore.spec(k)?.group;
      if (group !== 'server' && group !== 'router') out[`p.${k}`] = JSON.stringify(v);
    }
    return out;
  }

  /** Baseline of a definition without touching the form (the form shows a running server). */
  async function loadBaseline(id: string | null) {
    if (!id) {
      baseline = null;
      return;
    }
    try {
      const cfg = await invokeShared<LaunchConfig>('get_definition_form', { definitionId: id }, definitionsEpoch());
      if (selectedDefinitionId !== id) return;
      const model = models.find((m) => samePath(m.path, cfg.model_path));
      baseline = modelPart(paramsFromPreset(cfg, model, settings));
    } catch {
      baseline = null;
    }
  }

  /** Setting ids (form keys) that differ from the definition. */
  let changedKeys = $derived.by(() => {
    const base = baseline;
    if (!base || !selectedDefinitionId || routerMode) return [] as string[];
    const now = modelPart(params);
    const keys = new Set([...Object.keys(now), ...Object.keys(base)]);
    const auto = new Set(autoFieldsNow());
    return [...keys].filter((k) => now[k] !== base[k] && !auto.has(k));
  });

  let definitionUsers = $derived(
    selectedDefinitionId
      ? app.profiles.filter((p) => p.config.definition_id === selectedDefinitionId).map((p) => p.name)
      : []
  );

  /** "Varsayılan olarak kaydet" (target) / "Yeni tanım olarak kaydet" (alias). */
  async function saveToDefinition(target: string | null, alias: string | null) {
    if (defBusy || routerMode || !selectedModel) return;
    defBusy = true;
    try {
      const config = $state.snapshot(buildLaunchConfig()) as LaunchConfig;
      const def = await invoke<ModelDefinition>('save_definition_form', { definitionId: target, alias, config });
      unsavedLoad.clear('dashboard');
      await domain.refreshDefinitions();
      await selectDefinition(def.id);
      toast.success(target ? t('def.saved_default', { alias: def.alias }) : t('def.saved_new', { alias: def.alias }));
    } catch (e) {
      toast.error(String(e));
    } finally {
      defBusy = false;
    }
  }

  async function onDefinitionAction(action: DefinitionAction) {
    const def = selectedDefinition;
    switch (action) {
      case 'new': {
        if (!selectedModel) return;
        const taken = new Set(domain.definitions.map((d) => d.alias.toLowerCase()));
        const stem = selectedModel.name.replace(/[^A-Za-z0-9._:-]+/g, '-').replace(/^-+|-+$/g, '') || 'model';
        let alias = stem;
        for (let n = 2; taken.has(alias.toLowerCase()); n++) alias = `${stem}-${n}`;
        editDef = {
          def: { id: '', alias, base: { kind: 'local', path: selectedModel.path }, load: {}, created_at: 0, updated_at: 0 },
          focusAlias: true,
        };
        return;
      }
      case 'duplicate':
        if (!def) return;
        try {
          const copy = await invoke<ModelDefinition>('duplicate_model_definition', { id: def.id, alias: null });
          await domain.refreshDefinitions();
          await selectDefinition(copy.id);
          editDef = { def: copy, focusAlias: true };
        } catch (e) {
          toast.error(String(e));
        }
        return;
      case 'edit':
      case 'rename':
        if (def) editDef = { def, focusAlias: action === 'rename' };
        return;
      case 'export':
        if (!def) return;
        try {
          const path = await invoke<string | null>('export_model_definition', { id: def.id, embedPreset: true });
          if (path) toast.success(t('def.exported', { path }));
        } catch (e) {
          toast.error(String(e));
        }
        return;
      case 'import':
        try {
          importPreview = await invoke<DefinitionImport | null>('preview_model_definition_import');
        } catch (e) {
          toast.error(t('def.import_failed', { error: String(e) }));
        }
        return;
      case 'import_modelfile':
        try {
          modelfilePreview = await invoke<ModelfileImport | null>('preview_modelfile_import');
        } catch (e) {
          toast.error(t('def.import_failed', { error: String(e) }));
        }
        return;
      case 'import_ollama':
        ollamaPicker = true;
        return;
      case 'delete':
        if (def) deleteDef = def;
        return;
    }
  }

  // Profiles deleted elsewhere may take their auto-created definitions with them.
  let profilesSig = $derived(app.profiles.map((p) => `${p.id}:${p.config.definition_id ?? ''}`).join('|'));
  $effect(() => {
    void profilesSig;
    untrack(() => void domain.refreshDefinitions());
  });

  let selectedModel = $derived(models.find((m) => m.id === selectedModelId));

  // Rust estimate of the current form (debounced: sliders fire on every step).
  let fitResult = $state<HardwareFitResult | null>(null);
  let fitConfig = $derived(selectedModel ? buildLaunchConfig() : null);
  let fitKey = $derived(fitConfig ? JSON.stringify(fitConfig) : '');
  $effect(() => {
    const key = fitKey;
    const config = untrack(() => fitConfig);
    void hardware;
    if (!config) {
      fitResult = null;
      return;
    }
    const timer = setTimeout(() => {
      estimateLaunchFit(config, app.selectedServerId)
        .then((r) => {
          if (key === untrack(() => fitKey)) fitResult = toHardwareFit(r);
        })
        .catch(() => {});
    }, 150);
    return () => clearTimeout(timer);
  });

  let recommendedNgl = $derived(fitResult?.recommendedGpuLayers ?? null);
  let maxGpuLayers = $derived(Math.max(99, (selectedModel?.gguf?.block_count ?? 0) + 1));
  let readyModelCount = $derived(
    models.filter((m) => m.is_ready && (!routerMode || !m.kind || ['llm', 'embedding', 'reranker'].includes(m.kind))).length
  );

  let selectedProfile = $derived(app.profiles.find((p) => p.id === app.selectedServerId) ?? null);
  let isDefaultServer = $derived(app.selectedServerId === DEFAULT_SERVER_ID);
  /** What is saved for the selected server (profile or the Dashboard server's last start), null = nothing. */
  let savedLaunch = $derived(selectedProfile?.config ?? (isDefaultServer && app.defaultServer?.saved ? app.defaultServer.config : null));

  /**
   * Comparable form of a launch: both sides go through the same form conversion, so values
   * the form cannot show (or shows differently) never count as a change. The API key is left
   * out (a typed key is never shown again once saved).
   */
  function launchSignature(cfg: LaunchConfig): string {
    const model = cfg.router_mode ? undefined : models.find((m) => samePath(m.path, cfg.model_path));
    const { api_key: _key, api_key_ref: _ref, params: catalog, ...preset } = presetFromParams(paramsFromPreset(cfg, model, settings));
    return JSON.stringify({
      ...preset,
      params: Object.fromEntries(Object.entries(catalog ?? {}).sort(([a], [b]) => a.localeCompare(b))),
      router_mode: !!cfg.router_mode,
      model: cfg.router_mode ? '' : normPath(cfg.model_path ?? ''),
      definition_id: cfg.router_mode ? null : (cfg.definition_id ?? null),
      host: cfg.host || settings.server.host || '127.0.0.1',
      // An automatic port is one setting whatever it picked last time.
      port: isAutoPort(cfg) ? 'auto' : cfg.port,
    });
  }

  /** The form matches what is saved for this server ("Saved"); otherwise "Not saved". */
  let formSaved = $derived.by(() => {
    const saved = savedLaunch;
    if (!saved) return false;
    return launchSignature(saved) === launchSignature($state.snapshot(buildLaunchConfig()) as LaunchConfig);
  });
  /**
   * The user typed or changed a field by hand since the form was last loaded (model, definition,
   * new server) or saved. Picking a model alone is not a draft: it is reloaded on the next visit.
   */
  let userEdited = $state(false);
  $effect(() => {
    if (formSaved) userEdited = false;
  });
  /** Input / change / chip clicks inside the editable sections (disclosure toggles excluded). */
  function markEdited(e: Event) {
    if (e.type === 'click' && (e.target as HTMLElement | null)?.closest('[aria-expanded]')) return;
    if (e.type === 'click' && !(e.target as HTMLElement | null)?.closest('button')) return;
    userEdited = true;
  }
  /**
   * Hand-made edits that differ from the saved launch: only these show "Not saved" and keep the
   * page loaded while hidden to the tray. A model picked without edits stays "Saved".
   */
  let formDraft = $derived(savedLaunch !== null && !formSaved && userEdited);
  $effect(() => holdReleaseWhile('dashboard-form', () => formDraft));

  let savedStateTip = $derived(
    savedLaunch && !formDraft
      ? isDefaultServer
        ? t('srv.default_saved_tip')
        : t('srv.card_saved')
      : isDefaultServer
        ? t('srv.default_unsaved_tip')
        : selectedProfile
          ? t('srv.profile_unsaved_tip')
          : t('ui.srv_unsaved_tip')
  );
  // No port in the name: the port is always shown next to it.
  let autoName = $derived(uniqueName(baseNameFor(routerMode ? null : selectedModelId, routerMode), selectedProfile?.id ?? null));
  let effectiveName = $derived(serverName.trim() || autoName);

  /** Port the selected server listens on right now (null = not running). */
  let runningPort = $derived(app.activeServer?.port ?? null);
  /** Saved port shown next to "Not saved" when the form's port differs from it. */
  let savedPortNote = $derived(
    formDraft && savedLaunch && !portAuto && !isAutoPort(savedLaunch) && savedLaunch.port !== port ? savedLaunch.port : null,
  );

  /** Another running instance of ours already listening on the chosen port. */
  let portConflict = $derived(portAuto ? null : portOwner(app.servers, port, app.selectedServerId));
  let portWarning = $derived(portConflict ? t('srv.port_in_use', { label: app.serverLabel(portConflict.id) }) : null);

  /** Why the current form can't be launched, or null when it can. */
  function launchError(): string | null {
    if (!routerMode) {
      if (!selectedModel) return t('dash.err_select_model');
      if (!selectedModel.is_ready) return t('dash.err_incomplete_model');
    } else if (readyModelCount === 0) {
      return t('dash.err_router_no_ready');
    }
    return null;
  }

  /** The exact LaunchConfig `start_server` receives (also what a saved profile stores). */
  function buildLaunchConfig(preset: ModelPreset = presetFromParams(params)): LaunchConfig {
    return {
      ...preset,
      auto_fields: routerMode ? [] : autoFieldsNow(),
      model_path: selectedModel ? selectedModel.path : settings.models_dir,
      router_mode: routerMode,
      models_path: null,
      host,
      port: port || 0,
      port_auto: portAuto,
      // The backend layers the form over this definition (and serves its alias).
      definition_id: routerMode ? null : selectedDefinitionId,
    };
  }

  let busy = false;

  // A start error about the port goes stale once the port setting changes.
  $effect(() => {
    void port;
    void portAuto;
    untrack(() => {
      actionError = null;
    });
  });

  /** Dedicated VRAM is exceeded: ask before starting (the driver may spill into system RAM). */
  let spillAsk = $state(false);
  /** The same launch with automatic KV precision, when that fits (offered in the dialog). */
  let autoKvFit = $state<HardwareFitResult | null>(null);

  /** The automatic-KV estimate before the dialog is in flight (a second click waits for it). */
  let spillChecking = false;

  async function handleStart() {
    if (busy || spillChecking || spillAsk) return;
    if (!routerMode && !launchError() && vramSpillRisk(fitResult, params.gpu_layers)) {
      spillChecking = true;
      autoKvFit = null;
      const key = fitKey;
      const server = app.selectedServerId;
      // A fixed KV precision (e.g. "Full") that overflows: automatic precision may fit.
      if (fitConfig && (params.cache_type_k || params.cache_type_v)) {
        try {
          const auto = toHardwareFit(
            await estimateLaunchFit({ ...fitConfig, cache_type_k: null, cache_type_v: null }, server),
          );
          if (auto.kv && !vramSpillRisk(auto, params.gpu_layers)) autoKvFit = auto;
        } catch {
          // No estimate: the dialog offers the other choices.
        }
      }
      spillChecking = false;
      // The form or the server changed while the estimate ran: this answer is for another launch.
      if (key !== fitKey || server !== app.selectedServerId) {
        autoKvFit = null;
        return;
      }
      spillAsk = true;
      return;
    }
    void startNow();
  }

  /**
   * Load settings changed by hand and used by the start that just succeeded, but saved nowhere:
   * leaving the screen, switching model, stopping the server or closing the app asks once
   * whether they become the model's default. What the form filled in itself (the default-load
   * rule, the estimate) is not a manual change.
   */
  function registerUnsavedLoad() {
    const model = selectedModel;
    if (routerMode || !model) return unsavedLoad.clear('dashboard');
    const auto = new Set(autoFieldsNow());
    let manual: string[];
    if (selectedDefinitionId) {
      manual = changedKeys;
    } else {
      const now = modelPart(params);
      const base = modelPart(paramsFromPreset(null, model, settings));
      manual = Object.keys(now).filter((k) => now[k] !== base[k] && !auto.has(k) && k !== 'mmproj_path');
    }
    if (!manual.length) return unsavedLoad.clear('dashboard');
    const config = $state.snapshot(buildLaunchConfig()) as LaunchConfig;
    const definitionId = selectedDefinitionId;
    const alias = domain.definitions.find((d) => d.id === definitionId)?.alias ?? model.name;
    unsavedLoad.register({
      key: 'dashboard',
      alias,
      save: async () => {
        // The model's Dashboard definition (the file's first one), or a new one named after it.
        const target = definitionId ?? (await invoke<string | null>('get_dashboard_definition', { modelId: model.id }));
        let newAlias: string | null = null;
        if (!target) {
          const base = model.name.replace(/[^A-Za-z0-9._:-]+/g, '-').replace(/^-+|-+$/g, '') || 'model';
          const taken = new Set(domain.definitions.map((d) => d.alias.toLowerCase()));
          newAlias = base;
          for (let n = 2; taken.has(newAlias.toLowerCase()); n++) newAlias = `${base}-${n}`;
        }
        await invoke('save_definition_form', { definitionId: target, alias: newAlias, config });
        await domain.refreshDefinitions();
        // The form follows the definition it now belongs to, when it still shows this model.
        if (selectedModelId === model.id && !app.selectedServer?.active) void selectDefinitionOfModel(model.id);
      },
    });
  }

  async function selectDefinitionOfModel(modelId: string) {
    try {
      const defId = await invoke<string | null>('get_dashboard_definition', { modelId });
      if (defId && selectedModelId === modelId && !userEdited) await selectDefinition(defId);
    } catch {
      // The form keeps what it shows.
    }
  }

  async function startNow() {
    if (busy) return;
    actionError = launchError();
    if (actionError) return;
    busy = true;

    // Start never saves: the form's differences from the definition are a transient layer
    // (explicit "Save as default" / the profile's "Save changes" store them).
    try {
      // Start saves nothing else, but a name generated from the model has to match the model
      // that now runs (the server lists show the profile's name). Only when the form's model is
      // the profile's saved model: an unsaved model change must never rename the saved profile.
      const profile = selectedProfile;
      const sameSavedModel =
        profile && !!profile.config.router_mode === routerMode && (routerMode || (!!selectedModel && samePath(profile.config.model_path, selectedModel.path)));
      // A new server that never ran is still being defined: its first start keeps the model it
      // runs, and a generated name follows that model (a name typed by hand is kept).
      const firstStart = !!profile && app.unstartedProfiles.has(profile.id);
      if (profile && firstStart && !sameSavedModel) {
        const keepName = !!serverName.trim() || !isAutoName(profile, profile.model_id, !!profile.config.router_mode);
        const config = { ...($state.snapshot(buildLaunchConfig()) as LaunchConfig), api_key: profile.config.api_key, api_key_ref: profile.config.api_key_ref };
        app.setProfiles(await invoke<ServerProfile[]>('save_server_profile', {
          profile: {
            ...($state.snapshot(profile) as ServerProfile),
            name: keepName ? effectiveName : autoName,
            model_id: routerMode ? null : (selectedModel?.id ?? null),
            config,
          },
        }));
      } else if (profile && sameSavedModel && !serverName.trim() && profile.name !== autoName) {
        app.setProfiles(await invoke<ServerProfile[]>('save_server_profile', { profile: { ...($state.snapshot(profile) as ServerProfile), name: autoName } }));
      }
      await invoke('start_server', { config: buildLaunchConfig(), ...app.serverArgs() });
      registerUnsavedLoad();
      if (profile) app.unstartedProfiles.delete(profile.id);
      // A Dashboard server start saved its launch: every view shows it from now on.
      if (isDefaultServer) void app.refreshDefaultServer();
    } catch (e: any) {
      // Network refusals (e.g. network.lan_without_key) come back as keys; the app-wide
      // "open anyway" dialog is raised by the backend event.
      actionError = e ? localizeError(e) : t('dash.err_start_failed');
    } finally {
      busy = false;
    }
  }

  /**
   * A recovery option was retried: show its values in the form (unsaved changes unless the
   * dialog saved them to the definition) so the next start uses them too.
   */
  function applyRecovery(option: RecoveryOption) {
    for (const [id, v] of Object.entries(option.overrides)) {
      switch (id) {
        case 'gpu_layers':
        case 'context_size':
          if (typeof v === 'number') params[id] = v;
          break;
        case 'physical_batch':
        case 'logical_batch':
          if (typeof v === 'number') params[id] = v;
          break;
        case 'cache_type_k':
        case 'cache_type_v':
          if (typeof v === 'string') params[id] = v;
          break;
        case 'flash_attn':
          if (typeof v === 'boolean') params.flash_attn = v;
          break;
        default:
          params.catalog_params[id] = v;
      }
    }
  }

  /** Writes the form (name, purpose, every launch setting) into the selected server profile. */
  async function saveProfileChanges() {
    const existing = selectedProfile;
    if (!existing) return;
    const profile: ServerProfile = {
      ...($state.snapshot(existing) as ServerProfile),
      name: effectiveName,
      description: serverDesc.trim(),
      model_id: routerMode ? null : (selectedModel?.id ?? existing.model_id),
      config: $state.snapshot(buildLaunchConfig()) as LaunchConfig,
    };
    const list = await invoke<ServerProfile[]>('save_server_profile', { profile });
    app.setProfiles(list);
    // The typed key now lives in the keychain; keep only its reference in the form.
    params.api_key = '';
    params.api_key_ref = list.find((p) => p.id === profile.id)?.config.api_key_ref ?? null;
    // A generated name stays generated (it keeps following the model); a typed one is kept.
    serverName = serverName.trim();
  }

  async function handleSaveProfile() {
    if (!selectedProfile) return openSaveProfile();
    actionError = launchError();
    if (actionError) return;
    try {
      await saveProfileChanges();
      profileMsg = t('srv.saved_msg', { name: effectiveName });
    } catch (e: any) {
      actionError = String(e);
    }
  }

  /** Another server like the one on screen, on an automatic port; selected right away. */
  async function createNewServer() {
    userEdited = false;
    const err = launchError();
    if (err) {
      toast.error(err);
      return;
    }
    try {
      // The port is shown next to every server name, so it is not repeated in the name itself;
      // a second server of the same model gets a number instead.
      const name = uniqueName(baseNameFor(selectedModelId, routerMode), null);
      // Never the same port as the server on screen: Harness picks a free one at its start.
      const config = { ...($state.snapshot(buildLaunchConfig()) as LaunchConfig), port: 0, port_auto: true, api_key: null, api_key_ref: null };
      const profile: ServerProfile = {
        id: makeProfileId(name, [...app.profiles.map((p) => p.id), ...app.servers.map((s) => s.id)]),
        name,
        model_id: routerMode ? null : (selectedModel?.id ?? null),
        description: '',
        config,
        auto_start: false,
        updated_at: 0,
      };
      app.setProfiles(await invoke<ServerProfile[]>('save_server_profile', { profile }));
      app.unstartedProfiles.add(profile.id);
      app.selectServer(profile.id);
      toast.success(t('srv.new_created', { name }));
    } catch (e) {
      toast.error(String(e));
    }
  }

  // "New server" pressed elsewhere (header picker, Servers tab): create it once the models are known.
  $effect(() => {
    if (app.newServerPending && modelsLoaded) {
      app.newServerPending = false;
      untrack(createNewServer);
    }
  });

  function openSaveProfile() {
    actionError = launchError();
    if (actionError) return;
    profileMsg = null;
    profileConfig = $state.snapshot(buildLaunchConfig()) as LaunchConfig;
    showSaveProfile = true;
  }

  async function handleStop() {
    if (busy) return;
    busy = true;
    actionError = null;
    try {
      await invoke('stop_server', app.serverArgs());
      void unsavedLoad.flush();
    } catch (e: any) {
      actionError = e?.toString() || t('dash.err_stop_failed');
    } finally {
      busy = false;
    }
  }

  /** After a measured spill into system memory: stop, set the KV cache to q8_0 and start again. */
  async function reloadWithQ8() {
    if (busy) return;
    await handleStop();
    if (actionError) return;
    params.cache_type_k = 'q8_0';
    params.cache_type_v = 'q8_0';
    await startNow();
  }

  /** Very narrow windows (or a large UI scale): controls above telemetry in one scrolling column. */
  let stacked = $derived(viewportWidth() < 1100);
  // Compare to defaults, not the saved preset: saved advanced choices are active too.
  let hiddenParams = $derived(countHiddenParams(params, paramsFromPreset(null, selectedModel, settings)));
  let logsOpen = $state(false);
  let logError = $derived.by(() => {
    for (let i = logs.length - 1; i >= 0; i--) {
      if (classifyLevel(logs[i].text) === 'error') return `${logs[i].server ?? ''}:${logs[i].seq}`;
    }
    return null;
  });
  let serverFailed = $derived(typeof serverStatus === 'object' && 'Error' in serverStatus);
  $effect(() => {
    void app.selectedServerId;
    void uiLevel.level;
    logsOpen = untrack(() => serverFailed || logError !== null);
  });
  $effect(() => {
    const failed = serverFailed;
    const error = logError;
    if (failed || error !== null) logsOpen = true;
  });

  async function clearLogs() {
    try {
      await serverLogs.clear(shownLogIds);
    } catch (e) {
      toast.error(t('dash.n_clear_failed', { error: String(e) }));
    }
  }
</script>

<!-- Left: Model -> Key parameters -> Advanced -> Start -->
{#snippet controls()}
  <aside class={cn('flex min-h-0 flex-col bg-card/40', stacked ? 'overflow-clip border-b border-border' : 'h-full overflow-hidden')} aria-label={t('dash.aria_controls')}>
    <!-- Flat sections split by rules (no cards inside the panel): server, model, parameters, advanced. -->
    <div class="flex min-h-0 min-w-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
      <AdvancedPageNotice level={pageLevel(DASHBOARD_GROUPS, section.active)} />
      <div class="flex min-w-0 items-center gap-1">
        <h1 class="min-w-0 text-base font-semibold">{t((!uiLevel.advanced && DASHBOARD_GROUPS.flatMap(g => g.pages).find(p => p.id === section.active)!.simpleLabel) || DASHBOARD_GROUPS.flatMap(g => g.pages).find(p => p.id === section.active)!.label)}</h1>
        {#if section.active === 'sampling'}<HelpPopover text={t('menus.sampling_note')} />{/if}
      </div>
      <div hidden={section.active !== 'sampling'} class="flex flex-col gap-3">
        <Button variant="outline" onclick={() => { app.requestedTab = 'playground'; requestSection('playground', 'sampling'); panes.pgSettings.open(); }}>{t('nav.playground')}</Button>
        <Button variant="outline" onclick={() => domain.openPresetManager(null)}>{t('ip.manage')}</Button>
      </div>
<div hidden={section.active !== 'overview'} class="flex flex-col gap-3">
      {#if !uiLevel.advanced}
        <p class="text-sm leading-relaxed text-muted-foreground">{t('uiLevel.start_hint')}</p>
        <Button onclick={() => section.select('model')}>{t('uiLevel.model')}</Button>
        <div class="flex flex-wrap gap-2">
          <Button variant="outline" onclick={onGoToHub}>{t('uiLevel.find')}</Button>
          <Button variant="outline" onclick={() => app.openPlaygroundTab('chat')}>{t('uiLevel.use')}</Button>
          <Button variant="ghost" onclick={onGoToConnections}>{t('uiLevel.connect')}</Button>
        </div>
      {/if}
      <AdvancedOnly>
      <div class="flex flex-col gap-2.5 border-b border-border pb-4" role="group" aria-label={t('srv.card_aria')}>
        <div class="flex flex-wrap items-center justify-between gap-2">
          <!-- Run state is shown once, globally, in the title bar's server indicator. -->
          <div class="flex min-w-0 flex-wrap items-baseline gap-2">
            <h2 class="text-sm font-semibold text-foreground">{t('ui.dash_server')}</h2>
            <span class="break-words text-xs text-muted-foreground" title={savedStateTip}>
              {#if savedLaunch && !formDraft}
                {t('ui.dash_saved')} · <span class="font-num">{savedPortText(savedLaunch, t('dash.port_auto'))}</span>
              {:else}
                {t('ui.srv_unsaved')} · <span class="font-num">{savedPortText({ port, port_auto: portAuto }, t('dash.port_auto'))}</span>{#if savedPortNote !== null}
                  <span class="font-num"> ({t('srv.saved_port', { port: savedPortNote })})</span>{/if}
              {/if}
            </span>
          </div>
          <Button variant="ghost" size="xs" class="shrink-0" onclick={createNewServer} title={t('srv.new_server_tip')}>
            <Plus class="size-3" />
            {t('srv.new_server')}
          </Button>
        </div>
        {#if runningPort !== null && !portAuto && runningPort !== port}
          <p class="text-xs leading-snug text-warning" role="status">{t('srv.running_other_port', { running: runningPort, port })}</p>
        {/if}
        <div class="contents" oninput={markEdited} onchange={markEdited}>
        <Field help={t('help.server_name')} label={t('srv.server_name')} id="{nameId}-name">
          <Input id="{nameId}-name" type="text" maxlength={80} placeholder={autoName} bind:value={serverName} />
        </Field>
        <Field help={t('help.description')} label={t('srv.desc_label')} id="{nameId}-desc" note={selectedProfile ? undefined : t('srv.card_unsaved_hint')}>
          <Input id="{nameId}-desc" type="text" maxlength={200} placeholder={t('srv.desc_placeholder')} bind:value={serverDesc} />
        </Field>
        </div>
      </div>
      </AdvancedOnly>

</div>
<div hidden={section.active !== 'model'} class="flex flex-col gap-3">
      <ModelSelector
        active={modelPageSeen}
        models={launchableModels}
        {selectedModelId}
        {selectedModel}
        {isScanning}
        bind:routerMode
        modelsDir={settings.models_dir}
        {fitResult}
        {recommendedNgl}
        gpuLayers={params.gpu_layers}
        onSelectModel={selectModel}
        onRefresh={refreshModels}
        {onGoToHub}
        onOpenTour={() => (showCoachMarks = true)}
        onApplyRecommendedNgl={() => (params.gpu_layers = recommendedNgl ?? params.gpu_layers)}
        onOpenMigration={() => (showMigrationModal = true)}
        onOpenImport={() => (showImportModal = true)}
        onUnlink={unlinkModel}
        definitions={domain.definitions.filter((d) => (!isSpeechDefinition(d) || isOnnxCtcDefinition(d)))}
        {selectedDefinitionId}
        onSelectDefinition={selectDefinition}
        {onDefinitionAction}
      />

</div>
      <!-- Hand edits below (not the model picker above) count as a draft worth keeping. -->
      <div class="contents" oninput={markEdited} onchange={markEdited} onclick={markEdited} role="presentation">
<div hidden={section.active !== 'model'} class="flex flex-col gap-3">
      <RuntimePicker active={modelPageSeen} bind:runtime={params.runtime} modelPath={routerMode ? null : (selectedModel?.path ?? null)} engine={capsStore.engine} />
      <MetadataFixNotice active={modelPageSeen} modelPath={routerMode ? null : (selectedModel?.path ?? null)} bind:params onApply={() => (userEdited = true)} />
      {#if serverStatus === 'Running' && app.selectedServer?.active?.router_mode}
        <RouterModels serverId={app.selectedServerId} />
      {/if}

</div>
      {#if capsStore.otherEngine}
        <div hidden={section.active !== 'model'} class="flex flex-col gap-3">
          <h2 class="text-sm font-semibold">{t('dash.engine_settings')}</h2>
          <EngineParams bind:params />
        </div>
        <LaunchParams page={section.active === 'network' ? 'network' : ''} bind:params bind:port bind:portAuto {maxGpuLayers} {hardware} {portWarning} slotAdvice={null} />
        {#if !['overview', 'model', 'network'].includes(section.active)}
          <p class="text-xs text-muted-foreground" role="note">{t('dash.engine_page_note', { engine: engineLabel(capsStore.engine) })}</p>
        {/if}
      {:else if selectedModel?.kind !== 'asr'}
      <LaunchParams page={section.active} bind:params bind:port bind:portAuto {maxGpuLayers} {hardware} {portWarning} slotAdvice={routerMode ? null : (fitResult?.slots ?? null)} />

      <AdvancedParams page={section.active}
        bind:params
        {models}
        {selectedModelId}
        {host}
        {hardware}
        {routerMode}
        serverId={app.selectedServerId}
        {onGoToConnections}
      />
      {/if}
      </div>
    </div>

    <!-- Pinned action footer (compact in a short window: one-line banner, links in a menu). -->
    <div class={cn('flex shrink-0 flex-col gap-2 border-t border-border bg-card px-4 py-3 short:gap-1.5 short:py-2', stacked && 'sticky bottom-0 z-10')}>
      {#if !uiLevel.advanced && hiddenParams > 0}
        <div class="flex min-w-0 flex-wrap items-center gap-2 text-xs text-muted-foreground" role="status">
          <span>{t('dash.hidden_params', { count: hiddenParams })}</span>
          <Button variant="ghost" size="xs" disabled={uiLevel.busy} onclick={() => uiLevel.set('advanced')}>{t('dash.show_advanced')}</Button>
        </div>
      {/if}
      {#if !routerMode && selectedModel}
        <DefinitionSaveBar
          definition={selectedDefinition ?? null}
          fileAlias={selectedModel.name}
          changes={changedKeys.length}
          usedBy={definitionUsers}
          busy={defBusy}
          onSaveDefault={() => saveToDefinition(selectedDefinitionId, null)}
          onSaveNew={(alias) => saveToDefinition(null, alias)}
          onRevert={() => selectedDefinitionId && selectDefinition(selectedDefinitionId)}
        />
      {/if}
      <ServerActions
        {serverStatus}
        {routerMode}
        {selectedModel}
        {readyModelCount}
        {actionError}
        {profileMsg}
        onStart={handleStart}
        onStop={handleStop}
        isProfile={!!selectedProfile}
        onSaveProfile={handleSaveProfile}
        onRecovered={applyRecovery}
      />
    </div>
  </aside>
{/snippet}

{#snippet telemetryContent()}
  <TelemetryPanel {serverStatus} telemetry={app.telemetry} onStart={undefined} />
  <EffectiveValues
    {serverStatus}
    serverId={app.selectedServerId}
    kv={{ k: params.cache_type_k, v: params.cache_type_v, customArgs: params.custom_args }}
    onReloadQ8={reloadWithQ8}
  />
  <LoraLiveScales {serverStatus} serverId={app.selectedServerId} />
{/snippet}
{#snippet logContent()}
  <LogConsole {logs} sourceIds={logSources} shownIds={shownLogIds} onClear={clearLogs} />
{/snippet}

<!-- Right: live telemetry and terminal -->
{#snippet telemetry()}
  <div class={cn('flex min-w-0 flex-1 flex-col overflow-hidden', stacked ? 'min-h-96' : 'h-full min-h-0')} role="region" aria-label={t('dash.aria_telemetry')}>
    <!-- The empty state only repeats "Start" when the controls (with their own button) are hidden. -->
    {#if uiLevel.advanced}
      <VerticalSplit id="dash-telemetry" label={t('pane.dash_split')}>
        {#snippet top()}{@render telemetryContent()}{/snippet}
        {#snippet bottom()}{@render logContent()}{/snippet}
      </VerticalSplit>
    {:else}
      <div class="min-h-0 flex-1 overflow-y-auto">{@render telemetryContent()}</div>
      <Button variant="ghost" class="shrink-0 justify-start rounded-none border-t border-border px-4" aria-expanded={logsOpen} onclick={() => (logsOpen = !logsOpen)}>
        {t(logsOpen ? 'dash.hide_logs' : 'dash.show_logs')}
      </Button>
      {#if logsOpen}<div class="h-72 min-h-0 shrink-0">{@render logContent()}</div>{/if}
    {/if}
  </div>
{/snippet}

<div class="flex h-full w-full overflow-hidden">
  <SectionSidebar pane={panes.dashboardNav} label={t('nav.dashboard')} groups={DASHBOARD_GROUPS} active={section.active} onSelect={section.select} />
  <div class={cn('flex min-w-0 flex-1', stacked ? 'flex-col overflow-y-auto' : 'overflow-hidden')}>
    <div class={cn('min-w-0', stacked ? 'shrink-0' : 'h-full w-[min(48%,560px)] min-w-[304px] shrink-0')}>
      {@render controls()}
    </div>
    {@render telemetry()}
  </div>
</div>

{#if showMigrationModal}
  <MigrationModal
    model={selectedModel}
    {storageDirs}
    onMigrated={handleModelMigrated}
    onClose={() => (showMigrationModal = false)}
  />
{/if}

{#if showSaveProfile && profileConfig}
  <SaveProfileModal
    config={profileConfig}
    modelId={routerMode ? null : (selectedModel?.id ?? null)}
    defaultName={effectiveName}
    defaultDescription={serverDesc.trim()}
    onSaved={(name) => {
      profileMsg = t('srv.saved_msg', { name });
      showSaveProfile = false;
    }}
    onClose={() => (showSaveProfile = false)}
  />
{/if}

{#if showImportModal}
  <ImportModal onClose={() => (showImportModal = false)} onImported={refreshModels} />
{/if}

{#if editDef}
  <DefinitionDialog
    definition={editDef.def}
    {models}
    focusAlias={editDef.focusAlias}
    onSaved={(def) => {
      const before = editDef?.def;
      editDef = null;
      toast.success(t('def.saved', { alias: def.alias }));
      // The alias is the served model id: client configs written earlier still name the old one.
      if (before?.id && before.id === def.id && before.alias !== def.alias) {
        toast.warning(t('def.alias_changed', { old: before.alias, alias: def.alias }), { duration: 10000 });
      }
      void refreshAfterDefinitionSaved(def.id);
    }}
    onClose={() => (editDef = null)}
  />
{/if}

{#if deleteDef}
  <DeleteDefinitionDialog
    definition={deleteDef}
    onDeleted={() => {
      const alias = deleteDef?.alias ?? '';
      deleteDef = null;
      toast.success(t('def.deleted', { alias }));
      if (selectedModel) void selectModel(selectedModel.id);
    }}
    onClose={() => (deleteDef = null)}
  />
{/if}

{#if importPreview}
  <ImportDefinitionDialog
    preview={importPreview}
    onImported={(def) => {
      importPreview = null;
      toast.success(t('def.imported', { alias: def.alias }));
      if (def.base.kind === 'local') void selectDefinition(def.id);
    }}
    onClose={() => (importPreview = null)}
  />
{/if}

{#if ollamaPicker}
  <OllamaModelfilePicker
    onPreview={(preview) => {
      ollamaPicker = false;
      modelfilePreview = preview;
    }}
    onClose={() => (ollamaPicker = false)}
  />
{/if}

{#if modelfilePreview}
  <ImportModelfileDialog
    preview={modelfilePreview}
    onImported={(def) => {
      modelfilePreview = null;
      toast.success(t('def.imported', { alias: def.alias }));
      if (def.base.kind === 'local') void selectDefinition(def.id);
    }}
    onClose={() => (modelfilePreview = null)}
  />
{/if}

{#if showCoachMarks}
  <CoachMarks onClose={() => (showCoachMarks = false)} />
{/if}

{#if spillAsk && fitResult}
  <SpillConfirmDialog
    fit={fitResult}
    {recommendedNgl}
    gpuLayers={params.gpu_layers}
    contextSize={params.context_size}
    trainedContext={selectedModel?.context_length ?? null}
    autoKv={autoKvFit?.kv ?? null}
    onAutoKvAndStart={() => {
      spillAsk = false;
      params.cache_type_k = '';
      params.cache_type_v = '';
      void startNow();
    }}
    onApplyAndStart={() => {
      spillAsk = false;
      if (recommendedNgl !== null) params.gpu_layers = recommendedNgl;
      void startNow();
    }}
    onLowerContextAndStart={() => {
      spillAsk = false;
      if (selectedModel?.context_length) params.context_size = selectedModel.context_length;
      void startNow();
    }}
    onStartAnyway={() => {
      spillAsk = false;
      void startNow();
    }}
    onCancel={() => (spillAsk = false)}
  />
{/if}
