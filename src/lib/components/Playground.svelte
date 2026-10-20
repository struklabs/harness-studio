<script lang="ts">
  import { NO_REASONING, observeThinking, type ReasoningControl } from './playground/thinking';
  import { guardToolCalls } from './playground/toolLoopGuard';
  import AdvancedOnly from './common/AdvancedOnly.svelte';
  import { PLAYGROUND_MODES, playgroundModeFromUrl } from './playground/navigation';
  import { isVisible } from '#lib/uiLevel.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedPageNotice from './common/AdvancedPageNotice.svelte';
  import { onMount, tick, untrack } from 'svelte';
  import { selectChatModel } from './playground/modelSelection';
  import { estimatePromptBudget, hasContextPressure } from './playground/contextBudget';
  import { IDLE_HISTORY_BATCH, anchoredScroll, needsOlder, pathStart, previousBatch, settledWhile, tailStart, whenIdle } from './playground/longChat';
  import { definitionFile, definitionsForMode, selectModeModel } from './playground/modelModes';
  import { library } from '#lib/stores/library.svelte.js';
  import { holdReleaseWhile } from '#lib/windowActivity.js';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { ACTION_TOAST_DURATION } from '#lib/toast.js';
  import { presetName } from '#lib/components/presets/presetText.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Empty from '#lib/components/ui/empty/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { requestErrorText } from './connections/net';
  import ChatMessage from './playground/ChatMessage.svelte';
  import ChatInput from './playground/ChatInput.svelte';
  import {
    IMAGE_TOKEN_ESTIMATE,
    ImageError,
    MAX_IMAGES_PER_MESSAGE,
    MAX_IMAGE_BYTES,
    MAX_IMAGE_EDGE,
    countImageParts,
    loadImage,
    visionFromProps,
    withoutImages,
    type PgImage,
  } from './playground/images';
  import { loadImagePath, splitImagePaths } from './playground/imagePaths';
  import WorkspacePicker from './playground/WorkspacePicker.svelte';
  import InfoTip from './common/InfoTip.svelte';
  import SettingsPanel from './playground/SettingsPanel.svelte';
  import ToolGeneratorDialog from './playground/ToolGeneratorDialog.svelte';
  import { withToolGuidance as baseToolGuidance } from './playground/toolGuidance';
  import { withSkillGuidance, type SkillMetadata } from './playground/skillGuidance';
  import { salvageToolCalls, TextCallStreamBuffer } from './playground/textToolCalls';
  import { chatRequest } from './playground/chatRequest';
  import { projects } from '#lib/stores/projects.svelte.js';
  import { projectAllowsSkill, projectAllowsTool } from '#lib/projects.js';
  import { notifyRoutingFallback } from './playground/routingNotice';
  import { parseChoiceLogprobs, rememberLogprobs, type TokenLogprob } from './playground/logprobs';
  import { parseTextToolCalls } from '#lib/tools-client.js';
  import ConversationSidebar from './playground/ConversationSidebar.svelte';
  import ModelPicker from './playground/ModelPicker.svelte';
  import { gatewayModelStates, type GatewayModelEntry, type ModelLoadState } from './playground/gatewayModels';
  import { isRemoteModel, type RemoteModel } from './playground/remoteModels';
  import EmbeddingPanel from './playground/EmbeddingPanel.svelte';
  import RerankPanel from './playground/RerankPanel.svelte';
  import PipelinePanel from './playground/PipelinePanel.svelte';
  import { ensureLibraryDefinitions, loadAndChat } from '#lib/stores/downloads.svelte.js';
  import { isChatLoadable } from '#lib/downloadFollowUp.js';
  import { kindsForTab, servedModelName } from '#lib/libraryDefinitions.js';
  import { loadAudio, AudioError, MAX_AUDIO_BYTES, MAX_AUDIO_PER_MESSAGE, type PgAudio } from './playground/audio';
  import TranscribePanel from './playground/TranscribePanel.svelte';
  import ImagePanel from './playground/ImagePanel.svelte';
  import { imageSession } from './playground/image.svelte';
  import TtsPanel from './playground/TtsPanel.svelte';
  import AudioProcessPanel from '#lib/components/playground/AudioProcessPanel.svelte';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import { domain, samePath } from '#lib/stores/domain.svelte.js';
  import { DEFAULT_PRESET_ID, presetFromRequest } from './presets/request';
  import type { InferencePreset, ServeMode, LibraryView } from '#lib/types.js';
  import { Unplug, MessageSquare, Plus, SlidersHorizontal, PanelLeft, TriangleAlert, Columns2 } from '@lucide/svelte';
  import SplitView from './playground/SplitView.svelte';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';
  import {
    SIDES,
    COMPARE_KEY,
    beginRound,
    emptySide,
    gwTarget,
    isCapacityError,
    isSplit,
    loadCompareDefaults,
    modelFileName,
    parseTarget,
    presetBody,
    sanitizeCompare,
    saveCompareDefaults,
    sideHistory,
    sideOf,
    srvTarget,
    streamInto,
    type CompareMeta,
    type CompareSettings,
    type Endpoint,
    type PaneConfig,
    type CoResidency,
    type Side,
    type SideState,
    type StreamResult,
  } from './playground/split';
  import { formatBytes } from '#lib/format.js';
  import { formatTokens, formatTokenNumber } from '#lib/utils/tokens.js';
  import { SseParser, type SseEvent } from './playground/sse';
  import { describeError } from '#lib/gateway-busy-text.js';
  import {
    ApprovalBroker,
    buildApiMessages,
    buildStats,
    executeToolCalls,
    runAgentLoop,
    type Timings,
    type Usage,
  } from './playground/agentLoop';
  import {
    DEFAULT_SETTINGS,
    DEFAULTS_KEY,
    LEGACY_STORAGE_KEY,
    applyPreset,
    loadDefaultSettings,
    presetModified,
    loadLegacyState,
    newId,
    samplingParams,
    saveDefaultSettings,
    type ChatMessage as Msg,
    type PgSettings,
    type ToolsState,
  } from './playground/types';
  import {
    convApi,
    fallbackTitle,
    settingsFromConversation,
    settingsSnapshot,
    toVariantInput,
    viewMessages,
    type Conversation,
    type ConversationView,
  } from './playground/conversations';
  import { generateTitleWithRetry } from './playground/titles';
  import { isTurnEnd as turnEndAt, regeneratePlan, turnRoot, variantNavOf, variantTarget } from './playground/branching';
  import { ToolCallAccumulator } from '#lib/mcp-client.js';
  import { coreToolAsServerTool, getMcpStatus, listCoreTools, type ToolLimits } from '#lib/tools-client.js';
  import type { ServerTool } from '#lib/mcp-types.js';
  import { panes } from '#lib/stores/panes.svelte.js';
  import ResizablePane from './common/ResizablePane.svelte';
  import DocumentsBar from './playground/DocumentsBar.svelte';
  import McpResourcesButton from './playground/McpResourcesButton.svelte';
  import CompactionDivider from './playground/CompactionDivider.svelte';
  import CompactButton from './playground/CompactButton.svelte';
  import {
    activeCompaction,
    autoPaused,
    compactionApi,
    compactionDefaults,
    COMPACTION_DEFAULTS_KEY,
    initGlobalCompaction,
    estimateTokens,
    parseCompactCommand,
    thresholdTokens,
  } from './playground/compaction.svelte';
  import {
    applyRag,
    ragApi,
    resolveEmbedModel,
    type RagDocument,
    type RagMode,
    type RagModels,
    type RagPrepared,
    type RagProgress,
  } from './playground/rag';
  import { BackendError, errorText, rawError } from '#lib/errors.js';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { FileUp } from '@lucide/svelte';

  /** Example prompts shown in the empty chat. */
  const EXAMPLES = ['pg.example_1', 'pg.example_2', 'pg.example_3', 'pg.example_4'] as const;
  /** Share of the context window from which the meter turns into a warning. */
  const CONTEXT_WARN = 0.8;

  interface Props {
    onGoToDashboard: () => void;
    /** Opens the Model Hub (documents: no embedding model yet). */
    onGoToHub?: () => void;
  }

  let { onGoToDashboard, onGoToHub }: Props = $props();

  // ---- Conversation (state.db) ----
  let conv = $state<Conversation | null>(null);
  let messages = $state<Msg[]>([]);
  /** Index of the first mounted message: a long chat opens with its tail only; older ones mount on scroll. */
  let winStart = $state(0);
  let shownStart = $derived(Math.min(winStart, Math.max(0, messages.length - 1)));
  let shownMessages = $derived(shownStart > 0 ? messages.slice(shownStart) : messages);
  let settings = $state<PgSettings>({ ...DEFAULT_SETTINGS });
  let settingsLoaded = $state(false);
  /** Bumped whenever the sidebar list may have changed. */
  let listKey = $state(0);
  let dbReadOnly = $state(false);
  let dbError = $state('');
  // Side panels: size and visibility live in the shared pane store (resizable, persisted).
  let showSidebar = $derived(panes.pgConversations.expanded);

  let input = $state('');
  /** Pictures attached to the message being written (sent with it, then cleared). */
  let pendingImages = $state<PgImage[]>([]);
  let pendingAudio = $state<PgAudio[]>([]);
  let libraryAudio = $state(false);
  let audioAliases = $state<Record<string, boolean>>({});
  let libraryVision = $state<boolean | null>(null);
  let streaming = $state(false);
  /** A saved chat is being opened (messages not read yet). */
  let openingConv = $state(false);
  let showSettings = $derived(panes.pgSettings.expanded);
  let controller: AbortController | null = null;
  let currentRun: Promise<void> | null = null;

  let models = $state<string[]>([]);
  /** Gateway only: alias -> `loaded` | `loading` | `unloaded` (the scheduler's view). */
  let modelStates = $state<Record<string, ModelLoadState>>({});
  let modelsLoading = $state(false);
  let modelsError = $state(false);
  let modelsAbort: AbortController | null = null;

  let listEl = $state<HTMLDivElement>();
  let composer = $state<ReturnType<typeof ChatInput>>();
  let stickToBottom = true;
  let scrollQueued = false;

  // ---- Agent tools: the core's registry (built-ins and the MCP servers' tools, `tools_*`) ----
  /** The core's registry: built-in tools and the tools of MCP servers (Harness's own MCP client). */
  let coreTools = $state<ServerTool[]>([]);
  let skills = $state<SkillMetadata[]>([]);
  function withToolGuidance(messages: Record<string, unknown>[], tools: { function?: { name?: string } }[]) {
    return withSkillGuidance(baseToolGuidance(messages, tools), tools.some(t => t.function?.name === 'load_skill') ? skills : []);
  }
  /** Rounds, per-call time and result size from the core (shared with the CLI and the API). */
  let toolLimits = $state<ToolLimits | null>(null);
  let toolsState = $state<ToolsState>('idle');
  let toolsError = $state('');
  let toolsAbort: AbortController | null = null;
  /** MCP servers still starting: the list is read again until they are done. */
  let mcpPoll: ReturnType<typeof setTimeout> | null = null;
  let mcpPolls = 0;
  const broker = new ApprovalBroker();

  // ---- Split view: same prompt, two models or presets ----
  let compare = $state<CompareSettings>(sanitizeCompare(null));
  let sideState = $state<Record<Side, SideState>>({ a: 'idle', b: 'idle' });
  /** Why the sides run one after the other (shown above the panes). */
  let splitNotice = $state('');
  let splitView = $state<ReturnType<typeof SplitView>>();
  let rootEl = $state<HTMLDivElement>();
  /** Controllers of the running split turn (Stop aborts both). */
  let splitControllers: AbortController[] = [];

  // ---- Tabs: chat, or the Embedding / Rerank test benches ----
  let pgTab = $state<ServeMode>('chat');
  let modeModels = $state({ embedding: '', rerank: '', transcription: '', image: '', pipeline: '' });
  /** Pipeline task ("fill-mask"...) of the Tasks tab's model, from its library file. */
  let pipelineTask = $derived.by(() => {
    const def = domain.definitions.find(d => d.alias === modeModels.pipeline);
    return def ? definitionFile(def, library.models)?.task : undefined;
  });
  let chatAliases = $derived(definitionsForMode(domain.definitions, 'chat', library.models).map(def => def.alias));
  let chatModels = $derived(models.filter(alias => chatAliases.includes(alias) || (app.gatewayRunning && isRemoteModel(alias)) || (!app.gatewayRunning && serveMode === 'chat')));
  let pickerModel = $derived(pgTab === 'chat' ? settings.model : pgTab === 'image' ? imageSession.model : modeModels[pgTab]);
  $effect(() => {
    void library.version;
    untrack(() => void library.refresh());
  });
  $effect(() => {
    for (const mode of ['embedding', 'rerank', 'transcription', 'image', 'pipeline'] as const) {
      const available = definitionsForMode(domain.definitions, mode, library.models).map(def => def.alias);
      modeModels[mode] = selectModeModel(modeModels[mode], available);
    }
  });
  onMount(() => {
    const navigate = () => { const mode = playgroundModeFromUrl(location.href); if (mode) pgTab = mode; };
    navigate();
    window.addEventListener('hashchange', navigate);
    window.addEventListener('popstate', navigate);
    return () => { window.removeEventListener('hashchange', navigate); window.removeEventListener('popstate', navigate); };
  });
  // Another view (the Hub's "use it in Speech" toast) asked for a tab.
  $effect(() => {
    const tab = app.requestedPlaygroundTab;
    if (!tab) return;
    app.requestedPlaygroundTab = null;
    pgTab = tab;
    const alias = app.requestedPlaygroundModel;
    app.requestedPlaygroundModel = null;
    if (alias && tab === 'chat' && isRemoteModel(alias)) useModel(app.selectedServerId, alias);
    else if (alias && (tab === 'embedding' || tab === 'rerank' || tab === 'pipeline')) modeModels[tab] = alias;
  });
  // Embedding / Rerank list definitions: library files found by a scan get their default one first.
  $effect(() => {
    void ensureLibraryDefinitions(kindsForTab(pgTab));
  });
  // The Speech tab's "Add to chat": the transcript goes into the draft, the composer gets focus.
  $effect(() => {
    const text = app.requestedComposerText;
    if (text === null) return;
    app.requestedComposerText = null;
    input = input.trim() ? `${input.replace(/\s+$/, '')}\n${text}` : text;
    focusComposerPending = true;
  });
  // The chat tab (and its composer) mounts only after the tab switch that follows the text.
  let focusComposerPending = $state(false);
  $effect(() => {
    if (!focusComposerPending || pgTab !== 'chat' || !composer) return;
    focusComposerPending = false;
    void tick().then(() => composer?.focus(true));
  });
  // The header's mode tabs turn into icons when their labels do not fit next to the model picker,
  // and the title yields its place when even the icons do not fit (large UI scale, side panel open).
  // Measured rather than a fixed width: fonts (macOS is wider), zoom and the right-hand controls vary.
  /** 0: labels, 1: icon tabs, 2: icon tabs and the title hidden. */
  let tabsCompact = $state(0);
  /** Width the left header group needed at each level (measured when it overflowed there). */
  const tabsFitWidths: number[] = [];
  function fitTabs(node: HTMLElement) {
    const check = () => {
      if (tabsCompact < 2 && node.scrollWidth > node.clientWidth + 1) {
        tabsFitWidths[tabsCompact] = node.scrollWidth;
        tabsCompact += 1;
        // The next level changes the content, not the node's size: measure again once it renders.
        void tick().then(check);
      } else if (tabsCompact > 0 && node.clientWidth >= tabsFitWidths[tabsCompact - 1]) {
        tabsCompact -= 1;
        void tick().then(check);
      }
    };
    const ro = new ResizeObserver(check);
    ro.observe(node);
    return { destroy: () => ro.disconnect() };
  }
  /** The split view is shown (compare mode, chat tab). */
  let splitOn = $derived(compare.enabled && pgTab === 'chat');
  /** What the selected server was started for (`--embeddings` / `--rerank` servers do not chat). */
  let serveMode = $derived<ServeMode>(app.isRunning ? (app.activeServer?.serve_mode ?? 'chat') : 'chat');
  let lastServeMode: ServeMode = 'chat';
  $effect(() => {
    // Follow the server: an embedding/rerank server opens its own tab once.
    const m = serveMode;
    if (m === lastServeMode) return;
    lastServeMode = m;
    if (m !== 'chat') pgTab = m;
  });

  // With the gateway on, any model definition can be chatted with: the gateway loads it on the
  // first request (JIT), even when no server runs yet.
  let canChat = $derived(app.isRunning || app.gatewayRunning);
  /**
   * The model the Dashboard would start: its saved launch's file, else the first chat model of
   * the library. Null when there is none (the offline notice then only points to the Dashboard).
   */
  let startModel = $derived.by(() => {
    const ready = library.models.filter(isChatLoadable);
    const saved = app.selectedSavedConfig?.model_path;
    return (saved ? ready.find((m) => samePath(m.path, saved)) : undefined) ?? ready[0] ?? null;
  });
  let startLabel = $derived(startModel ? t('pg.start_model', { model: startModel.gguf?.model_name || startModel.name || startModel.filename }) : '');
  let starting = $state(false);
  async function startFromHere() {
    if (!startModel || starting || app.isBusy) return;
    starting = true;
    try {
      await loadAndChat(startModel);
    } finally {
      starting = false;
    }
  }
  let persistEnabled = $derived(!dbReadOnly && !dbError);
  /** Every tool that can be offered: the core's (built-ins and the MCP servers' tools, run by Harness's own MCP client). */
  let allTools = $derived(coreTools);
  let activeTools = $derived(
    settings.toolsEnabled && toolsState === 'ok' && toolLimits
      ? allTools.filter((tl) => !settings.disabledTools.includes(tl.tool))
      : []
  );
  let composerWaiting = $derived(!settingsLoaded || (settings.toolsEnabled && (toolsState === 'idle' || toolsState === 'loading')));
  let awaitingApproval = $derived.by(() => {
    const last = messages.at(-1);
    if (!last) return false;
    // A split turn runs one tool loop per side.
    const lists = last.compareSides ? last.compareSides.map((s) => s.toolCalls) : [last.toolCalls];
    return lists.some((l) => l?.some((c) => c.status === 'approval'));
  });

  function toggleSidebar() {
    panes.pgConversations.toggle();
  }

  // ---- Persistence queue: every state.db write runs in order ----
  let dbQueue: Promise<unknown> = Promise.resolve();
  function queue<T>(fn: () => Promise<T>): Promise<T> {
    const run = dbQueue.then(fn, fn);
    dbQueue = run.catch(() => {});
    return run;
  }

  function dbFail(e: unknown) {
    toast.error(t('pg.conv_error', { message: String(e) }));
  }

  /** Variants already stored (later saves become updates). */
  const savedVariants = new Set<string>();

  /** Stores a message (append or new variant) or updates its stored variant. */
  function saveMessage(msg: Msg, convId: string | null, anyParentVariant = false) {
    if (!convId || !persistEnabled) return;
    if (!msg.variantId) msg.variantId = newId();
    const snap = $state.snapshot(msg) as Msg;
    // Compaction markers are written by the backend (compact_conversation).
    if (snap.role === 'compaction') return;
    const role = snap.role;
    const vid = snap.variantId as string;
    const variant = toVariantInput(snap, snap.role === 'assistant' ? settings.model : undefined);
    if (savedVariants.has(vid)) {
      queue(() => convApi.updateVariant(vid, variant)).catch(dbFail);
      return;
    }
    savedVariants.add(vid);
    const op = snap.newVariant
      ? () => convApi.addVariant(snap.id, variant, vid)
      : () =>
          convApi.append({
            conversation_id: convId,
            parent_id: snap.parentId ?? null,
            role,
            variant,
            id: snap.id,
            variant_id: vid,
            any_parent_variant: anyParentVariant,
          });
    queue(op).catch((e) => {
      savedVariants.delete(vid);
      dbFail(e);
    });
  }

  function settingsJson(s: PgSettings): string {
    return JSON.stringify(s);
  }

  /** JSON of the settings last written to (or loaded from) the open conversation. */
  let lastSettingsJson = '';

  /** Creates the conversation lazily with the first message (empty chats are not stored). */
  async function ensureConversation(firstText: string): Promise<string | null> {
    if (conv) return conv.id;
    if (!persistEnabled) return null;
    const snap = $state.snapshot(settings) as PgSettings;
    try {
      const c = await queue(() =>
        convApi.create({
          title: fallbackTitle(firstText),
          title_source: 'fallback',
          system_prompt: snap.systemPrompt,
          settings: settingsSnapshot(snap),
          model_alias: snap.model || null,
          server_id: app.selectedServerId,
          preset_id: snap.presetId || null,
          metadata: compare.enabled ? { compare: $state.snapshot(compare) } : undefined,
          project_id: projects.activeId,
        })
      );
      conv = c;
      lastSettingsJson = settingsJson(snap);
      lastCompareJson = c.metadata?.compare ? JSON.stringify($state.snapshot(compare)) : 'null';
      listKey++;
      return c.id;
    } catch (e) {
      dbFail(e);
      return null;
    }
  }

  let workspaceVersion = $state(0);
  /** The opened conversation's model, waiting for the model list to contain it. Restored once:
   *  the list now refreshes on every server/gateway event, and re-applying the stored alias then
   *  silently replaced the user's later pick (and saved the old one back). */
  let pendingModel: { convId: string; alias: string } | null = null;
  /** The model the list refresh put in place itself (stored one missing); anything else in
   *  `settings.model` is the user's pick, whichever control made it, and a pending restore keeps off it. */
  let autoModel: string | null = null;

  function applyView(view: ConversationView, restoreSettings: boolean) {
    workspaceVersion++;
    const prevConvId = conv?.id;
    conv = view.conversation;
    const list = viewMessages(view);
    for (const m of list) if (m.variantId) savedVariants.add(m.variantId);
    // Same chat (branch switch): keep the first mounted message; a freshly opened chat starts at its tail.
    winStart = prevConvId !== undefined && view.conversation.id === prevConvId
      ? pathStart(list.map((m) => m.id), untrack(() => messages[shownStart])?.id)
      : tailStart(list.length);
    messages = list;
    if (restoreSettings) {
      const restored = settingsFromConversation(view.conversation, $state.snapshot(settings) as PgSettings);
      const alias = view.conversation.model_alias;
      if (alias && models.includes(alias)) restored.model = alias;
      pendingModel = alias && !models.includes(alias) ? { convId: view.conversation.id, alias } : null;
      // While the restore waits, the model carried over from before is only a placeholder, not a pick.
      autoModel = pendingModel ? restored.model : null;
      settings = restored;
      lastSettingsJson = settingsJson(restored);
      // Split view: a conversation stored in compare mode reopens in it (with its panes).
      const stored = view.conversation.metadata?.compare;
      compare = stored ? sanitizeCompare(stored) : { ...compare, enabled: false };
      lastCompareJson = stored ? JSON.stringify(compare) : 'null';
      splitNotice = '';
    }
  }

  /** JSON of the split settings last written to (or loaded from) the open conversation. */
  let lastCompareJson = 'null';

  // Split settings: remembered for new chats and kept in the open conversation's metadata.
  $effect(() => {
    const snap = $state.snapshot(compare) as CompareSettings;
    const json = JSON.stringify(snap);
    const c = conv;
    const timer = setTimeout(() => {
      saveCompareDefaults(snap);
      if (!c || !persistEnabled || json === lastCompareJson) return;
      // A conversation that never used the split view does not get the metadata.
      if (lastCompareJson === 'null' && !snap.enabled) return;
      lastCompareJson = json;
      const metadata = { ...(c.metadata ?? {}), compare: snap };
      queue(() => convApi.update(c.id, { metadata }))
        .then((updated) => {
          if (conv?.id === updated.id) conv.metadata = updated.metadata;
        })
        .catch(dbFail);
    }, 400);
    return () => clearTimeout(timer);
  });

  // Settings: remembered as defaults for new chats and saved into the open conversation.
  $effect(() => {
    // Startup must finish migration/defaults/last-chat restore before any write.
    if (!settingsLoaded || dbError) return;
    const snap = $state.snapshot(settings) as PgSettings;
    const json = settingsJson(snap);
    const convId = conv?.id;
    const timer = setTimeout(() => {
      saveDefaultSettings(snap);
      if (!convId || !persistEnabled || json === lastSettingsJson) return;
      lastSettingsJson = json;
      queue(() =>
        convApi.update(convId, {
          system_prompt: snap.systemPrompt,
          settings: settingsSnapshot(snap),
          model_alias: snap.model,
          server_id: app.selectedServerId,
          preset_id: snap.presetId,
        })
      ).catch(dbFail);
    }, 400);
    return () => clearTimeout(timer);
  });

  async function stopAndWait() {
    controller?.abort();
    for (const c of splitControllers) c.abort();
    if (currentRun) await currentRun.catch(() => {});
  }

  async function openConversation(id: string) {
    openingConv = true;
    try {
      await stopAndWait();
      const view = await queue(() => convApi.open(id));
      applyView(view, true);
      input = '';
      pendingImages = [];
      pendingAudio = [];
      scrollToBottom(true);
    } catch (e) {
      dbFail(e);
    } finally {
      openingConv = false;
    }
  }

  async function newChat(presetId?: string) {
    workspaceVersion++;
    await stopAndWait();
    conv = null;
    messages = [];
    winStart = 0;
    input = '';
    pendingImages = [];
    pendingAudio = [];
    splitNotice = '';
    lastCompareJson = 'null';
    // A conversation's own compaction settings don't carry over to the next one.
    settings.compaction = null;
    settings.workdir = ''; // Follow the live default; never copy it into a new chat.
    if (app.isRunning || app.gatewayRunning) await refreshModels();
    preferRunningModel();
    // A new chat starts from the model definition's default preset (else Default).
    selectPreset(presetId ?? defaultPresetFor(settings.model));
    tick().then(() => composer?.focus());
  }

  // ---- Inference presets ----
  /** The served model's definition default preset, else the built-in Default. */
  function defaultPresetFor(model: string): string {
    const def = domain.definitions.find((d) => d.alias.toLowerCase() === model.toLowerCase());
    const id = def?.default_preset_id;
    return id && domain.preset(id) ? id : DEFAULT_PRESET_ID;
  }

  /** Applies a preset to the open conversation's settings (only once presets are loaded). */
  function selectPreset(id: string) {
    const preset = domain.preset(id);
    if (!preset) return;
    applyPreset(settings, $state.snapshot(preset) as InferencePreset, id);
  }

  async function saveAsPreset() {
    const basePreset = domain.preset(settings.presetId);
    const base = basePreset ? presetName(basePreset) : t('ip.new_name');
    const taken = new Set(domain.presets.map((p) => p.name.toLowerCase()));
    let name = t('ip.custom_name', { name: base });
    for (let n = 2; taken.has(name.toLowerCase()); n++) name = `${t('ip.custom_name', { name: base })} (${n})`;
    try {
      const snap = $state.snapshot(settings) as PgSettings;
      const saved = await invoke<InferencePreset>('save_inference_preset', {
        preset: presetFromRequest(name, snap.systemPrompt, snap.request),
      });
      await domain.refreshPresets();
      settings.presetId = saved.id;
      toast.success(t('ip.created', { name }));
    } catch (e) {
      toast.error(String(e));
    }
  }

  // A fresh chat follows the model: switching models applies that definition's default preset
  // (unless the settings were changed by hand).
  let lastModel = '';
  $effect(() => {
    const model = settings.model;
    void domain.presetsLoaded;
    void domain.definitionsLoaded;
    untrack(() => {
      if (model === lastModel) return;
      lastModel = model;
      if (conv || messages.length > 0 || !model) return;
      if (presetModified(settings, domain.preset(settings.presetId))) return;
      const want = defaultPresetFor(model);
      if (want !== settings.presetId) selectPreset(want);
    });
  });

  // Command palette: "New chat with preset…".
  $effect(() => {
    const pending = domain.pendingNewChatPreset;
    if (!pending || !domain.presetsLoaded) return;
    domain.pendingNewChatPreset = null;
    untrack(() => void newChat(pending));
  });

  /** Switches the conversation (or the open tab's model) to `alias` on `serverId`. */
  function useModel(serverId: string, alias: string) {
    if (app.gatewayRunning && isRemoteModel(alias) && !models.includes(alias)) models = [...models, alias];
    app.selectServer(serverId);
    if (pgTab !== 'chat') {
      modeModels[pgTab] = alias;
      if (pgTab === 'image') imageSession.model = alias;
      return;
    }
    settings.model = alias;
    // An explicit pick replaces the open chat's stored model (and any restore still pending).
    if (conv) conv.model_alias = alias;
    pendingModel = null;
  }

  /** "Load and Chat": a fresh chat on the model that was just loaded (or was already running). */
  async function startChatWith(serverId: string, alias: string) {
    pgTab = 'chat';
    app.selectServer(serverId);
    await newChat();
    useModel(serverId, alias);
    selectPreset(defaultPresetFor(alias));
  }
  $effect(() => {
    const pending = domain.pendingChat;
    if (!pending || !domain.presetsLoaded) return;
    domain.pendingChat = null;
    untrack(() => void startChatWith(pending.serverId, pending.alias));
  });

  function onConversationDeleted(id: string) {
    if (conv?.id === id) newChat();
  }

  function onConversationRenamed(id: string, title: string) {
    if (conv?.id === id) {
      conv.title = title;
      conv.title_source = 'user';
    }
  }

  /** One-time, verified move of browser-persisted Playground data into state.db. */
  async function migrateLocalState() {
    const legacyRaw = localStorage.getItem(LEGACY_STORAGE_KEY);
    const legacy = loadLegacyState();
    const settingsRaw = localStorage.getItem(DEFAULTS_KEY);
    const values = [
      { key: DEFAULTS_KEY, value: settingsRaw ?? (legacy ? JSON.stringify(legacy.settings) : null) },
      { key: COMPARE_KEY, value: localStorage.getItem(COMPARE_KEY) },
      { key: COMPACTION_DEFAULTS_KEY, value: localStorage.getItem(COMPACTION_DEFAULTS_KEY) },
    ].filter((item): item is { key: string; value: string } => item.value !== null);
    if (!legacy && values.length === 0) return;

    const msgs = legacy?.messages.filter((m) => m.content || m.reasoning || m.error || m.toolCalls?.length) ?? [];
    const firstUser = msgs.find((m) => m.role === 'user');
    const conversation = legacy
      ? {
          conversation: {
            title: firstUser ? fallbackTitle(firstUser.content) : '',
            title_source: firstUser ? ('fallback' as const) : ('none' as const),
            system_prompt: legacy.settings.systemPrompt,
            settings: settingsSnapshot(legacy.settings),
            model_alias: legacy.settings.model || null,
          },
          messages: msgs.map((m) => ({
            role: m.role === 'user' ? ('user' as const) : ('assistant' as const),
            variant: toVariantInput(m, m.role === 'assistant' ? legacy.settings.model : undefined),
          })),
        }
      : null;
    try {
      const result = await queue(() => convApi.importPlaygroundState({ conversation, values }));
      const verified = new Set(result.verified_keys);
      for (const item of values) {
        if (verified.has(item.key) && localStorage.getItem(item.key) === item.value) localStorage.removeItem(item.key);
      }
      const legacyDefaultsSafe = settingsRaw !== null || verified.has(DEFAULTS_KEY);
      if (
        legacy &&
        result.conversation_verified &&
        legacyDefaultsSafe &&
        localStorage.getItem(LEGACY_STORAGE_KEY) === legacyRaw
      ) {
        localStorage.removeItem(LEGACY_STORAGE_KEY);
        localStorage.removeItem(`${LEGACY_STORAGE_KEY}_migrated`);
      }
    } catch (e) {
      dbFail(e);
    }
  }

  async function init() {
    try {
      const st = await convApi.status();
      dbReadOnly = st.read_only;
      dbError = st.error ?? '';
    } catch (e) {
      dbError = String(e);
    }
    if (dbError) {
      // Keep temporary chat usable, without overwriting unread saved preferences.
      settings.toolsEnabled = false;
      settingsLoaded = true;
      return;
    }
    if (!dbReadOnly) await migrateLocalState();

    try {
      const [defSettings, defCompare] = await Promise.all([
        loadDefaultSettings(),
        loadCompareDefaults(),
        initGlobalCompaction(),
      ]);
      if (!conv && messages.length === 0) {
        settings = defSettings;
        settings.workdir = '';
        compare = defCompare;
      }
    } catch (e) {
      console.error('Failed to load playground defaults:', e);
    }

    try {
      const target = (await queue(() => convApi.lastOpened())) ?? (await queue(() => convApi.list({ limit: 1 })))[0]?.id;
      // Don't replace a chat the user already started while this was loading.
      if (target && !conv && messages.length === 0) {
        const view = await queue(() => convApi.open(target));
        if (!conv && messages.length === 0) {
          applyView(view, true);
          scrollToBottom(true);
        }
      }
    } catch (e) {
      dbFail(e);
    }
    listKey++;
    settingsLoaded = true;
  }

  // The selected server serves an embedding / rerank model: the picker shows that model (the chat
  // model of `settings` is not what the Embedding / Rerank tabs talk to).
  let pickerLabel = $derived(
    app.activeServer?.serve_mode && app.activeServer.serve_mode !== 'chat' && app.activeServer.model_path
      ? servedModelName(app.activeServer.model_path, domain.definitions)
      : undefined
  );

  // ---- Models (/v1/models; needed in router mode) ----
  // A gateway load or unload starts / stops a server: the model badges (loaded, loading, loads
  // on first request) are re-read then instead of staying as they were when the page opened.
  let serversSig = $derived(app.servers.map((s) => `${s.id}:${s.status}`).join(','));
  $effect(() => {
    void serversSig;
    if (!app.isRunning && !app.gatewayRunning) {
      models = [];
      modelStates = {};
      modelsError = false;
      return;
    }
    void app.baseUrl; // refetch when the server address changes
    void app.inferenceUrl; // ...or the gateway starts / stops
    untrack(refreshModels);
    return () => modelsAbort?.abort();
  });

  // The list is also read again when the backend says something changed: a gateway that opens
  // after the page (its first read found no endpoint, an error or an empty list), a definition
  // made by the CLI, a server that came up. Pushes arriving close together become one read.
  onMount(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const cleanups: (() => void)[] = [];
    const schedule = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        if (disposed || (!app.isRunning && !app.gatewayRunning)) return;
        void refreshModels();
      }, 300);
    };
    const visible = () => {
      if (document.visibilityState === 'visible') schedule();
    };
    for (const event of ['definitions-changed', 'servers-changed', 'gateway-changed', 'window-visible']) {
      void listen(event, schedule).then((unlisten) => {
        if (disposed) unlisten();
        else cleanups.push(unlisten);
      }).catch(() => {});
    }
    window.addEventListener('focus', schedule);
    document.addEventListener('visibilitychange', visible);
    return () => {
      disposed = true;
      clearTimeout(timer);
      for (const cleanup of cleanups) cleanup();
      window.removeEventListener('focus', schedule);
      document.removeEventListener('visibilitychange', visible);
    };
  });

  /**
   * Model a fresh chat started with. While the user has not picked another one, the chat follows
   * what actually runs (the last used model may be a gateway alias nobody has loaded).
   */
  let freshModel = untrack(() => settings.model);

  /** Gateway on: a model that is loaded now (the selected server's first), else null. */
  function runningModel(): string | null {
    if (!app.gatewayRunning) return null; // the list is exactly what the selected server serves
    const own = app.isRunning ? aliasOfServer(app.selectedServerId) : undefined;
    if (own && models.includes(own) && modelStates[own] === 'loaded') return own;
    return models.find((m) => modelStates[m] === 'loaded') ?? null;
  }

  /** Keeps a loaded model; an unloaded one gives way to a running one when there is one. */
  function preferRunningModel() {
    // Gateway off: the list is exactly the selected server's models. Gateway on: a server started
    // without a definition has no alias here, so fall back to whichever model is loaded.
    const own = app.isRunning && serveMode === 'chat'
      ? aliasOfServer(app.selectedServerId) ?? (app.gatewayRunning ? null : models[0] ?? null) : null;
    settings.model = selectChatModel(settings.model, chatModels, modelStates, own ?? runningModel());
    freshModel = settings.model;
  }

  function extractModelName(pathStr: string): string {
    const parts = pathStr.split(/[/\\]/);
    const file = parts[parts.length - 1] || pathStr;
    return file.replace(/\.gguf$/i, '');
  }

  async function refreshModels() {
    modelsAbort?.abort();
    const c = new AbortController();
    modelsAbort = c;
    const timer = setTimeout(() => c.abort(), 5000);
    modelsLoading = true;
    modelsError = false;
    try {
      // Gateway on: every model definition (the managed endpoint's /v1/models) with its load
      // state; off: what the selected server serves.
      const viaGateway = app.gatewayRunning;
      const res = viaGateway
        ? await fetch(`${app.inferenceUrl}/models`, { headers: app.inferenceHeaders(), signal: c.signal })
        : await fetch(`${app.baseUrl}/models`, { headers: app.authHeaders(), signal: c.signal });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      const list: GatewayModelEntry[] = Array.isArray(data?.data) ? data.data : [];
      const ids: string[] = list
        .map((m) => m?.id)
        .filter((id: unknown): id is string => typeof id === 'string' && id.length > 0);
      if (viaGateway) {
        const remote = await invoke<RemoteModel[]>('link_models');
        for (const model of remote) if (!ids.includes(model.id)) ids.push(model.id);
      }
      modelStates = viaGateway ? gatewayModelStates(list, (id) => domain.state(id)?.state) : {};
      const activeFallback =
        !viaGateway && app.activeServer?.model_path ? [extractModelName(app.activeServer.model_path)] : [];
      models = ids.length > 0 ? ids : activeFallback;
      const fresh = !conv && messages.length === 0 && settings.model === freshModel;
      const wanted = conv && pendingModel?.convId === conv.id ? pendingModel.alias : null;
      if (wanted && models.includes(wanted)) {
        if (settings.model === autoModel || !models.includes(settings.model)) settings.model = wanted;
        pendingModel = null;
      } else if (!chatModels.includes(settings.model)) {
        settings.model = chatModels[0] ?? '';
        autoModel = settings.model;
      }
      if (fresh) preferRunningModel();
    } catch {
      if (modelsAbort === c) {
        if (app.activeServer?.model_path && serveMode === 'chat') {
          const fallback = extractModelName(app.activeServer.model_path);
          models = [fallback];
          settings.model = fallback;
          autoModel = fallback;
          modelsError = false;
        } else {
          models = [];
          modelsError = true;
        }
      }
    } finally {
      clearTimeout(timer);
      if (modelsAbort === c) {
        modelsLoading = false;
        modelsAbort = null;
      }
    }
  }

  $effect(() => {
    if (!settingsLoaded || !settings.toolsEnabled) {
      untrack(() => {
        toolsAbort?.abort();
        toolsAbort = null;
        if (mcpPoll) clearTimeout(mcpPoll);
        mcpPoll = null;
        coreTools = [];
        toolLimits = null;
        toolsState = 'idle';
      });
      return;
    }
    // Core tools (and the MCP servers' tools) need no llama-server.
    mcpPolls = 0;
    untrack(refreshTools);
    return () => {
      toolsAbort?.abort();
      if (mcpPoll) clearTimeout(mcpPoll);
      mcpPoll = null;
    };
  });

  // ---- Tool generator (the user starts it from the Tools section) ----

  let toolGenOpen = $state(false);
  /** The chat model's endpoint, the same one the messages and the compaction go to. */
  let toolGenEndpoint = $derived(
    canChat && settings.model ? { base_url: app.inferenceUrl, headers: app.inferenceHeaders(), model: settings.model } : null,
  );
  /** The GGUF of the chat model when this server's own model answers (to warn about a small one). */
  // The chat model's file: its local definition first (the gateway serves models by alias, so the
  // selected server need not be the one that runs it), else what the selected server runs.
  let toolGenModelPath = $derived.by(() => {
    const def = domain.definitions.find((d) => d.alias.toLowerCase() === settings.model.toLowerCase());
    if (def?.base.kind === 'local') return def.base.path;
    return app.isRunning && (!app.gatewayRunning || aliasOfServer(app.selectedServerId)?.toLowerCase() === settings.model.toLowerCase())
      ? (app.activeServer?.model_path ?? null)
      : null;
  });

  /** Full schemas are supplied locally; discovery chooses which reach the model. */
  function wireToolDefs() {
    return activeTools.map((tl) => $state.snapshot(tl.definition));
  }

  // Tool schemas depend on discovery, so there is no fixed tool prefix to prewarm.

  let reasoningControl = $state<ReasoningControl>(NO_REASONING);
  let verifiedTools = $state<boolean | null>(null);
  $effect(() => {
    void serversSig;
    void library.version;
    const path = toolGenModelPath;
    const definition = domain.definitions.find(d => d.alias.toLowerCase() === settings.model.toLowerCase());
    verifiedTools = null;
    reasoningControl = NO_REASONING;
    libraryAudio = false;
    audioAliases = {};
    libraryVision = null;
    if (!path && definition?.base.kind !== 'hf') return;
    let cancelled = false;
    void invoke<LibraryView>('list_library').then((view) => {
      if (cancelled) return;
      const normalize = (p: string) => p.replace(/\\/g, '/').toLowerCase();
      const ref = definition?.base;
      const entry = view.entries.find(e =>
        (path && normalize(e.model.path) === normalize(path)) ||
        (ref?.kind === 'hf' && e.repo_id === ref.repo && normalize(e.model.path).endsWith(`/${normalize(ref.file)}`))
      );
      verifiedTools = entry?.caps.verified?.tools ?? null;
      reasoningControl = entry?.caps.reasoning_control ?? NO_REASONING;
      const projector = definition?.mmproj ? view.entries.find(e => normalize(e.model.path) === normalize(definition.mmproj!)) : undefined;
      libraryAudio = !!(projector?.caps.audio_input ?? entry?.caps.audio_input);
      libraryVision = projector?.caps.vision ?? entry?.caps.vision ?? null;
      audioAliases = Object.fromEntries(domain.definitions.map(d => [d.alias.toLowerCase(), !!d.mmproj &&
        !!view.entries.find(e => normalize(e.model.path) === normalize(d.mmproj!))?.caps.audio_input]));
    }).catch(() => {});
    return () => { cancelled = true; };
  });
  let composerToolsHint = $derived([
    t(settings.toolsEnabled ? 'pg.web_tools_enabled_hint' : 'pg.web_tools_disabled_hint'),
    settings.toolsEnabled ? t('w4.permissions') : '',
    toolsState === 'loading' ? t('pg.tools_loading') : '',
    toolsState === 'disabled' ? t('pg.web_tools_policy_off') : toolsState === 'error' ? toolsError : '',
    verifiedTools === false ? t('pg.web_tools_unsupported') : verifiedTools === null ? t('pg.web_tools_unmeasured') : '',
  ].filter(Boolean).join(' '));

  // The active project: its attached tools and skills, and a chat the Projects page asked for.
  onMount(() => { if (!projects.loaded) void projects.load(); });
  let toolsProject: string | null | undefined;
  $effect(() => {
    const id = projects.activeId;
    if (toolsProject !== undefined && toolsProject !== id && untrack(() => settings.toolsEnabled)) void refreshTools();
    toolsProject = id;
  });
  $effect(() => {
    const id = projects.pendingConversation;
    if (!id) return;
    projects.pendingConversation = null;
    void openConversation(id);
  });

  onMount(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    void listen('tools-changed', () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        if (!disposed && settings.toolsEnabled) void refreshTools();
      }, 100);
    }).then(unlisten => { if (disposed) unlisten(); else cleanup = unlisten; }).catch(() => {});
    return () => { disposed = true; clearTimeout(timer); cleanup?.(); };
  });

  async function refreshTools() {
    toolsAbort?.abort();
    const c = new AbortController();
    toolsAbort = c;
    toolsState = 'loading';
    toolsError = '';
    try {
      const [listing, catalog] = await Promise.all([listCoreTools(), invoke<SkillMetadata[]>('skills_list')]);
      if (toolsAbort !== c) return;
      skills = catalog.filter((s) => projectAllowsSkill(projects.active, s.name));
      if (listing.policy === 'off') {
        coreTools = [];
        toolLimits = null;
        toolsState = 'disabled';
        toolsAbort = null;
        return;
      }
      coreTools = listing.tools.filter((tl) => projectAllowsTool(projects.active, tl)).map(coreToolAsServerTool);
      toolLimits = listing.limits;
      toolsState = 'ok';
      void pollMcpStarting();
    } catch (e) {
      if (toolsAbort !== c) return;
      coreTools = [];
      toolLimits = null;
      toolsState = 'error';
      toolsError = e instanceof Error ? e.message : String(e);
    }
    if (toolsAbort === c) toolsAbort = null;
  }

  /** A first `npx` start can take a while: while an MCP server is still connecting, read the list again. */
  async function pollMcpStarting() {
    if (mcpPoll || mcpPolls >= 20) return;
    try {
      const st = await getMcpStatus(false);
      if (!st.some((s) => s.state === 'connecting')) return;
    } catch {
      return;
    }
    mcpPolls++;
    mcpPoll = setTimeout(() => {
      mcpPoll = null;
      if (settings.toolsEnabled) void refreshTools();
    }, 3000);
  }

  // ---- Context meter: last response's tokens vs. the server's per-slot context ----
  let propsCtx = $state<number | null>(null);
  /** `/props` `modalities.vision` of the selected server (null: not reported / not read yet). */
  let propsVision = $state<boolean | null>(null);
  let propsAudio = $state<boolean | null>(null);

  $effect(() => {
    if (!app.isRunning) {
      propsCtx = null;
      propsVision = null;
      propsAudio = null;
      return;
    }
    const url = app.hostUrl;
    void app.activeServer?.model_path; // refetch after a model change
    propsVision = null;
    propsAudio = null;
    const c = new AbortController();
    untrack(() => fetchContextSize(url, c));
    return () => c.abort();
  });

  async function fetchContextSize(url: string, c: AbortController) {
    const timer = setTimeout(() => c.abort(), 5000);
    try {
      // GET /props is always available (only POST needs --props).
      const res = await fetch(`${url}/props`, { headers: app.authHeaders(), signal: c.signal });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      const n = data?.default_generation_settings?.n_ctx ?? data?.n_ctx;
      propsCtx = typeof n === 'number' && n > 0 ? n : null;
      propsVision = visionFromProps(data);
      propsAudio = typeof data?.modalities?.audio === 'boolean' ? data.modalities.audio : null;
    } catch {
      if (!c.signal.aborted) {
        propsCtx = null;
        propsVision = null;
        propsAudio = null;
      }
    } finally {
      clearTimeout(timer);
    }
  }

  let contextSize = $derived(propsCtx ?? app.telemetry?.context_size ?? null);
  /** The compaction the next request applies (its summary replaces the older messages). */
  let activeCmp = $derived(activeCompaction(messages));
  /** Compaction settings in effect: the conversation's own, else the global defaults. */
  let cmpSettings = $derived(settings.compaction ?? compactionDefaults.value);
  let contextUsed = $derived.by(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      // Compacted since the last answer: the marker's own count (summary + kept turns).
      if (activeCmp?.markerIndex === i) return activeCmp.meta.tokens_after;
      const s = messages[i].stats;
      if (!s || s.source === 'none') continue;
      // Stored stats are normalized to cache-inclusive input on load.
      const used =
        (s.promptN ?? 0) + (s.predictedN ?? 0);
      if (used > 0) return used;
    }
    return null;
  });
  let draftTokens = $state(0);
  let draftTokensExact = $state(false);

  // History/tools are rebuilt only when they change; serialization runs after debounce.
  // Picture data stays out of the estimate and the token-count request (megabytes of base64);
  // each picture is counted as a fixed cost instead (`IMAGE_TOKEN_ESTIMATE`).
  // Held while a reply streams: each token would otherwise rebuild the whole history.
  let historyWithImages = $derived.by(
    settledWhile(() => streaming, () => buildApiMessages(messages, settings), () => messages.length),
  );
  let countHistory = $derived(withoutImages(historyWithImages));
  let imageEstimate = $derived((countImageParts(historyWithImages) + pendingImages.length) * IMAGE_TOKEN_ESTIMATE);
  let countTools = $derived(wireToolDefs());
  let ragEstimate = $derived.by(() => ragDocs.filter(doc => doc.status === 'ready').reduce((total, doc) =>
    total + (doc.mode === 'retrieval' ? Math.min(doc.tokens, settings.ragTopK * 512) : doc.tokens), 0));
  let promptBudget = $derived(estimatePromptBudget(withToolGuidance(countHistory, countTools), countTools, input));
  $effect(() => {
    if (streaming) return;
    const draft = input;
    const ragPending = ragDocs.some((d) => d.status === 'ready');
    const history = countHistory;
    const tools = countTools;
    const model = settings.model;
    const sampling = samplingParams($state.snapshot(settings) as PgSettings, { tools: tools.length > 0, reasoningControl });
    const ownServer = app.isRunning && (!app.gatewayRunning || aliasOfServer(app.selectedServerId)?.toLowerCase() === model.toLowerCase());
    const tokenize = ownServer ? { host_url: app.hostUrl, headers: app.authHeaders() } : null;
    draftTokensExact = false;
    draftTokens = promptBudget.inputTokens;
    let stale = false;
    const timer = setTimeout(async () => {
      const nextMessages = [...history, { role: 'user', content: draft }];
      const body: Record<string, unknown> = { messages: withToolGuidance(nextMessages, tools), ...sampling };
      if (model) body.model = model;
      if (tools.length) body.tools = tools;
      try {
        const [n, exact] = await compactionApi.countTokens(tokenize, body);
        if (!stale) { draftTokens = n; draftTokensExact = exact && !ragPending; }
      } catch { /* Keep the cheap estimate if IPC is unavailable. */ }
    }, 400);
    return () => { stale = true; clearTimeout(timer); };
  });

  let totalEstimate = $derived(draftTokens + ragEstimate + imageEstimate);
  let contextPressure = $derived.by(() => hasContextPressure(totalEstimate, ragContext, promptBudget.toolTokens));
  let contextRatio = $derived(contextSize ? totalEstimate / contextSize : 0);

  let conversationUsage = $derived.by(() => {
    let input = 0, output = 0, estimated = false;
    for (const m of messages) {
      for (const r of [...(m.priorRounds ?? []), m]) {
        const stats = r.stats;
        if (!stats || stats.source === 'none') continue;
        input += stats.promptN ?? 0;
        output += stats.predictedN ?? 0;
        estimated ||= stats.source === 'estimate';
      }
    }
    return { input, output, estimated };
  });
  let conversationTotalTokens = $derived(conversationUsage.input + conversationUsage.output);

  function fmtInt(n: number): string {
    return n.toLocaleString(locale(), { maximumFractionDigits: 0 });
  }

  // An unsent message keeps the page loaded while hidden to the tray.
  $effect(() => holdReleaseWhile('playground-input', () => input.trim().length > 0));
  // A tool call waiting for the user's approval: the agent loop lives in this page.
  $effect(() => holdReleaseWhile('playground-approval', () => awaitingApproval));

  onMount(() => {
    scrollToBottom(true);
    void domain.refreshPresets();
    void domain.refreshDefinitions();
    init();
    const unProgress = listen<RagProgress>('rag-progress', (e) => onRagProgress(e.payload));
    // Files dropped on the window (Tauri gives their paths) are attached to the conversation.
    const unDrop = getCurrentWebview().onDragDropEvent((e) => {
      const visible = !!rootEl && rootEl.offsetParent !== null && pgTab === 'chat' && persistEnabled;
      const p = e.payload;
      if (!visible || p.type === 'leave') {
        dropActive = false;
      } else if (p.type === 'enter' || p.type === 'over') {
        dropActive = true;
      } else if (p.type === 'drop') {
        dropActive = false;
        const { images, others } = splitImagePaths(p.paths);
        if (images.length) void addImagePaths(images);
        const audio = others.filter(p => /\.(wav|mp3|flac)$/i.test(p));
        if (audio.length && audioEnabled) void addAudioPaths(audio);
        const docs = others.filter(p => !/\.(wav|mp3|flac)$/i.test(p));
        if (docs.length) void attachFiles(docs);
      }
    });
    return () => {
      // The page keeps the Playground mounted across tabs; unmounting ends the stream (partial answer is kept).
      controller?.abort();
      unProgress.then((f) => f()).catch(() => {});
      unDrop.then((f) => f()).catch(() => {});
    };
  });

  // ---- Documents (local RAG, see playground/rag.ts) ----
  let ragDocs = $state<RagDocument[]>([]);
  let ragModels = $state<RagModels | null>(null);
  /** Documents are being searched for the next answer. */
  let ragSearching = $state(false);
  /** Files are dragged over the window. */
  let dropActive = $state(false);
  /** Document context of the running answer (applied to every request of the tool loop). */
  let turnRag: RagPrepared | null = null;
  let turnRagShown = false;

  $effect(() => {
    const id = conv?.id;
    if (!id) {
      ragDocs = [];
      return;
    }
    untrack(() => void refreshDocs(id));
  });

  // Embedding / rerank definitions: again when the gateway or the definitions change.
  $effect(() => {
    void app.gatewayRunning;
    void domain.definitions.length;
    untrack(() => {
      ragApi
        .models()
        .then((m) => (ragModels = m))
        .catch(() => (ragModels = null));
    });
  });

  /** Only the newest list request is applied: an older one (e.g. started when a new conversation
   *  was created, before its first document was added) would otherwise hide the new chips. */
  let docsRequest = 0;
  async function refreshDocs(id: string) {
    const req = ++docsRequest;
    try {
      const list = await ragApi.list(id);
      if (conv?.id === id && req === docsRequest) ragDocs = list;
    } catch (e) {
      dbFail(e);
    }
  }

  function onRagProgress(p: RagProgress) {
    if (p.conversation_id !== conv?.id) return;
    const d = ragDocs.find((x) => x.id === p.document_id);
    if (d) {
      d.status = p.status;
      if (p.total) {
        d.embedded = p.done;
        d.chunks = p.total;
      }
      if (p.error) d.error = p.error;
    }
    if (p.status === 'ready' || p.status === 'failed' || p.status === 'cancelled' || !d) void refreshDocs(p.conversation_id);
  }

  /** Alias documents are embedded with now (null: keywords only or no gateway). */
  function ragEmbedAlias(): string | null {
    if (!ragModels?.gateway) return null;
    return resolveEmbedModel(settings.ragEmbedModel, ragModels.embedding);
  }

  /** Context of the chat model in tokens (per slot), for "full text fits". */
  let ragContext = $derived.by(() => {
    const def = domain.definitions.find((d) => d.alias.toLowerCase() === settings.model.toLowerCase());
    const defCtx = Number(def?.load?.context_size) || null;
    const ownModel = app.isRunning && aliasOfServer(app.selectedServerId)?.toLowerCase() === settings.model.toLowerCase();
    if (app.gatewayRunning && !ownModel) return defCtx;
    return contextSize ?? defCtx;
  });

  /** Token count at which auto-compaction fires (null: off or the context is unknown). */
  let cmpLimit = $derived(cmpSettings.auto && ragContext ? thresholdTokens(cmpSettings, ragContext) : null);

  async function attachFiles(paths?: string[]) {
    if (!persistEnabled) {
      toast.error(t('pg.db_read_only'));
      return;
    }
    const files = paths ?? (await ragApi.pick(t('doc.picker_filter')).catch(() => [] as string[]));
    if (!files.length) return;
    const name = files[0].split(/[/\\]/).pop() ?? files[0];
    const convId = conv?.id ?? (await ensureConversation(name));
    if (!convId) return;
    try {
      const added = await ragApi.add(convId, files, ragEmbedAlias());
      if (conv?.id === convId) {
        docsRequest++;
        ragDocs = [...ragDocs, ...added.filter((a) => !ragDocs.some((d) => d.id === a.id))];
      }
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  // ---- Pictures (vision models) ----

  /**
   * Whether the chat model reads pictures. The selected server's own `/props` answers for a
   * model it serves; a gateway model that is not the selected server has a projector only when
   * its definition names one. Unknown (no definition, no `/props`) does not block.
   */
  let imagesBlocked = $derived.by(() => {
    const model = settings.model.toLowerCase();
    const own = app.isRunning && (!app.gatewayRunning || aliasOfServer(app.selectedServerId)?.toLowerCase() === model);
    if (own && propsVision !== null) return !propsVision;
    const def = domain.definitions.find((d) => d.alias.toLowerCase() === model);
    if (libraryVision !== null) return !libraryVision;
    return def ? !def.mmproj && !own : false;
  });
  let imagesBlockedHint = $derived(t('pg.image_needs_vision'));
  let audioEnabled = $derived.by(() => {
    if (splitOn) return SIDES.every(side => {
      const target = parseTarget(compare[side].target);
      if (!target) return false;
      if (target.kind === 'server' && target.serverId === app.selectedServerId && propsAudio !== null) return propsAudio;
      const alias = target.kind === 'gateway' ? target.alias : aliasOfServer(target.serverId);
      return !!alias && !!audioAliases[alias.toLowerCase()];
    });
    const own = app.isRunning && (!app.gatewayRunning || aliasOfServer(app.selectedServerId)?.toLowerCase() === settings.model.toLowerCase());
    if (own && propsAudio !== null) return propsAudio;
    const def = domain.definitions.find(d => d.alias.toLowerCase() === settings.model.toLowerCase());
    return !!def?.mmproj && libraryAudio;
  });
  async function addAudioFiles(files: File[]) {
    if (!audioEnabled) return;
    const model = settings.model;
    const version = workspaceVersion;
    for (const file of files) {
      try {
        if (pendingAudio.length >= MAX_AUDIO_PER_MESSAGE) throw new AudioError('limit', file.name);
        const clip = await loadAudio(file);
        if (!audioEnabled || settings.model !== model || workspaceVersion !== version) return;
        if (pendingAudio.length >= MAX_AUDIO_PER_MESSAGE) throw new AudioError('limit', file.name);
        pendingAudio = [...pendingAudio, clip];
      } catch (e) {
        toast.error(e instanceof AudioError ? t(`pg.audio_err_${e.code}`, { name: e.file, mb: String(MAX_AUDIO_BYTES / 1024 / 1024), max: String(MAX_AUDIO_PER_MESSAGE) }) : errorText(e));
      }
    }
  }
  async function addAudioPaths(paths: string[]) {
    if (!audioEnabled) return;
    const model = settings.model;
    const version = workspaceVersion;
    const files: File[] = [];
    for (const path of paths) {
      try {
        const data = await invoke<string>('read_audio_file', { path });
        const bytes = Uint8Array.from(atob(data), c => c.charCodeAt(0));
        files.push(new File([bytes], path.split(/[/\\]/).pop() ?? path));
      } catch (e) { toast.error(errorText(e)); }
    }
    if (audioEnabled && settings.model === model && workspaceVersion === version) await addAudioFiles(files);
  }
  async function pickAudio() {
    const paths = await invoke<string[]>('pick_audio_files', { label: t('pg.audio_picker_filter') }).catch(() => []);
    await addAudioPaths(paths);
  }

  function imageErrorText(e: unknown): string {
    if (e instanceof ImageError) {
      return t(`pg.image_err_${e.code}`, { name: e.file, edge: String(MAX_IMAGE_EDGE), mb: String(MAX_IMAGE_BYTES / (1024 * 1024)), max: String(MAX_IMAGES_PER_MESSAGE) });
    }
    return errorText(e);
  }

  /** Adds pictures (button, paste or drop) to the message being written. */
  async function addImages(load: (() => Promise<PgImage>)[]) {
    if (imagesBlocked) {
      toast.error(imagesBlockedHint);
      return;
    }
    for (const read of load) {
      if (pendingImages.length >= MAX_IMAGES_PER_MESSAGE) {
        toast.error(imageErrorText(new ImageError('limit', '')));
        return;
      }
      try {
        pendingImages = [...pendingImages, await read()];
      } catch (e) {
        toast.error(imageErrorText(e));
      }
    }
    tick().then(() => composer?.focus(true));
  }

  const addImageFiles = (files: File[]) => addImages(files.map((f) => () => loadImage(f)));
  const addImagePaths = (paths: string[]) => addImages(paths.map((p) => () => loadImagePath(p)));
  async function pickImages() {
    const paths = await invoke<string[]>('pick_image_files', { label: t('pg.image_picker_filter') }).catch(() => [] as string[]);
    if (paths.length) addImagePaths(paths);
  }

  async function reindexDocs(ids?: string[]) {
    const c = conv;
    if (!c) return;
    try {
      await ragApi.reindex(c.id, ragEmbedAlias(), ids);
      await refreshDocs(c.id);
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  async function setDocMode(id: string, mode: RagMode) {
    try {
      const d = await ragApi.setMode(id, mode);
      ragDocs = ragDocs.map((x) => (x.id === id ? d : x));
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  async function removeDoc(id: string) {
    try {
      await ragApi.remove(id);
      ragDocs = ragDocs.filter((x) => x.id !== id);
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  /** Searches the conversation's documents for the last user message (before the answer). */
  async function prepareRag(convId: string | null): Promise<RagPrepared | null> {
    if (!convId || !ragDocs.some((d) => d.status === 'ready')) return null;
    const query = [...messages].reverse().find((m) => m.role === 'user')?.content ?? '';
    ragSearching = true;
    try {
      return await ragApi.prepare({
        conversation_id: convId,
        query,
        top_k: settings.ragTopK,
        context_tokens: ragContext,
        embed_model: ragEmbedAlias(),
        rerank_model: ragModels?.gateway && settings.ragRerankModel ? settings.ragRerankModel : null,
      });
    } catch (e) {
      // Answer without documents, and say why under the answer.
      return { system: null, user: null, sources: [], retrieval: null, reranked: null, notices: [rawError(e)], tokens: 0 };
    } finally {
      ragSearching = false;
    }
  }

  // ---- Scrolling ----
  function onListScroll() {
    if (!listEl) return;
    stickToBottom = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 48;
    if (needsOlder(shownStart, listEl.scrollTop)) void mountOlder();
  }

  let mountingOlder = false;
  /** True while older messages are being added above: the log's live region stays quiet so they are not read out. */
  let olderBusy = $state(false);
  /** Mounts the next batch of older messages above the view and keeps the same content under the viewport. */
  async function mountOlder(batch?: number) {
    if (mountingOlder || !listEl || shownStart <= 0) return;
    mountingOlder = true;
    olderBusy = true;
    try {
      const el = listEl;
      const top = el.scrollTop;
      const before = el.scrollHeight;
      winStart = batch ? Math.max(0, shownStart - batch) : previousBatch(shownStart);
      await tick();
      // Chromium's own scroll anchoring may already have compensated; only fix what it left.
      if (Math.abs(el.scrollTop - top) < 1) el.scrollTop = anchoredScroll(top, before, el.scrollHeight);
    } finally {
      mountingOlder = false;
      requestAnimationFrame(() => (olderBusy = false));
    }
    // A tall viewport (or short messages) can still show the top: keep going until it is covered.
    if (listEl && needsOlder(shownStart, listEl.scrollTop)) void mountOlder();
  }

  // The rest of the history mounts in small batches while the page is idle, so scrolling up
  // rarely waits for a batch and the browser's find (Ctrl+F) reaches the whole chat soon after open.
  $effect(() => {
    const start = shownStart;
    const id = conv?.id;
    if (start <= 0 || streaming) return;
    return whenIdle(() => {
      if (conv?.id === id && !streaming) void mountOlder(IDLE_HISTORY_BATCH);
    });
  });

  /** Keyboard / screen reader path to older history: one batch per press, focus stays at the top. */
  let olderButton = $state<HTMLButtonElement | null>(null);
  // When the last batch mounts (by the button or while idle) the button goes away: hand its focus
  // to the first message instead of dropping it to the page. Runs before the DOM update removes it.
  $effect.pre(() => {
    if (shownStart > 0 || !olderButton || document.activeElement !== olderButton) return;
    void tick().then(() => listEl?.querySelector<HTMLElement>('article')?.focus());
  });

  /** The running follow-the-end loop of scrollToBottom; a new call restarts its frame count. */
  let bottomSettle: { frames: number } | null = null;
  function scrollToBottom(force = false) {
    if (force) stickToBottom = true;
    if (scrollQueued) return;
    scrollQueued = true;
    requestAnimationFrame(() => {
      scrollQueued = false;
      if (listEl && stickToBottom) {
        listEl.scrollTop = listEl.scrollHeight;
        // Messages rendered for the first time (content-visibility) can turn out taller than
        // their estimate and push the end down: follow it for a few frames.
        // The growth itself fires scroll events that clear stickToBottom, so only a user input stops this.
        if (bottomSettle) bottomSettle.frames = 0;
        else followEnd(listEl);
      }
      // A short tail that does not scroll never fires a scroll event: fill upwards from here.
      if (listEl && listEl.scrollHeight - listEl.clientHeight < 600 && needsOlder(shownStart, listEl.scrollTop)) void mountOlder();
    });
  }

  /** Keeps the list at its end for a few frames while newly rendered messages settle their height. */
  function followEnd(el: HTMLDivElement) {
    const state = { frames: 0 };
    bottomSettle = state;
    let userMoved = false;
    const stop = () => (userMoved = true);
    const opts = { once: true, passive: true } as const;
    el.addEventListener('wheel', stop, opts);
    el.addEventListener('pointerdown', stop, opts);
    el.addEventListener('keydown', stop, opts);
    const settle = () => {
      if (userMoved || el !== listEl || ++state.frames > 8) {
        bottomSettle = null;
        el.removeEventListener('wheel', stop);
        el.removeEventListener('pointerdown', stop);
        el.removeEventListener('keydown', stop);
        return;
      }
      if (el.scrollHeight - el.scrollTop - el.clientHeight > 1) {
        el.scrollTop = el.scrollHeight;
        stickToBottom = true;
      }
      requestAnimationFrame(settle);
    };
    requestAnimationFrame(settle);
  }

  // ---- Composer ----
  async function send() {
    const text = input.trim();
    const images = $state.snapshot(pendingImages) as PgImage[];
    const audio = $state.snapshot(pendingAudio) as PgAudio[];
    if (audio.length && !audioEnabled) { toast.error(t('pg.audio_needs_model')); return; }
    if ((!text && !images.length && !audio.length) || streaming || compacting || !canChat || composerWaiting) return;
    if (images.length && imagesBlocked) {
      toast.error(imagesBlockedHint);
      return;
    }
    const command = images.length || audio.length ? null : parseCompactCommand(text);
    if (command && !splitOn) {
      input = '';
      await runCompaction('manual', command.instruction);
      return;
    }
    if (splitOn) return sendSplit(text, images, audio);
    input = '';
    pendingImages = [];
    pendingAudio = [];
    streaming = true; // blocks a second send while the conversation is being created
    const convId = await ensureConversation(text || images[0]?.name || audio[0]?.name || '');
    await maybeAutoCompact(convId, text);
    const parent = messages.at(-1);
    messages.push({ id: newId(), role: 'user', content: text, ...(images.length ? { images } : {}), ...(audio.length ? { audio } : {}), variantId: newId(), parentId: parent?.id ?? null });
    saveMessage(messages[messages.length - 1], convId);
    await runAgent(convId);
  }

  function stop() {
    controller?.abort();
    for (const c of splitControllers) c.abort();
    if (compacting && conv) void compactionApi.cancel(conv.id);
  }

  function isTurnEnd(i: number): boolean {
    return turnEndAt(messages, i);
  }

  // ---- Context compaction (see playground/compaction.svelte.ts, harness_core::compaction) ----

  /** A summary is being written (sending waits; Stop cancels it). */
  let compacting = $state(false);

  /** Context of a model definition (gateway alias), for a summary model other than the chat model. */
  function definitionContext(alias: string): number | null {
    const def = domain.definitions.find((d) => d.alias.toLowerCase() === alias.toLowerCase());
    return Number(def?.load?.context_size) || null;
  }

  /**
   * Summarizes the older turns of the open conversation into a marker. True when it did.
   * `midTurn`: between two tool rounds of the running answer (the running turn's question stays
   * verbatim, see `harness_core::compaction::plan_mid_turn`); then a stored-but-too-small or
   * non-shrinking summary is not an error to show, the loop just goes on.
   */
  async function runCompaction(trigger: 'manual' | 'auto', instruction = '', midTurn = false): Promise<boolean> {
    const c = conv;
    const manual = trigger === 'manual';
    if (!c || !persistEnabled || compacting) {
      if (manual) toast.error(t(persistEnabled ? 'cmp.nothing' : 'pg.db_read_only'));
      return false;
    }
    const ctx = ragContext;
    if (!ctx) {
      if (manual) toast.error(t('cmp.no_context'));
      return false;
    }
    const s = $state.snapshot(cmpSettings);
    const other = !!s.summaryModel && app.gatewayRunning && models.includes(s.summaryModel) && s.summaryModel !== settings.model;
    const summaryModel = other ? s.summaryModel : settings.model;
    // Exact counts from the chat model's own llama-server when it is the selected one; else
    // the backend asks the endpoint (count_tokens) or estimates.
    const ownServer =
      app.isRunning && (!app.gatewayRunning || aliasOfServer(app.selectedServerId)?.toLowerCase() === settings.model.toLowerCase());
    const extra = [s.instruction.trim(), instruction.trim()].filter(Boolean).join('\n');
    compacting = true;
    try {
      // Behind the pending saves (the answer so far is written by the same queue).
      const out = await queue(() => compactionApi.compact({
        conversation_id: c.id,
        leaf_id: messages.at(-1)?.id ?? null,
        endpoint: { base_url: app.inferenceUrl, headers: app.inferenceHeaders(), model: summaryModel || null },
        tokenize: ownServer ? { host_url: app.hostUrl, headers: app.authHeaders() } : null,
        n_ctx: ctx,
        summary_ctx: other ? definitionContext(summaryModel) : null,
        keep_turns: s.keepTurns,
        clear_tool_results: s.clearToolResults,
        summary_tokens: s.summaryTokens || null,
        instruction: extra || null,
        trigger,
        mid_turn: midTurn,
        documents: ragDocs.filter((d) => d.status === 'ready').map((d) => d.name),
        system_prompt: settings.systemPrompt,
      }));
      if (conv?.id === c.id) applyView(out.view, false);
      const numbers = { n: out.marker.covered, before: fmtInt(out.marker.tokens_before), after: fmtInt(out.marker.tokens_after) };
      if (manual) toast.success(t('cmp.done', numbers));
      else toast.info(t(midTurn ? 'cmp.done_mid' : 'cmp.done_auto', numbers));
      scrollToBottom(true);
      return true;
    } catch (e) {
      const key = e instanceof BackendError ? (e.code ?? '') : '';
      // Automatic: too little to summarize yet is not an error; the message is sent as it is.
      if (!manual && (key === 'compaction.too_little' || key === 'compaction.nothing_to_compact')) return false;
      if (key === 'compaction.cancelled') return false;
      // Between tool rounds a summary that would not shrink the request (`compaction.no_gain`)
      // is dropped without a word: the answer carries on, and no further attempt is made.
      if (midTurn && key === 'compaction.no_gain') return false;
      toast.error(manual ? errorText(e) : t(midTurn ? 'cmp.mid_failed' : 'cmp.auto_failed', { message: errorText(e) }));
      return false;
    } finally {
      compacting = false;
    }
  }

  /** Before a new message: compacts first when the conversation reached the threshold. */
  async function maybeAutoCompact(convId: string | null, text: string) {
    const s = cmpSettings;
    const ctx = ragContext;
    const used = contextUsed;
    if (!convId || !s.auto || !persistEnabled || !ctx || used === null) return;
    // An undone compaction pauses auto-compaction on this branch (it would redo it at once).
    if (autoPaused(messages)) return;
    if (used + estimateTokens(text) < thresholdTokens(s, ctx)) return;
    await runCompaction('auto');
  }

  /** Set while an answer runs: a mid-turn compaction failed or gained nothing, do not try again. */
  let midTurnBlocked = false;

  /**
   * Between two tool rounds of a running answer: compacts when the conversation (the last
   * request, its answer and the tool results about to be sent back) reached the threshold, so a
   * long tool loop does not run into the context limit. Consistent with the compaction before a
   * new message (`maybeAutoCompact`): same settings, same marker, same undo.
   */
  async function compactBetweenRounds(convId: string | null, last: Msg, signal: AbortSignal) {
    const s = cmpSettings;
    const ctx = ragContext;
    if (midTurnBlocked || signal.aborted || !convId || !s.auto || !persistEnabled || !ctx || compacting) return;
    if (autoPaused(messages)) return;
    const st = last.stats;
    if (!st || st.source === 'none') return;
    const sent =
      (st.promptN ?? 0) + (st.predictedN ?? 0);
    // The results of this round's tools join the next request.
    const results = (last.toolCalls ?? []).reduce((n, call) => n + estimateTokens(call.result ?? ''), 0);
    if (sent + results < thresholdTokens(s, ctx)) return;
    const done = await runCompaction('auto', '', true);
    if (!done) midTurnBlocked = true;
  }

  /** Undo (the full history is sent again) or re-apply a compaction. */
  async function setCompactionUndone(m: Msg, undone: boolean) {
    if (streaming || compacting || !persistEnabled) return;
    try {
      applyView(await queue(() => compactionApi.setUndone(m.id, undone)), false);
      toast.success(undone ? t('cmp.undone_toast') : t('cmp.redone_toast'));
    } catch (e) {
      dbFail(e);
    }
  }

  let chatCount = $derived(messages.filter((m) => m.role !== 'compaction').length);

  function variantNavFor(i: number): { index: number; count: number } | null {
    return variantNavOf(messages, i);
  }

  /**
   * Regenerates the whole answer of a turn as a new variant (or re-sends a last unanswered
   * message). Works on any turn of the history: the messages after it stay in the tree under the
   * old variant, so the arrows (< 1/3 >) bring that continuation back.
   */
  async function regenerate(index: number) {
    if (streaming || !canChat) return;
    const convId = conv?.id ?? null;
    const plan = regeneratePlan(messages, index, (v) => savedVariants.has(v));
    messages = messages.slice(0, plan.keep);
    await runAgent(convId, plan.variantOf);
  }

  /**
   * "Edit" on a user message anywhere in the history: the edited text becomes a sibling of the
   * message (same parent), the messages after it stay on the old branch, and the answer is
   * generated from there. The sibling arrows next to the message bring the old branch back.
   */
  async function editAndResend(index: number, text: string) {
    const old = messages[index];
    const content = text.trim();
    if (streaming || compacting || !canChat || !old || old.role !== 'user' || !content) return;
    if (content === old.content.trim()) return;
    const stored = !!conv && persistEnabled && !!old.variantId && savedVariants.has(old.variantId);
    const convId = conv?.id ?? null;
    if (stored && convId) {
      try {
        const edited = await queue(() => convApi.editUserMessage(old.id, content));
        applyView(await queue(() => convApi.get(convId)), false);
        if (messages.at(-1)?.id !== edited.id) return;
      } catch (e) {
        dbFail(e);
        return;
      }
    } else {
      messages = [...messages.slice(0, index), { id: newId(), role: 'user', content, images: old.images, audio: old.audio, variantId: newId(), parentId: old.parentId ?? null }];
      if (convId) saveMessage(messages[messages.length - 1], convId);
    }
    stickToBottom = true;
    await runAgent(convId);
  }

  // ---- Edit from here (rewind) ----

  /** Pending confirmation: the user message to rewind to and how many messages follow it. */
  let rewindAsk = $state<{ index: number; later: number } | null>(null);

  /**
   * "Edit" on a user message (also in the split view): rewinds the chat to just before it. The
   * message and everything after it leave the active path, the original text goes back into the
   * input to be edited and sent again. Nothing is deleted: what was sent from here on stays in
   * the conversation tree as the other branch (the arrows next to the new message bring it
   * back, and so does "Undo" in the toast). Asks first when messages would disappear.
   */
  function requestRewind(index: number) {
    if (streaming || compacting || messages[index]?.role !== 'user') return;
    const later = messages.slice(index + 1).filter((m) => m.role !== 'compaction').length;
    if (later === 0) void rewindTo(index);
    else rewindAsk = { index, later };
  }

  async function rewindTo(index: number) {
    rewindAsk = null;
    if (streaming || compacting) return;
    const old = messages[index];
    if (!old || old.role !== 'user') return;
    const stored = !!conv && persistEnabled && !!old.variantId && savedVariants.has(old.variantId);
    if (stored) {
      try {
        applyView(await queue(() => convApi.rewindBefore(old.id)), false);
      } catch (e) {
        dbFail(e);
        return;
      }
    } else {
      messages = messages.slice(0, index);
    }
    const before = input;
    // Keep what the user had already typed; the rewound text goes first.
    input = before.trim() ? `${old.content}\n\n${before}` : old.content;
    // The pictures of the message go back into the composer with its text.
    if (old.audio?.length) pendingAudio = [...old.audio, ...pendingAudio].slice(0, MAX_AUDIO_PER_MESSAGE);
    if (old.images?.length) pendingImages = [...old.images, ...pendingImages].slice(0, MAX_IMAGES_PER_MESSAGE);
    stickToBottom = true;
    tick().then(() => composer?.focus(true));
    toast.info(t('pg.rewind_done'), {
      duration: ACTION_TOAST_DURATION,
      action: stored
        ? {
            label: t('pg.rewind_undo'),
            onClick: () => void undoRewind(old, before),
          }
        : undefined,
    });
  }

  /** Brings the rewound branch back (the original message and what followed it). */
  async function undoRewind(old: Msg, previousInput: string) {
    if (streaming || compacting || !conv) return;
    try {
      applyView(await queue(() => convApi.selectBranch(old.id)), false);
      input = previousInput;
      scrollToBottom(true);
    } catch (e) {
      dbFail(e);
    }
  }

  async function switchVariant(index: number, dir: -1 | 1) {
    if (streaming || !conv) return;
    const root = messages[turnRoot(messages, index)];
    const target = variantTarget(root, dir);
    if (!target) return;
    try {
      applyView(await queue(() => convApi.selectVariant(root.id, target)), false);
    } catch (e) {
      dbFail(e);
    }
  }

  async function switchSibling(index: number, dir: -1 | 1) {
    if (streaming || !conv) return;
    const m = messages[index];
    const target = m.siblingIds?.[(m.siblingIndex ?? 0) + dir];
    if (!target) return;
    try {
      applyView(await queue(() => convApi.selectBranch(target)), false);
    } catch (e) {
      dbFail(e);
    }
  }

  async function forkFrom(index: number) {
    const m = messages[index];
    if (streaming || !conv || !m.variantId || !savedVariants.has(m.variantId)) return;
    try {
      const title = t('pg.fork_title', { title: conv.title.trim() || t('pg.conv_untitled') });
      const c = await queue(() => convApi.fork(m.id, title));
      await openConversation(c.id);
      listKey++;
      toast.success(t('pg.forked'));
    } catch (e) {
      dbFail(e);
    }
  }

  function useExample(text: string) {
    input = text;
    tick().then(() => composer?.focus(true));
  }

  function resetSettings() {
    settings = {
      ...DEFAULT_SETTINGS,
      request: { sampling: {}, reasoning: {} },
      model: settings.model,
      toolsEnabled: settings.toolsEnabled,
      workdir: settings.workdir,
      disabledTools: [...settings.disabledTools],
      autoTitle: settings.autoTitle,
    };
    selectPreset(defaultPresetFor(settings.model));
  }

  // ---- Split view (see playground/split.ts) ----

  /**
   * Panes when the split view is first turned on: the current model on both sides, the current
   * preset on A and another one on B (two presets, one model); either side can then change.
   */
  function initPanes() {
    const current =
      app.gatewayRunning && settings.model
        ? gwTarget(settings.model)
        : app.isRunning
          ? srvTarget(app.selectedServerId)
          : '';
    if (!compare.a.target) compare.a.target = current;
    if (!compare.b.target) compare.b.target = current;
    if (!compare.a.presetId && !compare.b.presetId) {
      compare.a.presetId = settings.presetId;
      compare.b.presetId = domain.presets.find((p) => p.id !== settings.presetId)?.id ?? settings.presetId;
    }
  }

  function toggleCompare() {
    if (streaming) return;
    if (!compare.enabled) initPanes();
    compare.enabled = !compare.enabled;
    splitNotice = '';
    pgTab = 'chat';
    tick().then(() => {
      if (compare.enabled) splitView?.scrollToBottom(true);
      else scrollToBottom(true);
      composer?.focus();
    });
  }

  /** Ctrl+Shift+M toggles the split view while the Playground is on screen. */
  function onGlobalKeydown(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || !e.shiftKey || e.altKey || e.key.toLowerCase() !== 'm') return;
    if (!rootEl || rootEl.offsetParent === null) return;
    e.preventDefault();
    toggleCompare();
  }

  /** Alias of the model definition a running server serves (for `model` and the gateway). */
  function aliasOfServer(serverId: string): string | undefined {
    return domain.states.find((s) => s.servers.some((x) => x.server_id === serverId))?.alias;
  }

  /** Where a pane's requests go; `null` when the target is not available now. */
  async function endpointFor(p: PaneConfig): Promise<Endpoint | null> {
    const tg = parseTarget(p.target);
    if (!tg) return null;
    if (tg.kind === 'gateway') {
      if (!app.gatewayRunning) return null;
      return { baseUrl: app.inferenceUrl, headers: app.inferenceHeaders(), model: tg.alias, kind: 'gateway' };
    }
    const s = app.servers.find((x) => x.id === tg.serverId);
    if (!s?.active || s.status !== 'Running') return null;
    let headers: Record<string, string> = {};
    if (s.active.api_key_required ?? s.active.api_key_set) {
      try {
        const key = await invoke<string | null>('get_server_api_key', { serverId: s.id });
        if (key) headers = { Authorization: `Bearer ${key}` };
      } catch {
        // the request reports the missing key
      }
    }
    return {
      baseUrl: `http://${s.active.connect_host}:${s.active.port}/v1`,
      headers,
      model: aliasOfServer(s.id) ?? modelFileName(s.active.model_path),
      kind: 'server',
      serverId: s.id,
    };
  }

  function systemPromptFor(p: PaneConfig): string {
    return domain.preset(p.presetId)?.system_prompt || settings.systemPrompt;
  }

  /** Before sending: can both sides answer at the same time? (the requests still decide) */
  async function planConcurrency(a: Endpoint, b: Endpoint): Promise<{ sequential: boolean; notice: string }> {
    if (compare.sequential) return { sequential: true, notice: t('pgs.seq_forced') };
    if (a.kind !== 'gateway' || b.kind !== 'gateway') return { sequential: false, notice: '' };
    const same = a.model.toLowerCase() === b.model.toLowerCase();
    try {
      const r = await invoke<CoResidency | null>('gateway_co_residency', { models: same ? [a.model] : [a.model, b.model] });
      if (r && !r.concurrent) {
        const notice =
          r.reason === 'max_loaded_models'
            ? t('pgs.seq_max_loaded', { limit: r.limit ?? 1 })
            : t('pgs.seq_memory', { need: formatBytes(r.need_bytes ?? 0, 1), free: formatBytes(r.available_bytes ?? 0, 1) });
        return { sequential: true, notice };
      }
      if (r && same && r.slots === 1) return { sequential: false, notice: t('pgs.one_slot') };
    } catch {
      // unknown model or gateway error: the requests report it
    }
    return { sequential: false, notice: '' };
  }

  /** Panes, endpoints and the fresh turn of one split run. Null (with a toast) when a side is not available. */
  async function prepareSplitRun() {
    const panes = { a: $state.snapshot(compare.a) as PaneConfig, b: $state.snapshot(compare.b) as PaneConfig };
    const [epA, epB] = await Promise.all([endpointFor(panes.a), endpointFor(panes.b)]);
    if (!epA || !epB) {
      toast.error(t('pgs.pick_target'));
      return null;
    }
    return { panes, endpoints: { a: epA, b: epB } as Record<Side, Endpoint> };
  }

  /** Both sides' empty answers of one split run (they share `turnId`, the stored message). */
  function newSides(
    userId: string,
    turnId: string,
    panes: Record<Side, PaneConfig>,
    endpoints: Record<Side, Endpoint>
  ): Msg[] {
    const group = newId();
    return SIDES.map((side) => {
      const m = emptySide(side, userId);
      m.id = turnId;
      const ep = endpoints[side];
      m.compareMeta = {
        group,
        model: ep.model,
        target: ep.kind,
        server_id: ep.serverId ?? null,
        preset_id: panes[side].presetId || null,
        preset_name: ((p) => (p ? presetName(p) : null))(domain.preset(panes[side].presetId)),
      };
      return m;
    });
  }

  /** Sends one message to both sides (at once, or A then B when they cannot run together). */
  async function sendSplit(text: string, images: PgImage[] = [], audio: PgAudio[] = []) {
    streaming = true; // blocks a second send while the endpoints resolve
    const run = await prepareSplitRun();
    if (!run) {
      streaming = false;
      return;
    }
    input = '';
    pendingImages = [];
    pendingAudio = [];
    const convId = await ensureConversation(text || images[0]?.name || audio[0]?.name || '');
    const parent = messages.at(-1);
    messages.push({ id: newId(), role: 'user', content: text, ...(images.length ? { images } : {}), ...(audio.length ? { audio } : {}), variantId: newId(), parentId: parent?.id ?? null });
    const user = messages[messages.length - 1];
    // After a split turn the new message belongs to both sides.
    saveMessage(user, convId, !!parent && isSplit(parent));
    const turnId = newId();
    const sides = newSides(user.id, turnId, run.panes, run.endpoints);
    messages.push({
      id: turnId,
      role: 'assistant',
      content: '',
      streaming: true,
      variantId: sides[0].variantId,
      parentId: user.id,
      compareSides: sides,
    });
    await runSplitTurn(messages[messages.length - 1], convId, run, false);
  }

  /**
   * Regenerates the last split turn: both sides answer the same question again, with the
   * current panes. The new pair is stored as another comparison group of the same message (the
   * arrows on the turn bring the earlier pair back).
   */
  async function regenerateSplit() {
    const turn = messages.at(-1);
    if (streaming || !canChat || !turn || !isSplit(turn)) return;
    streaming = true;
    const run = await prepareSplitRun();
    if (!run) {
      streaming = false;
      return;
    }
    const user = messages[messages.length - 2];
    const sides = newSides(user?.id ?? turn.parentId ?? '', turn.id, run.panes, run.endpoints);
    const stored = !!turn.variantId && savedVariants.has(turn.variantId);
    turn.compareSides = sides;
    turn.compareNav = undefined;
    turn.streaming = true;
    turn.variantId = sides[0].variantId;
    await runSplitTurn(turn, conv?.id ?? null, run, stored);
  }

  /** Runs both sides of `turn` (its `compareSides`) and stores the result. `again`: the turn is already stored. */
  async function runSplitTurn(
    turn: Msg,
    convId: string | null,
    run: { panes: Record<Side, PaneConfig>; endpoints: Record<Side, Endpoint> },
    again: boolean
  ) {
    const { panes, endpoints } = run;
    // History without the turn itself (every side's own earlier answers, tool rounds included).
    const history = ($state.snapshot(messages) as Msg[]).filter((m) => m.id !== turn.id);
    const live: Record<Side, Msg> = { a: turn.compareSides![0], b: turn.compareSides![1] };
    splitView?.scrollToBottom(true);

    turnRag = await prepareRag(convId);
    for (const side of SIDES) {
      // The sources belong to every side's answer.
      if (turnRag) live[side].rag = { sources: turnRag.sources, retrieval: turnRag.retrieval, reranked: turnRag.reranked, notices: turnRag.notices };
    }
    const plan = await planConcurrency(endpoints.a, endpoints.b);
    splitNotice = plan.notice;
    const texts = {
      http: (status: number, message: string) => t('pg.error_http', { status, message }),
      stream: (message: string) => t('pg.error_stream', { message }),
      network: (message: string) => t('pg.error_network', { message }),
      noBody: t('pg.error_no_body'),
      toolLoop: t('pg.tool_loop_cut'),
    };
    const controllers: Record<Side, AbortController> = { a: new AbortController(), b: new AbortController() };
    splitControllers = [controllers.a, controllers.b];
    const markSequential = (side: Side) => {
      live[side].compareMeta = { ...(live[side].compareMeta as CompareMeta), sequential: true };
    };
    const toolDefs = activeTools.map((tl) => $state.snapshot(tl.definition));
    /** One side's answer: model -> tools -> model rounds, like the single view's agent loop. */
    const runSide = async (side: Side): Promise<StreamResult> => {
      const ep = endpoints[side];
      const msg = live[side];
      msg.contextSize = (ep.serverId === app.selectedServerId ? contextSize : definitionContext(ep.model)) ?? undefined;
      const preset = domain.preset(panes[side].presetId);
      let result: StreamResult = { status: 0 };
      let round = 0;
      sideState[side] = 'running';
      try {
        await runAgentLoop({
          controller: controllers[side],
          streamTurn: async (c) => {
            if (round++ > 0) beginRound(msg);
            const body: Record<string, unknown> = {
              messages: withToolGuidance(applyRag(
                sideHistory(history, side, systemPromptFor(panes[side]), $state.snapshot(msg.priorRounds) as Msg[] | undefined),
                turnRag
              ), toolDefs),
              model: ep.model,
              ...presetBody(preset, toolDefs.length > 0),
            };
            if (toolDefs.length) body.tools = toolDefs;
            result = await streamInto(msg, ep, body, c.signal, texts, () => splitView?.scrollToBottom());
            return msg;
          },
          maxRounds: toolLimits?.max_rounds ?? 0,
          activeToolCount: () => activeTools.length,
          execute: (m, signal) =>
            executeToolCalls(m, signal, {
              tools: activeTools,
              limits: toolLimits!,
              conversationId: convId,
              model: ep.model,
              latestUserMessage: history.findLast((m) => m.role === 'user')?.content,
              workdir: settings.workdir,
              project: projects.activeId,
              broker,
              onProgress: () => splitView?.scrollToBottom(),
              approvalKey: (call) => `${side}:${call.id}`,
            }),
          persist: () => {},
        });
      } finally {
        sideState[side] = 'idle';
      }
      return result;
    };

    const runBoth = (async () => {
      if (plan.sequential) {
        sideState.b = 'queued';
        markSequential('b');
        await runSide('a');
        if (controllers.b.signal.aborted) {
          live.b.stopped = true;
          live.b.streaming = false;
        } else {
          await runSide('b');
        }
        return;
      }
      const results = await Promise.all(SIDES.map((side) => runSide(side)));
      // Sequential fallback: a side the gateway could not load next to the other one runs now
      // that the other one is done (the scheduler may then unload it).
      for (const [i, side] of SIDES.entries()) {
        if (!isCapacityError(results[i]) || controllers[side].signal.aborted) continue;
        const reason = results[i].code === 'max_loaded_models' ? t('pgs.reason_max_loaded') : t('pgs.reason_memory');
        splitNotice = t('pgs.seq_fallback', { side: side.toUpperCase(), reason });
        const m = live[side];
        m.content = '';
        m.reasoning = undefined;
        m.error = undefined;
        m.stats = undefined;
        m.stopped = undefined;
        m.finishReason = undefined;
        m.toolCalls = undefined;
        m.priorRounds = undefined;
        m.streaming = true;
        markSequential(side);
        await runSide(side);
      }
    })();
    currentRun = runBoth;
    try {
      await runBoth;
    } finally {
      if (currentRun === runBoth) currentRun = null;
      splitControllers = [];
      sideState = { a: 'idle', b: 'idle' };
      // Outside the split view the turn shows side A (the active variant).
      const a = live.a;
      turn.content = a.content;
      turn.reasoning = a.reasoning;
      turn.stats = a.stats;
      turn.error = a.error;
      turn.stopped = a.stopped;
      turn.finishReason = a.finishReason;
      turn.contextSize = a.contextSize;
      turn.toolCalls = a.toolCalls;
      turn.toolLimitReached = a.toolLimitReached;
      turn.priorRounds = a.priorRounds;
      turn.rag = a.rag;
      turn.streaming = false;
      streaming = false;
      saveSplitTurn(turn, convId, again);
      splitView?.scrollToBottom();
    }
    if (convId) void afterRun(convId, endpoints.a);
  }

  /**
   * Stores a split turn: one assistant message, side A and side B as its variants (A shown).
   * A regenerated turn (`again`) adds its pair to the stored message as another comparison group.
   */
  function saveSplitTurn(turn: Msg, convId: string | null, again = false) {
    if (!convId || !persistEnabled || !turn.compareSides) return;
    const [a, b] = $state.snapshot(turn.compareSides) as Msg[];
    if (!a.variantId || !b.variantId) return;
    const input = (m: Msg) => {
      const base = toVariantInput(m, m.compareMeta?.model);
      return { ...base, meta: { ...(base.meta ?? {}), compare: { ...m.compareMeta, side: m.compareSide } } };
    };
    const aId = a.variantId;
    const bId = b.variantId;
    savedVariants.add(aId);
    savedVariants.add(bId);
    if (again) {
      queue(() => convApi.addVariant(turn.id, input(a), aId)).catch(dbFail);
    } else {
      queue(() =>
        convApi.append({
          conversation_id: convId,
          parent_id: turn.parentId ?? null,
          role: 'assistant',
          variant: input(a),
          id: turn.id,
          variant_id: aId,
        })
      ).catch(dbFail);
    }
    queue(() => convApi.addVariant(turn.id, input(b), bId)).catch(dbFail);
    queue(() => convApi.selectVariant(turn.id, aId)).catch(dbFail);
  }

  /** The arrows of a regenerated split turn: shows another answer pair. */
  async function switchSplitGroup(index: number, dir: -1 | 1) {
    if (streaming || !conv) return;
    const m = messages[index];
    const target = m.compareNav?.ids[(m.compareNav?.index ?? 0) + dir];
    if (!target) return;
    try {
      applyView(await queue(() => convApi.selectVariant(m.id, target)), false);
    } catch (e) {
      dbFail(e);
    }
  }

  /** Approving or denying a tool call of one side's loop (two loops run at once). */
  function approveSideTool(side: Side, callId: string, always: boolean) {
    const turn = messages.at(-1);
    const call = turn?.compareSides?.find((s) => s.compareSide === side)?.toolCalls?.find((c) => c.id === callId);
    broker.approve(`${side}:${callId}`, always, call?.name);
  }

  function denySideTool(side: Side, callId: string) {
    broker.deny(`${side}:${callId}`);
  }

  /** "Continue with this answer": leaves the split view with one side's history, model and preset. */
  async function adoptSide(side: Side) {
    if (streaming) return;
    const pane = $state.snapshot(compare[side]) as PaneConfig;
    const tg = parseTarget(pane.target);
    compare.enabled = false;
    splitNotice = '';
    if (tg?.kind === 'gateway') {
      settings.model = tg.alias;
    } else if (tg?.kind === 'server') {
      await domain.refreshStates();
      app.selectServer(tg.serverId);
      const s = app.servers.find((x) => x.id === tg.serverId);
      const model = aliasOfServer(tg.serverId) ?? (s?.active ? modelFileName(s.active.model_path) : '');
      if (model) settings.model = model;
    }
    if (pane.presetId) selectPreset(pane.presetId);
    const c = conv;
    if (c && persistEnabled) {
      try {
        applyView(await queue(() => convApi.adoptSide(c.id, side)), false);
      } catch (e) {
        dbFail(e);
      }
    } else {
      messages = messages.map((m) =>
        isSplit(m) ? { ...sideOf(m, side), id: m.id, parentId: m.parentId, compareSides: m.compareSides } : m
      );
    }
    toast.success(t('pgs.adopted', { side: side.toUpperCase() }));
    tick().then(() => {
      scrollToBottom(true);
      composer?.focus();
    });
  }

  // ---- Streaming chat completion + agent loop (see playground/agentLoop.ts) ----

  /**
   * Runs one answer (model -> tools -> model rounds). `variantOf` = regenerate: the first reply
   * is stored as a new variant of that message instead of a new message.
   */
  async function runAgent(convId: string | null, variantOf?: string) {
    const c = new AbortController();
    controller = c;
    streaming = true;
    midTurnBlocked = false;
    turnRag = await prepareRag(convId);
    turnRagShown = false;
    let pendingVariantOf = variantOf;
    let last: Msg | undefined;
    const saveLast = () => {
      if (last) saveMessage(last, convId);
    };
    const run = (async () => {
      try {
        await runAgentLoop({
          controller: c,
          streamTurn: async (ctl) => {
            const of = pendingVariantOf;
            pendingVariantOf = undefined;
            last = await streamTurn(ctl, of);
            saveLast();
            return last;
          },
          maxRounds: toolLimits?.max_rounds ?? 0,
          activeToolCount: () => activeTools.length,
          execute: (msg, signal) =>
            executeToolCalls(msg, signal, {
              tools: activeTools,
              limits: toolLimits!,
              conversationId: convId,
              model: settings.model,
              latestUserMessage: messages.findLast((m) => m.role === 'user')?.content,
              workdir: settings.workdir,
              project: projects.activeId,
              broker,
              onProgress: () => scrollToBottom(),
            }),
          persist: saveLast,
          beforeNextRound: (msg, signal) => compactBetweenRounds(convId, msg, signal),
        });
      } finally {
        if (controller === c) {
          controller = null;
          streaming = false;
        }
        saveLast();
        scrollToBottom();
      }
    })();
    currentRun = run;
    try {
      await run;
    } finally {
      if (currentRun === run) currentRun = null;
    }
    if (convId) void afterRun(convId);
  }

  /** Reloads the stored path (variant/branch counters) and triggers the optional auto title. */
  async function afterRun(convId: string, ep?: Endpoint) {
    if (!persistEnabled) return;
    try {
      const view = await queue(() => convApi.get(convId));
      if (conv?.id === convId && !streaming) applyView(view, false);
    } catch (e) {
      dbFail(e);
    }
    listKey++;
    void autoTitle(convId, ep);
  }

  const titleAttempted = new Set<string>();

  /** Asks for a title after the first answer, retrying templates that reject the system role. */
  async function autoTitle(convId: string, ep?: Endpoint) {
    const c = conv;
    if (!c || c.id !== convId || c.title_source !== 'fallback' || !settings.autoTitle || !canChat) return;
    if (titleAttempted.has(convId)) return;
    const firstUser = messages.find((m) => m.role === 'user');
    const firstAnswer = messages.find((m) => m.role === 'assistant' && m.content.trim());
    if (!firstUser || !firstAnswer) return;
    titleAttempted.add(convId);
    const ctl = new AbortController();
    const timer = setTimeout(() => ctl.abort(), 30000);
    try {
      const uiLanguage = locale();
      const body: Record<string, unknown> = {
        stream: false,
        max_tokens: 32,
        temperature: 0.3,
        // Reasoning models would spend the tiny budget on thinking.
        chat_template_kwargs: { enable_thinking: false },
      };
      const model = ep ? ep.model : settings.model;
      if (model) body.model = model;
      // Managed endpoint while the gateway runs (the alias routes to this server), else direct.
      const { title, source } = await generateTitleWithRetry(firstUser.content, firstAnswer.content, uiLanguage,
        (titleMessages) => fetch(`${ep ? ep.baseUrl : app.inferenceUrl}/chat/completions`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', ...(ep ? ep.headers : app.inferenceHeaders()) },
          body: JSON.stringify({ ...body, messages: titleMessages }),
          signal: ctl.signal,
        }));
      if (!title) return;
      const changed = await queue(() => convApi.setTitle(convId, title, source, 'fallback'));
      if (changed) {
        if (conv?.id === convId) {
          conv.title = title;
          conv.title_source = source;
        }
        listKey++;
      }
    } catch {
      // optional feature; the fallback title stays
    } finally {
      clearTimeout(timer);
    }
  }

  function approveTool(callId: string, always: boolean) {
    const call = messages.at(-1)?.toolCalls?.find((x) => x.id === callId);
    broker.approve(callId, always, call?.name);
  }

  function denyTool(callId: string) {
    broker.deny(callId);
  }

  /** Adds a hint when the server rejects the request because of the tools (no tool-capable chat template). */
  function withToolHint(text: string, sentTools: boolean): string {
    return sentTools && /tool/i.test(text) ? `${text}\n${t('pg.tools_error_hint')}` : text;
  }

  /** One streaming chat request; appends and returns the (reactive) assistant message. */
  async function streamTurn(c: AbortController, variantOf?: string): Promise<Msg> {
    const endpoint = app.inferenceUrl;
    const model = settings.model;
    const apiMessages = applyRag(buildApiMessages(messages, settings), turnRag);
    const parent = messages.at(-1);
    messages.push({
      id: variantOf ?? newId(),
      role: 'assistant',
      content: '',
      streaming: true,
      variantId: newId(),
      parentId: parent?.id ?? null,
      newVariant: !!variantOf,
      contextSize: ragContext ?? undefined,
    });
    const msg = messages[messages.length - 1]; // reactive proxy
    if (turnRag && !turnRagShown) {
      // The sources belong to the first answer of the turn.
      turnRagShown = true;
      msg.rag = { sources: turnRag.sources, retrieval: turnRag.retrieval, reranked: turnRag.reranked, notices: turnRag.notices };
    }
    scrollToBottom(true);

    let toolDefs = wireToolDefs();
    let sentTools = toolDefs.length > 0;
    // Wait for a complete candidate before rendering tool-call JSON as ordinary text.
    const contentBuffer = new TextCallStreamBuffer();
    const body: Record<string, unknown> = {
      messages: withToolGuidance(apiMessages, toolDefs),
      stream: true,
      stream_options: { include_usage: true },
      ...samplingParams($state.snapshot(settings) as PgSettings, { tools: sentTools, reasoningControl }),
    };
    if (model) body.model = model;
    if (sentTools) body.tools = toolDefs;

    const started = performance.now();
    let ttftMs: number | undefined;
    let timings: Timings | null = null;
    let usage: Usage | null = null;
    let finished = false;
    let viaGateway = false;
    const acc = new ToolCallAccumulator();
    // Plain array: pushed per chunk, assigned to the message once at the end.
    const tokenLogprobs: TokenLogprob[] = [];

    const handle = (events: SseEvent[]) => {
      for (const ev of events) {
        if (ev.data === '[DONE]') {
          finished = true;
          continue;
        }
        let chunk: Record<string, unknown>;
        try {
          chunk = JSON.parse(ev.data);
        } catch {
          if (ev.event === 'error') msg.error = withToolHint(t('pg.error_stream', { message: ev.data }), sentTools);
          continue;
        }
        if (ev.event === 'error' || chunk.error) {
          const err = (chunk.error ?? chunk) as { message?: unknown };
          const message = typeof err === 'string' ? err : typeof err.message === 'string' ? err.message : ev.data;
          msg.error = withToolHint(t('pg.error_stream', { message }), sentTools);
          continue;
        }
        const choice = (chunk.choices as { delta?: Record<string, unknown>; finish_reason?: unknown }[] | undefined)?.[0];
        const delta = choice?.delta;
        // Answer tokens only: the thinking part (reasoning_content) is not part of the shown text.
        if (body.logprobs === true && typeof delta?.content === 'string') tokenLogprobs.push(...parseChoiceLogprobs(choice));
        if (delta) {
          const reasoning = typeof delta.reasoning_content === 'string' ? delta.reasoning_content : '';
          const content = typeof delta.content === 'string' ? delta.content : '';
          const hasCalls = Array.isArray(delta.tool_calls) && delta.tool_calls.length > 0;
          if ((reasoning || content || hasCalls) && ttftMs === undefined) ttftMs = performance.now() - started;
          if (reasoning) msg.reasoning = (msg.reasoning ?? '') + reasoning;
          if (content) {
            if (sentTools) msg.content += contentBuffer.push(content);
            else msg.content += content;
          }
          observeThinking(msg, performance.now());
          if (hasCalls) {
            acc.push(delta.tool_calls);
            const checked = guardToolCalls(acc.result().map((tc) => ({
              id: tc.id,
              name: tc.function.name,
              arguments: tc.function.arguments,
              status: 'pending' as const,
            })), toolDefs.length);
            if (checked.stopped) {
              msg.toolCalls = checked.calls;
              msg.error = t('pg.tool_loop_cut');
              msg.finishReason = 'tool_calls';
              finished = true;
              c.abort();
              break;
            }
          }
        }
        if (typeof choice?.finish_reason === 'string') {
          msg.finishReason = choice.finish_reason;
          const checked = guardToolCalls(acc.result().map((tc) => ({
            id: tc.id, name: tc.function.name, arguments: tc.function.arguments, status: 'pending' as const,
          })), toolDefs.length, true);
          if (checked.calls.length) msg.toolCalls = checked.calls;
          if (checked.stopped) {
            msg.error = t('pg.tool_loop_cut');
            msg.finishReason = 'tool_calls';
            finished = true;
            c.abort();
            break;
          }
        }
        if (chunk.timings && typeof chunk.timings === 'object') timings = chunk.timings as Timings;
        if (chunk.usage && typeof chunk.usage === 'object') usage = chunk.usage as Usage;
      }
      if (events.length) scrollToBottom();
    };

    try {
      const request = await chatRequest(endpoint, body,
        { 'Content-Type': 'application/json', Accept: 'text/event-stream', ...app.inferenceHeaders() }, c.signal);
      if (request.routingFallback) notifyRoutingFallback(request.routingError);
      const res = request.response;
      toolDefs = (request.body.tools ?? []) as typeof toolDefs;
      sentTools = toolDefs.length > 0;
      viaGateway = res.headers.has('x-harness-request-id');
      if (!res.ok) {
        const text = await res.text().catch(() => '');
        msg.error = withToolHint(
          t('pg.error_http', { status: res.status, message: describeError(text) || res.statusText }),
          sentTools
        );
        return msg;
      }
      if (!res.body) {
        msg.error = t('pg.error_no_body');
        return msg;
      }
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      const parser = new SseParser();
      while (!finished) {
        const { value, done } = await reader.read();
        if (done) {
          handle(parser.push(decoder.decode()));
          handle(parser.flush());
          break;
        }
        handle(parser.push(decoder.decode(value, { stream: true })));
      }
      if (finished) reader.cancel().catch(() => {});
      await salvageToolCalls(msg, sentTools ? toolDefs.map((tool) => tool.function.name) : [], c.signal, parseTextToolCalls, msg.content + contentBuffer.drain());
      if (msg.toolCalls && !msg.error) {
        const checked = guardToolCalls(msg.toolCalls, toolDefs.length, true);
        msg.toolCalls = checked.calls;
        if (checked.stopped) { msg.error = t('pg.tool_loop_cut'); msg.finishReason = 'tool_calls'; c.abort(); }
      }
    } catch (e) {
      if (c.signal.aborted) msg.stopped = true;
      else msg.error = requestErrorText(e, `${endpoint}/chat/completions`, (message) => t('pg.error_network', { message }));
    } finally {
      msg.content += contentBuffer.drain();
      if (tokenLogprobs.length) {
        msg.logprobs = tokenLogprobs;
        if (msg.variantId) rememberLogprobs(msg.variantId, tokenLogprobs);
      }
      if (msg.content || msg.reasoning || msg.toolCalls?.length || !msg.error) {
        msg.stats = buildStats(timings, usage, ttftMs, performance.now() - started);
      }
      const finalUsage = usage as Usage | null;
      const finalTimings = timings as Timings | null;
      if (!viaGateway && finalUsage?.prompt_tokens !== undefined && finalUsage?.completion_tokens !== undefined) {
        void invoke('record_direct_usage', { record: {
          endpoint, client: 'harness-playground', model,
          prompt_tokens: finalUsage.prompt_tokens, completion_tokens: finalUsage.completion_tokens,
          cached_tokens: finalUsage.prompt_tokens_details?.cached_tokens ?? finalTimings?.cache_n ?? 0,
        } }).catch(() => {});
      }
      if (msg.stats?.source === 'none' && !msg.error) {
        msg.stats = { ...msg.stats, source: 'estimate', promptN: estimateTokens(JSON.stringify({ ...body, messages: withoutImages(body.messages as Record<string, unknown>[]) })) + countImageParts(body.messages as Record<string, unknown>[]) * IMAGE_TOKEN_ESTIMATE, predictedN: estimateTokens(msg.content + (msg.reasoning ?? '')) };
      }
      msg.streaming = false;
      scrollToBottom();
    }
    return msg;
  }
</script>

<svelte:window onkeydown={onGlobalKeydown} />

<div class="flex h-full min-h-0 w-full overflow-hidden bg-background" bind:this={rootEl}>
  <ResizablePane pane={panes.pgConversations} side="left" label={t('pane.conversations')} class="bg-background">
      <ConversationSidebar
        activeId={conv?.id ?? null}
        refreshKey={listKey}
        readOnly={!persistEnabled}
        onOpen={(id) => {
          if (panes.pgConversations.floating) panes.pgConversations.close();
          return openConversation(id);
        }}
        onNew={() => newChat()}
        onDeleted={onConversationDeleted}
        onRenamed={onConversationRenamed}
      />
  </ResizablePane>

  <section class="@container/pg relative flex min-w-0 flex-1 flex-col" aria-labelledby="pg-title">
    {#if dropActive}
      <div
        class="pointer-events-none absolute inset-2 z-30 flex flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed border-border bg-background/90 text-center"
        role="status"
      >
        <FileUp class="size-6 text-muted-foreground" aria-hidden="true" />
        <p class="m-0 text-sm font-medium text-foreground">{t('doc.drop_title')}</p>
        <p class="m-0 max-w-sm text-xs text-muted-foreground">{t('doc.drop_desc')}</p>
      </div>
    {/if}
    <div class="flex h-11 shrink-0 items-center justify-between gap-2 border-b border-border px-3">
      <!-- Narrow centre (both side panels open): the title and the compare label yield their
           place and the model picker truncates; the mode tabs always stay. The group clips
           rather than slide under the picker (macOS fonts are wider than the thresholds assume). -->
      <div class="flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden" use:fitTabs>
        <Tooltip.Root>
          <Tooltip.Trigger>
            {#snippet child({ props })}
              <Button
                {...props}
                variant={showSidebar ? 'secondary' : 'ghost'}
                size="icon-sm"
                aria-expanded={showSidebar}
                aria-controls={panes.pgConversations.elementId}
                aria-label={showSidebar ? t('pg.hide_conversations') : t('pg.show_conversations')}
                onclick={toggleSidebar}
              >
                <PanelLeft />
              </Button>
            {/snippet}
          </Tooltip.Trigger>
          <Tooltip.Content>{showSidebar ? t('pg.hide_conversations') : t('pg.show_conversations')}</Tooltip.Content>
        </Tooltip.Root>
        <h2 id="pg-title" class={cn('shrink-0 text-base font-semibold text-foreground @max-[600px]/pg:sr-only', tabsCompact >= 2 && 'sr-only')}>{t('pg.title')}</h2>
        <ToggleGroup.Root
          type="single"
          variant="outline"
          size="sm"
          value={pgTab}
          onValueChange={(v) => {
            if (v) pgTab = v as ServeMode;
          }}
          class="ml-1 shrink-0"
          aria-label={t('emb.tabs_label')}
        >
          <!-- Narrow centre: the tabs turn into icons (label kept for screen readers and as a
               tooltip) so the model picker stays readable next to them. -->
          {#each PLAYGROUND_MODES.filter(mode => isVisible(mode.level, uiLevel.level) || mode.id === pgTab) as tab (tab.id)}
            <ToggleGroup.Item value={tab.id} class="h-7 px-2 text-xs" title={t(tab.label)}>
              <tab.icon class={cn('size-3.5', tabsCompact === 0 && 'hidden')} aria-hidden="true" />
              <span class={cn(tabsCompact > 0 && 'sr-only')}>{t(tab.label)}</span>
            </ToggleGroup.Item>
          {/each}
        </ToggleGroup.Root>
        {#if conv && pgTab === 'chat'}
          <span class="min-w-0 truncate text-xs text-muted-foreground @max-[820px]/pg:hidden" title={conv.title}>
            <span aria-hidden="true">·</span>
            {conv.title.trim() || t('pg.conv_untitled')}
          </span>
          {#if uiLevel.advanced && conversationTotalTokens > 0}
            <span data-conversation-tokens class="min-w-0 truncate text-xs text-muted-foreground font-num @max-[820px]/pg:hidden" title={t('pg.conversation_total_title')}>
              <span aria-hidden="true">·</span>
              {#if conversationUsage.estimated}≈{/if}{t('pg.conversation_total', { n: formatTokens(conversationTotalTokens), input: formatTokens(conversationUsage.input), output: formatTokens(conversationUsage.output) })}
            </span>
          {/if}
        {/if}
      </div>
      <div class="flex shrink-0 items-center gap-1.5">
        {#if pgTab === 'chat' && (uiLevel.advanced || compare.enabled)}
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant={compare.enabled ? 'secondary' : 'ghost'}
                  size="sm"
                  aria-pressed={compare.enabled}
                  aria-keyshortcuts="Control+Shift+M"
                  disabled={streaming}
                  onclick={toggleCompare}
                >
                  <Columns2 />
                  <span class="max-[1100px]:sr-only @max-[720px]/pg:sr-only">{t('pgs.toggle')}</span>
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-72">
              <p class="font-medium">{compare.enabled ? t('pgs.toggle_off') : t('pgs.toggle_on')}</p>
              <p class="mt-0.5">{t('pgs.toggle_desc')}</p>
            </Tooltip.Content>
          </Tooltip.Root>
        {/if}
        {#if !splitOn}
        <ModelPicker
          currentModel={pickerModel}
          mode={pgTab}
          label={pgTab === serveMode && pgTab !== 'chat' && pgTab !== 'image' ? pickerLabel : undefined}
          onUse={useModel}
          onPicked={() => tick().then(() => composer?.focus(true))}
        />
        {/if}
        <!-- The chat's context use; the Embedding / Rerank tabs don't send the conversation. -->
        {#if pgTab === 'chat' && !splitOn && contextSize && contextUsed !== null}
          {@const high = contextRatio >= CONTEXT_WARN}
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <!-- Focusable so keyboard users can reach the explanation tooltip. -->
                <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
                <div
                  {...props}
                  role="meter"
                  tabindex="0"
                  aria-valuemin={0}
                  aria-valuemax={contextSize}
                  aria-valuenow={Math.min(contextUsed, contextSize)}
                  aria-label={t('pg.ctx_label')}
                  class="flex shrink-0 items-center gap-2 rounded-md px-1.5 py-1 outline-none focus-visible:ring-2 focus-visible:ring-ring/50 @max-[580px]/pg:hidden"
                >
                  <div class="relative h-1.5 w-14 overflow-hidden rounded-full bg-muted" aria-hidden="true">
                    <div
                      class={cn('h-full rounded-full transition-[width]', high ? 'bg-warning' : 'bg-muted-foreground')}
                      style:width={`${Math.min(100, contextRatio * 100)}%`}
                    ></div>
                    {#if cmpLimit}
                      <!-- Auto-compaction threshold. -->
                      <div class="absolute inset-y-0 w-px bg-foreground/60" style:left={`${Math.min(100, (cmpLimit / contextSize) * 100)}%`}></div>
                    {/if}
                  </div>
                  <AdvancedOnly><span class={cn('font-num text-xs whitespace-nowrap @max-[720px]/pg:sr-only', high ? 'text-warning' : 'text-muted-foreground')}>
                    {t('pg.ctx_value', { used: fmtInt(contextUsed), total: fmtInt(contextSize) })}
                  </span></AdvancedOnly>
                  {#if high}<TriangleAlert class="size-3.5 text-warning" aria-hidden="true" />{/if}
                </div>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-80">
              <p class="font-medium">{uiLevel.advanced ? t('pg.ctx_title', { pct: Math.round(contextRatio * 100) }) : t('pg.ctx_label')}</p>
              <p class="mt-1">{t('pg.ctx_tooltip')}</p>
              <AdvancedOnly><p class="mt-1">
                {cmpLimit ? t('cmp.meter_auto', { limit: fmtInt(cmpLimit) }) : t('cmp.meter_off')}
              </p></AdvancedOnly>
              {#if activeCmp}<p class="mt-1">{t('cmp.meter_compacted', { n: activeCmp.meta.covered })}</p>{/if}
              {#if high}<p class="mt-1">{cmpLimit ? t('cmp.meter_high_auto') : t('pg.ctx_tooltip_high')}</p>{/if}
            </Tooltip.Content>
          </Tooltip.Root>
        {/if}
        {#if !showSidebar}
          <Button variant="ghost" size="sm" onclick={() => newChat()} disabled={messages.length === 0 && !streaming && !conv}>
            <Plus />
            {t('pg.new_chat')}
          </Button>
        {/if}
        {#if !splitOn}
        <Tooltip.Root>
          <Tooltip.Trigger>
            {#snippet child({ props })}
              <Button
                {...props}
                variant={showSettings ? 'secondary' : 'ghost'}
                size="icon-sm"
                aria-pressed={showSettings}
                aria-controls={panes.pgSettings.elementId}
                aria-label={showSettings ? t('pg.hide_settings') : t('pg.show_settings')}
                onclick={() => panes.pgSettings.toggle()}
              >
                <SlidersHorizontal />
              </Button>
            {/snippet}
          </Tooltip.Trigger>
          <Tooltip.Content>{showSettings ? t('pg.hide_settings') : t('pg.show_settings')}</Tooltip.Content>
        </Tooltip.Root>
        {/if}
      </div>
    </div>

    {#if dbError || dbReadOnly}
      <div class="flex shrink-0 items-start gap-2 border-b border-warning/30 bg-warning/10 px-4 py-2" role="status">
        <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
        <span class="min-w-0 flex-1 text-xs text-foreground [overflow-wrap:anywhere]">
          {dbError ? t('pg.db_unavailable', { message: dbError }) : t('pg.db_read_only')}
        </span>
      </div>
    {/if}

    {#if pgTab !== 'chat'}
      {#if pgTab === 'embedding' || pgTab === 'rerank'}
        <div class="flex flex-wrap items-center gap-2 px-4 py-2 text-xs text-muted-foreground" role="status">
          <span>{t('pg.model_definition_hint')}</span>
          <Button variant="link" size="sm" onclick={() => app.openTab('library')}>{t('pg.model_definition_library')}</Button>
        </div>
      {/if}
      <!-- Speech goes through the gateway (whisper-server JIT), not the selected llama-server:
           the Speech panel shows its own gateway notice instead. -->
      {#if !canChat && pgTab !== 'transcription' && pgTab !== 'image' && pgTab !== 'pipeline'}
        <div class="flex shrink-0 items-center gap-3 border-b border-warning/30 bg-warning/10 px-4 py-2" role="status">
          <Unplug class="size-4 shrink-0 text-warning" aria-hidden="true" />
          <span class="min-w-0 flex-1 text-xs text-foreground">
            {app.isBusy ? t('pg.offline_starting') : t('pg.offline_title')}
          </span>
          <Button variant="outline" size="sm" onclick={onGoToDashboard}>{t('pg.go_dashboard')}</Button>
        </div>
      {/if}
      <div class="min-h-0 flex-1 overflow-y-auto">
        <AdvancedPageNotice level={pgTab === 'embedding' || pgTab === 'rerank' || pgTab === 'pipeline' ? 'advanced' : undefined} />
        {#if pgTab === 'embedding'}
          <EmbeddingPanel model={modeModels.embedding} />
        {:else if pgTab === 'transcription'}
          <TranscribePanel />
          <hr class="mx-auto w-full max-w-[860px] border-border" />
          <TtsPanel />
          <AdvancedOnly>
            <hr class="mx-auto w-full max-w-[860px] border-border" />
            <AudioProcessPanel />
          </AdvancedOnly>
        {:else if pgTab === 'image'}
          <ImagePanel />
        {:else if pgTab === 'pipeline'}
          <PipelinePanel model={modeModels.pipeline} task={pipelineTask} />
        {:else}
          <RerankPanel model={modeModels.rerank} />
        {/if}
      </div>
    {:else if !canChat && messages.length === 0}
      <Empty.Root class="border-0" role="status">
        <Empty.Header>
          <Empty.Media variant="icon"><Unplug /></Empty.Media>
          <Empty.Title>{t('pg.offline_title')}</Empty.Title>
          <Empty.Description>{app.isBusy || starting ? t('pg.offline_starting') : t('pg.offline_desc')}</Empty.Description>
        </Empty.Header>
        <Empty.Content>
          {#if startModel && !app.isBusy && !starting}
            <Button class="max-w-full" onclick={startFromHere}><span class="truncate">{startLabel}</span></Button>
          {/if}
          <Button variant={startModel && !app.isBusy && !starting ? 'outline' : 'default'} onclick={onGoToDashboard}>{t('pg.go_dashboard')}</Button>
        </Empty.Content>
      </Empty.Root>
    {:else}
      {#if !canChat}
        <div class="flex shrink-0 items-center gap-3 border-b border-warning/30 bg-warning/10 px-4 py-2" role="status">
          <Unplug class="size-4 shrink-0 text-warning" aria-hidden="true" />
          <span class="min-w-0 flex-1 text-xs text-foreground">
            {app.isBusy ? t('pg.offline_starting') : t('pg.offline_title')}
          </span>
          <Button variant="outline" size="sm" onclick={onGoToDashboard}>{t('pg.go_dashboard')}</Button>
        </div>
      {/if}
      {#if serveMode !== 'chat' && !app.gatewayRunning}
        <div class="flex shrink-0 items-center gap-3 border-b border-warning/30 bg-warning/10 px-4 py-2" role="status">
          <TriangleAlert class="size-4 shrink-0 text-warning" aria-hidden="true" />
          <span class="min-w-0 flex-1 text-xs text-foreground">
            {serveMode === 'embedding' ? t('emb.server_mode_banner_embedding') : serveMode === 'transcription' ? t('asr.server_mode_banner') : serveMode === 'image' ? t('img.server_mode_banner') : t('emb.server_mode_banner_rerank')}
          </span>
          <Button variant="outline" size="sm" onclick={() => (pgTab = serveMode)}>
            {serveMode === 'embedding' ? t('emb.open_embedding_tab') : serveMode === 'transcription' ? t('asr.open_tab') : serveMode === 'image' ? t('img.open_tab') : t('emb.open_rerank_tab')}
          </Button>
        </div>
      {/if}

      {#if splitOn}
        <SplitView
          bind:this={splitView}
          bind:compare
          {messages}
          gatewayModels={app.gatewayRunning ? chatModels : []}
          {modelStates}
          busy={streaming || compacting}
          {canChat}
          {sideState}
          notice={splitNotice}
          toolsEnabled={settings.toolsEnabled && activeTools.length > 0}
          onAdopt={adoptSide}
          onRewind={requestRewind}
          onRegenerate={regenerateSplit}
          onVariant={switchSplitGroup}
          onSibling={switchSibling}
          onApproveTool={approveSideTool}
          onDenyTool={denySideTool}
          onFork={conv && persistEnabled ? forkFrom : undefined}
        />
      {:else}
      <div
        class="min-h-0 flex-1 overflow-y-auto"
        bind:this={listEl}
        onscroll={onListScroll}
        role="log"
        aria-live={olderBusy ? 'off' : 'polite'}
        aria-busy={olderBusy}
        aria-label={t('pg.messages_label')}
      >
        <div class="mx-auto flex min-h-full w-full max-w-[760px] flex-col gap-6 px-4 py-6">
          {#if !openingConv && shownStart > 0}
            <Button bind:ref={olderButton} variant="ghost" size="sm" class="self-center text-muted-foreground" onclick={() => mountOlder()}>
              {t('pg.load_older')}
            </Button>
          {/if}
          {#if openingConv}
            <SectionLoading
              shape="rows"
              rows={4}
              class="my-auto"
              label={t('sl.msgs_label')}
              phases={[t('sl.msgs_1'), t('sl.msgs_2')]}
            />
          {:else if messages.length === 0}
            <div class="my-auto flex flex-col items-center gap-5 py-6 text-center">
              <div class="flex items-center justify-center text-muted-foreground" aria-hidden="true">
                <MessageSquare class="size-5" />
              </div>
              <div class="flex flex-col gap-1">
                <h3 class="text-base font-semibold text-foreground">{t('pg.empty_title')}</h3>
                <p class="text-xs text-muted-foreground">{t('pg.empty_desc')}</p>
                {#if library.models.length === 0 && !pickerModel}
                  <h4 class="mt-3 text-sm font-medium">{t('setup.first')}</h4>
                  <p class="max-w-md text-xs text-muted-foreground">{t('setup.first_hint')}</p>
                  <Button size="sm" variant="outline" class="mt-2 self-center" onclick={() => app.requestedTab = 'hub'}>{t('setup.discover')}</Button>
                {/if}
              </div>
              <div class="grid w-full max-w-xl grid-cols-[repeat(2,minmax(0,1fr))] gap-2 text-left">
                {#each EXAMPLES as key (key)}
                  <button
                    type="button"
                    class="cursor-pointer rounded-lg border border-border bg-card px-3 py-2.5 text-xs leading-snug text-foreground outline-none transition-colors hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
                    onclick={() => useExample(t(key))}
                  >
                    {t(key)}
                  </button>
                {/each}
              </div>
            </div>
          {/if}
          {#each openingConv ? [] : shownMessages as message, k (message.id)}
            {@const i = shownStart + k}
            {#if message.role === 'compaction'}
              <CompactionDivider
                {message}
                active={activeCmp?.markerIndex === i}
                canToggle={!streaming && !compacting && persistEnabled}
                onUndo={() => setCompactionUndone(message, true)}
                onRedo={() => setCompactionUndone(message, false)}
              />
            {:else}
              <!-- Summarized messages stay readable but dimmed: the model now sees their summary. -->
              <div
                class={cn(activeCmp && i <= activeCmp.boundaryIndex && 'opacity-60 transition-opacity focus-within:opacity-100 hover:opacity-100')}
              >
                <ChatMessage
                  {message}
                  contextSize={ragContext}
                  busy={streaming || compacting}
                  {canChat}
                  isTurnEnd={isTurnEnd(i)}
                  isLast={i === messages.length - 1}
                  variantNav={variantNavFor(i)}
                  onRegenerate={() => regenerate(i)}
                  onVariant={(dir) => switchVariant(i, dir)}
                  onSibling={(dir) => switchSibling(i, dir)}
                  onRewind={() => requestRewind(i)}
                  onEditSend={(text) => editAndResend(i, text)}
                  onFork={conv && persistEnabled ? () => forkFrom(i) : undefined}
                  onApproveTool={approveTool}
                  onDenyTool={denyTool}
                />
              </div>
            {/if}
          {/each}
        </div>
      </div>
      {/if}
    {/if}

    <div class="shrink-0 px-4 pt-2 pb-4" class:hidden={pgTab !== 'chat'}>
      <div class={cn('mx-auto w-full', splitOn ? 'max-w-[1200px]' : 'max-w-[760px]')}>
        <DocumentsBar
          docs={ragDocs}
          bind:settings
          models={ragModels}
          contextTokens={ragContext}
          readOnly={!persistEnabled}
          onAttach={() => attachFiles()}
          onRemove={removeDoc}
          onCancel={(id) => void ragApi.cancel(id)}
          onRetry={(id) => reindexDocs([id])}
          onMode={setDocMode}
          onEmbedChange={() => reindexDocs()}
          onGoToHub={() => onGoToHub?.()}
        />
        {#if contextPressure}
          <p data-context-pressure role="status" class="mb-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
            {t('pg.context_pressure', { tools: formatTokenNumber(promptBudget.toolTokens, locale()), context: formatTokenNumber(ragContext, locale()) })}
          </p>
        {/if}
        <ChatInput
          bind:this={composer}
          bind:value={input}
          streaming={streaming || compacting}
          disabled={!canChat}
          sendBlocked={composerWaiting}
          toolsEnabled={settings.toolsEnabled}
          toolsBusy={!settingsLoaded || toolsState === 'loading'}
          toolsHint={composerToolsHint}
          {reasoningControl}
          bind:thinking={settings.thinking}
          bind:thinkingBudget={settings.thinkingBudget}
          presetThinking={settings.request.reasoning.enabled}
          onToggleTools={() => { settings.toolsEnabled = !settings.toolsEnabled; settings.toolsExplicit = true; }}
          contextKey={workspaceVersion}
          {awaitingApproval}
          onSend={send}
          onStop={stop}
          onAttach={!persistEnabled ? undefined : () => attachFiles()}
          audio={pendingAudio}
          onAudioFile={audioEnabled ? (file) => void addAudioFiles([file]) : undefined}
          onPickAudio={pickAudio}
          onRemoveAudio={(i) => pendingAudio = pendingAudio.filter((_, j) => j !== i)}
          audioContextKey={`${workspaceVersion}:${settings.model}:${audioEnabled}`}
          images={pendingImages}
          onImageFiles={addImageFiles}
          onPickImages={pickImages}
          onRemoveImage={(i) => (pendingImages = pendingImages.filter((_, j) => j !== i))}
          {imagesBlocked}
          {imagesBlockedHint}
          status={compacting ? t('cmp.compacting') : ragSearching ? t('doc.searching') : undefined}
        >
          {#snippet actions()}
            {#if persistEnabled}
              <McpResourcesButton disabled={streaming || compacting} onAttachFile={async (path) => attachFiles([path])} />
            {/if}
            <WorkspacePicker conversationKey={workspaceVersion} bind:value={settings.workdir} disabled={streaming || compacting} />
            {#if uiLevel.advanced && !splitOn && persistEnabled}
              <CompactButton
                disabled={!conv || !canChat || streaming || compacting || chatCount < 2}
                keepTurns={cmpSettings.keepTurns}
                usage={contextSize && contextUsed !== null
                  ? t('cmp.usage', { used: fmtInt(contextUsed), total: fmtInt(contextSize), pct: Math.round(contextRatio * 100) })
                  : undefined}
                onCompact={(instruction) => void runCompaction('manual', instruction)}
              />
            {/if}
            {#if uiLevel.advanced && pgTab === 'chat' && !splitOn}
              {@const reserved = settings.request?.sampling?.max_tokens ?? 400}
              {@const high = contextSize !== null && totalEstimate + reserved > contextSize}
              <span data-composer-tokens={totalEstimate} data-exact={draftTokensExact} title={high ? t('pg.ctx_overflow') : undefined} class="flex items-center gap-1 text-xs {high ? 'text-warning' : 'text-muted-foreground'} ml-2 font-num">
                <span class="whitespace-nowrap">{draftTokensExact ? '' : '≈'}{contextSize
                  ? t('pg.ctx_value', { used: formatTokenNumber(totalEstimate, locale()), total: formatTokenNumber(contextSize, locale()) })
                  : t('pg.composer_tokens', { n: formatTokenNumber(totalEstimate, locale()) })}</span>
                <InfoTip text={t('pg.composer_count_tooltip') + (contextSize ? ' ' + t('pg.composer_count_limit', { max: formatTokenNumber(contextSize, locale()) }) : '')} label={t('pg.composer_count')} />
              </span>
            {/if}
          {/snippet}
        </ChatInput>
      </div>
    </div>
  </section>

  <Dialog.Root open={!!rewindAsk} onOpenChange={(open) => !open && (rewindAsk = null)}>
    <Dialog.Content class="sm:max-w-md">
      <Dialog.Header>
        <Dialog.Title>{t('pg.rewind_title')}</Dialog.Title>
        <Dialog.Description>{t('pg.rewind_desc')}</Dialog.Description>
      </Dialog.Header>
      <p class="m-0 text-xs text-muted-foreground">
        {t('pg.rewind_affected')} <span class="font-num text-foreground">{rewindAsk?.later ?? 0}</span>. {t('pg.rewind_note')}
      </p>
      <Dialog.Footer class="mt-2 gap-2">
        <Button variant="outline" onclick={() => (rewindAsk = null)}>{t('pg.cancel')}</Button>
        <Button variant="destructive" onclick={() => rewindAsk && rewindTo(rewindAsk.index)}>{t('pg.rewind_confirm')}</Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>

  <ToolGeneratorDialog
    bind:open={toolGenOpen}
    endpoint={toolGenEndpoint}
    modelName={settings.model}
    modelPath={toolGenModelPath}
    onChanged={refreshTools}
  />

  {#if pgTab === 'chat' && !splitOn}
    <ResizablePane pane={panes.pgSettings} side="right" label={t('pane.pg_settings')} class="bg-background">
      <SettingsPanel
        {ragModels}
        onEmbedChange={() => reindexDocs()}
        onGoToHub={() => onGoToHub?.()}
        bind:settings
        {models}
        {modelStates}
        {modelsLoading}
        {modelsError}
        disabled={!canChat}
        onRefreshModels={refreshModels}
        onReset={resetSettings}
        tools={allTools}
        {toolsState}
        {toolsError}
        onRefreshTools={refreshTools}
        onCreateTool={() => (toolGenOpen = true)}
        onApplyPreset={selectPreset}
        onSaveAsPreset={saveAsPreset}
        contextSize={ragContext}
      />
    </ResizablePane>
  {/if}
</div>
