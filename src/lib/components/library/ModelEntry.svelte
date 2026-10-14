<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { type AgentModel, TOOL_MIN_ROUNDS } from '../connections/agentModels';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // One local model file (or split set) in My Models: facts, fit, state, its definitions with
  // load / unload / reload, and file actions (favorite, reveal, copy path, move, delete, repair).
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { ACTION_TOAST_DURATION } from '#lib/toast.js';
  import type { HardwareFitResult, LibraryEntry, LoadTarget, ModelCapability, ModelDefinition, ModelKind } from '#lib/types.js';
  import { presetName as presetDisplayName } from '#lib/components/presets/presetText.js';
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { onnxUnsupportedText } from '#lib/engines.js';
  import { savedPortText } from '../servers/utils';
  import { cn } from '#lib/utils.js';
  import { formatBytes } from '#lib/format.js';
  import { localizeError } from '#lib/network.js';
  import { hardwareFit } from '#lib/fit.js';
  import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
  import { domain, baseName, samePath } from '#lib/stores/domain.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { loadAndChat, openInPlayground } from '#lib/stores/downloads.svelte.js';
  import { playgroundTarget, speechKindKey } from '#lib/libraryDefinitions.js';
  import { isChatLoadable, showUpdateBadge } from '#lib/downloadFollowUp.js';
  import UpdateBadge from '#lib/components/hub/UpdateBadge.svelte';
  import { Button, buttonVariants } from '#lib/components/ui/button/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import PublisherAvatar from '#lib/components/hub/PublisherAvatar.svelte';
  import {
    Star,
    Ellipsis,
    FolderOpen,
    Copy,
    FolderInput,
    Trash2,
    Unlink,
    Play,
    Square,
    RotateCw,
    Pencil,
    Plus,
    ChevronDown,
    LoaderCircle,
    Wrench,
    LayoutDashboard,
    MessageSquare,
    SquareArrowOutUpRight,
    ChevronRight,
  } from '@lucide/svelte';
  import { formatParams, relativeTime } from './filters';
  import { isSherpaAsrFile } from '../playground/asr';
  import EngineNeeded from '../common/EngineNeeded.svelte';

  interface Props {
    entry: LibraryEntry;
    /** All entries (names of paired projectors / draft targets). */
    entries: LibraryEntry[];
    agentModels: AgentModel[];
    definitions: ModelDefinition[];
    layout: 'grid' | 'list';
    onEditDefinition: (def: ModelDefinition) => void;
    onNewDefinition: (entry: LibraryEntry) => void;
    onMove: (entry: LibraryEntry) => void;
    onDelete: (entry: LibraryEntry) => void;
    onToggleFavorite: (entry: LibraryEntry) => void;
    onChanged: () => void;
  }

  let { entry, entries, agentModels, definitions, layout, onEditDefinition, onNewDefinition, onMove, onDelete, onToggleFavorite, onChanged }: Props = $props();

  /** Kinds the gateway loads on the first request (speech, image, task pipelines): no Load button, no llama-server settings. */
  const GATEWAY_LOADED = ['asr', 'image', 'pipeline'];

  const KIND: Record<ModelKind, TKey> = {
    llm: 'lib.kind_llm',
    embedding: 'lib.kind_embedding',
    reranker: 'lib.kind_reranker',
    asr: 'lib.kind_asr',
    image: 'lib.kind_image',
    tts: 'lib.kind_tts',
    pipeline: 'lib.kind_pipeline',
    mmproj: 'lib.kind_mmproj',
    adapter: 'lib.kind_adapter',
    unknown: 'lib.kind_unknown',
  };

  let busy = $state<Record<string, boolean>>({});
  let m = $derived(entry.model);
  /** A sherpa-onnx recognition folder (Parakeet, SenseVoice...): run per request by the gateway, no definition or server. */
  let sherpaAsr = $derived(isSherpaAsrFile(m));
  // Match the file, not its name: different GGUF quantizations can have different evidence.
  let matchingAgents = $derived(agentModels.filter((a) => a.model_path && samePath(a.model_path, m.path)));
  let toolBadges = $derived(m.kind !== 'llm' ? [] : matchingAgents.length ? matchingAgents : [null]);
  let linked = $derived(m.origin === 'linked' || !!m.source);
  let params = $derived(formatParams(entry.params));
  let nameOf = (id: string) => entries.find((e) => e.model.id === id)?.model.name ?? baseName(id);
  /** Drafts suggested for this LLM (entries that list it as a target). */
  let drafts = $derived(entries.filter((e) => e.draft_for.includes(m.id)));

  /** Servers that run this file right now. */
  let running = $derived(
    app.servers.filter(
      (s) =>
        (s.status === 'Running' || s.status === 'Starting') &&
        !!s.active &&
        !s.active.router_mode &&
        [m.path, ...(m.shard_paths ?? [])].some((p) => samePath(p, s.active!.model_path))
    )
  );

  // Rust estimate (batched with the other rows), corrected with measured loads of this file.
  let fit = $state<HardwareFitResult | null>(null);
  /** Memory figure label: Apple Silicon has no separate VRAM. */
  function fitMemKey(f: HardwareFitResult): TKey {
    const apple = f.unifiedMemory && !f.sharedIgpu;
    return f.source === 'estimate' ? (apple ? 'lib.mem_est' : 'lib.vram_est') : apple ? 'lib.mem_measured' : 'lib.vram_measured';
  }
  $effect(() => {
    const eligible = m.is_ready && !sherpaAsr && ['llm', 'embedding', 'reranker', 'asr', 'image'].includes(m.kind ?? '');
    const ctx = Math.min(m.context_length || 32768, app.settings?.server.default_ctx || 32768);
    const path = m.path;
    const size = m.size_bytes;
    void app.hardware;
    if (!eligible) {
      fit = null;
      return;
    }
    let live = true;
    hardwareFit({ model_path: path, model_size_bytes: size, context_size: ctx, gpu_layers: 999 })
      .then((r) => {
        if (live) fit = r;
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  });
  const FIT_DOT: Record<string, string> = {
    full_gpu: 'bg-success',
    tight: 'bg-warning',
    hybrid: 'bg-warning',
    cpu_heavy: 'bg-muted-foreground',
    oom_risk: 'bg-destructive',
  };
  const FIT_TONE: Record<string, string> = {
    full_gpu: 'text-success',
    tight: 'text-warning',
    hybrid: 'text-warning',
    cpu_heavy: 'text-muted-foreground',
    oom_risk: 'text-destructive',
  };

  // Capabilities come from the registry record (one source for the library and the Hub); tools and
  // reasoning are template facts the record does not carry.
  const CAP_LABEL: Partial<Record<ModelCapability, TKey>> = {
    vision: 'lib.cap_vision',
    audio: 'lib.cap_audio',
    embedding: 'lib.cap_embedding',
    rerank: 'lib.cap_rerank',
    asr: 'lib.cap_asr',
    tts: 'lib.cap_tts',
  };
  let caps = $derived(
    [
      entry.caps.tools && t('lib.cap_tools'),
      entry.caps.reasoning && t(entry.caps.verified?.reasoning ? 'lib.cap_reasoning_verified' : 'lib.cap_reasoning'),
      ...entry.record.capabilities.map((c) => (CAP_LABEL[c] ? t(CAP_LABEL[c]) : false)),
    ].filter(Boolean) as string[]
  );

  const FORMAT_LABEL: Record<string, string> = { gguf: 'GGUF', whisper_ggml: 'whisper ggml' };
  let formatLabel = $derived(FORMAT_LABEL[entry.record.format] ?? entry.record.format);
  /** The runtime family that runs the file; says so when it is missing. Null for companion files. */
  let runtimeText = $derived.by(() => {
    const c = entry.record.compatibility;
    if (!c) return null;
    if (c.status === 'unsupported') return t('lib.runtime_none', { format: formatLabel });
    if (c.status === 'unsupported_platform') return `${c.runtime} · ${t('ort.state_unsupported')}`;
    return c.status === 'needs_runtime' ? t('lib.runtime_needs', { runtime: c.runtime }) : c.runtime;
  });
  let needsRuntime = $derived(entry.record.compatibility?.status === 'needs_runtime' ? entry.record.compatibility.runtime : null);
  /** The runtime has no build for this OS / architecture (ONNX on an Intel Mac): the file cannot be loaded here. */
  let platformBlocked = $derived.by(() => {
    const c = entry.record.compatibility;
    return c?.status === 'unsupported_platform' ? onnxUnsupportedText(c) : null;
  });

  /** Where the capability list comes from: a real run's `/props` or only the chat template text. */
  let capsSource = $derived.by(() => {
    if (!entry.caps.chat) return '';
    const v = entry.caps.verified;
    if (!v) return t('lib.caps_template');
    const when = new Date(v.observed_at * 1000).toLocaleDateString(locale());
    return entry.caps.reasoning && !v.reasoning
      ? t('lib.caps_verified_template_reasoning', { date: when })
      : t('lib.caps_verified', { date: when });
  });

  /** "LLM · 8B · Q4_K_M · 4.9 GB": what tells two files apart, in reading order. */
  let summary = $derived(
    [
      m.kind === 'asr' ? t(speechKindKey(m)) : entry.record.runtime === 'mlx-audio' ? t('lib.kind_tts_mlx') : t(KIND[m.kind ?? 'unknown']),
      entry.multimodal ? t('lib.kind_vlm') : null,
      uiLevel.advanced ? params : null,
      uiLevel.advanced && m.quantization && m.quantization !== 'Unknown' ? m.quantization : null,
      formatBytes(m.size_bytes),
      uiLevel.advanced && entry.draft_for.length > 0 ? t('lib.draft_badge') : null,
    ]
      .filter(Boolean)
      .join(' · ')
  );

  let lastUsed = $derived(relativeTime(entry.last_used_at, locale()));
  let added = $derived(relativeTime(entry.added_at, locale()));
  function presetLabel(id: string): string {
    const p = domain.preset(id);
    return p ? presetDisplayName(p) : id;
  }
  let presetName = $derived(entry.preset_used ? presetLabel(entry.preset_used) : null);
  let lastDef = $derived(domain.definition(entry.last_definition_id)?.alias ?? null);

  let selectedIsProfile = $derived(
    app.selectedServerId !== DEFAULT_SERVER_ID && app.profiles.some((p) => p.id === app.selectedServerId)
  );
  function profilesOf(def: ModelDefinition) {
    return app.profiles.filter((p) => p.config.definition_id === def.id);
  }

  async function load(def: ModelDefinition, target: LoadTarget) {
    busy[def.id] = true;
    try {
      const serverId = await invoke<string>('load_definition', { definitionId: def.id, target });
      await app.refreshProfiles();
      toast.success(t('lib.loading_toast', { alias: def.alias, server: app.serverLabel(serverId) }));
      await domain.refreshStates();
      onChanged();
    } catch (e) {
      toast.error(t('lib.load_failed', { error: localizeError(e) }));
    } finally {
      busy[def.id] = false;
    }
  }

  /** Restarts the server that runs the definition (a profile keeps its own settings). */
  function reload(def: ModelDefinition, serverId: string) {
    const target: LoadTarget = app.profiles.some((p) => p.id === serverId && p.config.definition_id === def.id)
      ? { kind: 'profile', profile_id: serverId }
      : { kind: 'server', server_id: serverId };
    return load(def, target);
  }

  async function unload(def: ModelDefinition, serverId: string) {
    busy[def.id] = true;
    try {
      await invoke('stop_server', { serverId });
      await domain.refreshStates();
    } catch (e) {
      toast.error(String(e));
    } finally {
      busy[def.id] = false;
    }
  }


  async function reveal() {
    try {
      await invoke('reveal_model_in_folder', { modelId: m.id });
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(m.path);
      toast.success(t('lib.path_copied'));
    } catch {
      toast.error(t('lib.copy_failed'));
    }
  }

  let repairing = $state(false);
  /** Address of one part, typed when nothing records where the file came from. */
  let repairUrl = $state('');
  async function repair(url: string | null = null) {
    repairing = true;
    try {
      await invoke<number>('repair_model_shards', { modelId: m.id, url });
      toast.success(t('lib.repair_queued', { count: entry.shards?.missing.length ?? 0 }), {
        duration: ACTION_TOAST_DURATION,
        closeButton: true,
        action: { label: t('lib.open_hub'), onClick: () => (app.requestedTab = 'hub') },
      });
    } catch (e) {
      toast.error(t('lib.repair_failed', { error: String(e) }));
    } finally {
      repairing = false;
    }
  }

  /** An MLX chat or vision folder: loaded like a GGUF chat model (its engine follows the folder). */
  let mlxChat = $derived(m.is_ready && m.kind === 'llm' && (entry.record.runtime === 'mlx' || entry.record.runtime === 'mlx-vlm'));

  let startingChat = $state(false);
  async function startChat() {
    startingChat = true;
    try {
      await loadAndChat(m);
      onChanged();
    } finally {
      startingChat = false;
    }
  }

  let openingPlayground = $state(false);
  /** The tab this file runs in (image, speech, embedding, rerank), or null for chat models and unfinished files. */
  let playgroundTab = $derived(playgroundTarget(m, entry.record.runtime));
  async function openPlayground() {
    openingPlayground = true;
    try {
      await openInPlayground(m, entry.record.runtime);
      onChanged();
    } finally {
      openingPlayground = false;
    }
  }

  function openInDashboard() {
    library.pendingSelectId = m.id;
    // The selected server is up: loading this model into its form would edit a running server.
    library.pendingNewServer = app.isRunning || app.isBusy;
    app.requestedTab = 'dashboard';
  }

  /** The definition's load settings (ctx, ngl, ...) are edited in the Dashboard form. */
  function openLoadSettings(def: ModelDefinition) {
    library.pendingSelectId = m.id;
    library.pendingDefinitionId = def.id;
    app.requestedTab = 'dashboard';
  }

  const STATE_TONE: Record<string, string> = {
    running: 'text-success',
    loading: 'text-warning',
    idle: 'text-muted-foreground',
    missing: 'text-muted-foreground/70',
  };
  const STATE_LABEL: Record<string, TKey> = {
    running: 'pgm.state_running',
    loading: 'pgm.state_loading',
    idle: 'pgm.state_idle',
    missing: 'pgm.state_missing',
  };
</script>

{#snippet loadMenu(def: ModelDefinition)}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger class={cn(buttonVariants({ variant: 'outline', size: 'xs' }))} disabled={busy[def.id] || !!platformBlocked}>
      <Play />{t('pgm.load')}<ChevronDown class="opacity-60" />
    </DropdownMenu.Trigger>
    <DropdownMenu.Content align="end" class="w-64">
      <DropdownMenu.Label class="text-xs text-muted-foreground">{t('pgm.load_with_default')}</DropdownMenu.Label>
      <DropdownMenu.Item onclick={() => load(def, { kind: 'new_server' })}>{t('pgm.load_new')}</DropdownMenu.Item>
      {#if !selectedIsProfile}
        <DropdownMenu.Item onclick={() => load(def, { kind: 'server', server_id: app.selectedServerId })}>
          <span class="min-w-0 truncate">
            {app.isRunning || app.isBusy
              ? t('pgm.load_here_restart', { server: app.serverLabel(app.selectedServerId) })
              : t('pgm.load_here', { server: app.serverLabel(app.selectedServerId) })}
          </span>
        </DropdownMenu.Item>
      {/if}
      {#if profilesOf(def).length}
        <DropdownMenu.Separator />
        <DropdownMenu.Label class="text-xs text-muted-foreground">{t('pgm.load_with_profile')}</DropdownMenu.Label>
        {#each profilesOf(def) as p (p.id)}
          <DropdownMenu.Item onclick={() => load(def, { kind: 'profile', profile_id: p.id })}>
            <span class="min-w-0 truncate">{p.name} <span class="font-num text-muted-foreground">{savedPortText(p.config, t('dash.port_auto'))}</span></span>
          </DropdownMenu.Item>
        {/each}
      {/if}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/snippet}

<article
  class={cn(
    'flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3',
    running.length > 0 && 'border-success/40'
  )}
  aria-label={m.name}
>
  <!-- Title row -->
  <div class="flex min-w-0 items-start gap-2">
    <button
      type="button"
      class={cn('mt-0.5 shrink-0 rounded-sm text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring', entry.favorite && 'text-foreground hover:text-foreground')}
      aria-pressed={entry.favorite}
      aria-label={entry.favorite ? t('lib.unfavorite') : t('lib.favorite')}
      title={entry.favorite ? t('lib.unfavorite') : t('lib.favorite')}
      onclick={() => onToggleFavorite(entry)}
    >
      <Star class="size-4" fill={entry.favorite ? 'currentColor' : 'none'} />
    </button>
    <div class="flex min-w-0 flex-1 flex-col gap-1">
      <div class="flex min-w-0 items-center gap-1.5">
        {#if entry.repo_id}
          <PublisherAvatar name={entry.repo_id.split('/')[0]} class="size-5 rounded text-xs" />
        {/if}
        <span class="min-w-0 truncate text-sm font-medium" title={m.name}>{m.name}</span>
        {#if running.length > 0}
          <span class="size-1.5 shrink-0 rounded-full bg-success" aria-label={t('lib.running')}></span>
        {/if}
        {#if showUpdateBadge(library.updates, entry.repo_id, true)}
          <UpdateBadge class="shrink-0 text-xs" />
        {/if}
      </div>
      <!-- Primary facts in one line of text (no badge per fact); the rest is under "Details". -->
      <p class="truncate text-xs text-muted-foreground" title={summary}>{summary}</p>
    </div>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger class={cn(buttonVariants({ variant: 'ghost', size: 'icon-sm' }), 'shrink-0')} aria-label={t('lib.actions')}>
        <Ellipsis />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end" class="w-56">
        <DropdownMenu.Item onclick={reveal}><FolderOpen />{t('lib.reveal')}</DropdownMenu.Item>
        <DropdownMenu.Item onclick={copyPath}><Copy />{t('lib.copy_path')}</DropdownMenu.Item>
        {#if ['llm', 'embedding', 'reranker'].includes(m.kind ?? '') && m.is_ready}
          <DropdownMenu.Item onclick={openInDashboard}><LayoutDashboard />{t('lib.open_dashboard')}</DropdownMenu.Item>
        {/if}
        <DropdownMenu.Separator />
        {#if !linked}
          <DropdownMenu.Item onclick={() => onMove(entry)} disabled={running.length > 0}><FolderInput />{t('lib.move')}</DropdownMenu.Item>
        {/if}
        <DropdownMenu.Item variant="destructive" onclick={() => onDelete(entry)}>
          {#if linked}<Unlink />{t('lib.unlink')}{:else}<Trash2 />{t('lib.delete')}{/if}
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </div>

  {#if toolBadges.some(a => uiLevel.advanced || a?.tool_level)}
    <div class="flex flex-wrap gap-1">
      {#each toolBadges.filter(a => uiLevel.advanced || a?.tool_level) as agentModel}
        {@const rounds = (agentModel?.tool_rounds_ok ?? 0) + (agentModel?.tool_rounds_bad ?? 0)}
        <span
          data-tool-level={agentModel?.tool_level ?? 'unmeasured'}
          class="w-fit rounded-md border border-border bg-muted px-2 py-1 text-xs text-foreground"
          title={uiLevel.advanced ? (agentModel?.tool_level
            ? t(`td.level_${agentModel.tool_level}`, { ok: agentModel.tool_rounds_ok, n: rounds })
            : t('td.level_unmeasured', { n: rounds, need: TOOL_MIN_ROUNDS })) + (agentModel ? ' ' + t('td.recommended', { n: agentModel.recommended_tools }) : '') : undefined}
        >{matchingAgents.length > 1 ? `${agentModel?.alias} · ` : ''}{t(`td.badge_${agentModel?.tool_level ?? 'unmeasured'}`)}</span>
      {/each}
    </div>
  {/if}

  {#if fit}
    <p class="flex min-w-0 items-center gap-1.5 text-xs" title={uiLevel.advanced ? `${fit.description} · ${t(fitMemKey(fit), { vram: formatBytes(fit.requiredVramBytes, 1) })}` : undefined}>
      <span class={cn('size-1.5 shrink-0 rounded-full', FIT_DOT[fit.category])} aria-hidden="true"></span>
      <span class={cn('truncate', FIT_TONE[fit.category])}>{fit.label}</span>
    </p>
  {/if}

  {#if needsRuntime}
    <div class="text-xs text-warning" role="status"><EngineNeeded family={needsRuntime} /></div>
  {:else if platformBlocked}
    <p class="text-xs text-warning" role="status">{t(platformBlocked.key, platformBlocked.params)}</p>
  {/if}

  <!-- Parts of a split model -->
  {#if entry.shards && entry.shards.missing.length > 0}
    {@const s = entry.shards}
      <div class="flex flex-col gap-1 rounded-md border border-warning/30 bg-warning/10 px-2.5 py-2 text-xs" role="status">
        <span class="text-warning">{t('lib.shards_missing', { present: s.present, total: s.total })}</span>
        <AdvancedOnly><span class="truncate font-mono text-xs text-muted-foreground" title={s.missing.join('\n')}>{s.missing.join(', ')}</span></AdvancedOnly>
        {#if entry.repair?.reason}
          <span class="text-muted-foreground">{localizeError(entry.repair.reason)}</span>
          {#if entry.repair.url_allowed}
            <form
              class="flex min-w-0 items-center gap-1.5"
              onsubmit={(e) => {
                e.preventDefault();
                void repair(repairUrl);
              }}
            >
              <FieldHelp text={t('help.repair_url')}><Input class="h-7 min-w-0 flex-1 font-mono text-xs" bind:value={repairUrl} placeholder={t('lib.repair_url_placeholder')} aria-label={t('lib.repair_url_label')} disabled={repairing} /></FieldHelp>
              <Button size="xs" variant="outline" type="submit" disabled={repairing || !repairUrl.trim()}>
                {#if repairing}<LoaderCircle class="animate-spin" />{:else}<Wrench />{/if}{t('lib.repair_url_button')}
              </Button>
            </form>
          {/if}
        {:else if entry.repair?.repo_id}
          <Button size="xs" variant="outline" class="self-start" onclick={() => repair()} disabled={repairing}>
            {#if repairing}<LoaderCircle class="animate-spin" />{:else}<Wrench />{/if}{t('lib.repair', { repo: entry.repair.repo_id })}
          </Button>
        {:else if entry.repair?.source_url}
          <span class="truncate font-mono text-xs text-muted-foreground" title={entry.repair.source_url}>{entry.repair.source_url}</span>
          <Button size="xs" variant="outline" class="self-start" onclick={() => repair()} disabled={repairing}>
            {#if repairing}<LoaderCircle class="animate-spin" />{:else}<Wrench />{/if}{t('lib.repair_recorded')}
          </Button>
        {/if}
      </div>
  {/if}

  <!-- Usage -->
  {#if running.length > 0 || lastUsed}
  <div class="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
    {#if running.length > 0}
      <span class="text-success">{t('lib.running_on', { servers: running.map((s) => uiLevel.advanced ? `${app.serverLabel(s.id)} :${s.active?.port ?? ''}` : app.serverLabel(s.id)).join(', ') })}</span>
    {/if}
    {#if lastUsed}<span title={new Date(entry.last_used_at ?? 0).toLocaleString(locale())}>{t('lib.last_used', { when: lastUsed })}{uiLevel.advanced && lastDef ? ` · ${lastDef}` : ''}</span>{/if}
  </div>
  {/if}

  <AdvancedOnly>
  <details class="group/details text-xs text-muted-foreground">
    <summary class="flex w-fit cursor-pointer list-none items-center gap-1 rounded-sm text-muted-foreground outline-none select-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 [&::-webkit-details-marker]:hidden">
      <ChevronRight class="size-3.5 transition-transform group-open/details:rotate-90 motion-reduce:transition-none" />
      {t('ui.lib_details')}
    </summary>
    <div class="mt-1.5 flex flex-col gap-1.5 pl-4.5">
      <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5">
        {#if m.architecture}<dt class="text-muted-foreground">{t('lib.arch')}</dt><dd class="truncate font-mono">{m.architecture}</dd>{/if}
        {#if m.context_length}<dt class="text-muted-foreground">{t('lib.ctx')}</dt><dd class="font-num">{m.context_length.toLocaleString(locale())}</dd>{/if}
        {#if fit}<dt class="text-muted-foreground">{t('lib.fit')}</dt><dd class="font-num">{t(fitMemKey(fit), { vram: formatBytes(fit.requiredVramBytes, 1) })}</dd>{/if}
        {#if caps.length > 0}<dt class="text-muted-foreground">{t('lib.caps')}</dt><dd>{caps.join(', ')}</dd>{/if}
        {#if runtimeText}<dt class="text-muted-foreground">{t('lib.runtime')}</dt><dd class="truncate">{formatLabel} · {runtimeText}</dd>{/if}
        {#if capsSource}<dt class="text-muted-foreground">{t('lib.caps_source')}</dt><dd class="font-num text-xs">{capsSource}</dd>{/if}
        {#if linked}<dt class="text-muted-foreground">{t('ui.lib_source')}</dt><dd class="truncate">{t('lib.linked', { source: m.source ?? '' })}</dd>{/if}
        {#if presetName}<dt class="text-muted-foreground">{t('ui.lib_preset')}</dt><dd class="truncate">{presetName}</dd>{/if}
        {#if entry.shards && entry.shards.missing.length === 0}<dt class="text-muted-foreground">{t('ui.lib_parts')}</dt><dd>{t('lib.shards_ok', { total: entry.shards.total })}</dd>{/if}
        {#if added}<dt class="text-muted-foreground">{t('ui.lib_added')}</dt><dd title={new Date(entry.added_at ?? 0).toLocaleString(locale())}>{added}</dd>{/if}
      </dl>
    {#if entry.pairs.length > 0}
      <p class="truncate text-xs text-muted-foreground" title={entry.pairs.map(nameOf).join('\n')}>
        {m.kind === 'mmproj' ? t('lib.projector_for', { names: entry.pairs.map(nameOf).join(', ') }) : t('lib.projector', { names: entry.pairs.map(nameOf).join(', ') })}
      </p>
    {/if}
    {#if entry.draft_for.length > 0}
      <p class="truncate text-xs text-muted-foreground" title={entry.draft_for.map(nameOf).join('\n')}>{t('lib.draft_for', { names: entry.draft_for.map(nameOf).join(', ') })}</p>
    {/if}
    {#if drafts.length > 0}
      <p class="truncate text-xs text-muted-foreground">{t('lib.suggested_drafts', { names: drafts.map((d) => d.model.name).join(', ') })}</p>
    {/if}
    <!-- Path -->
    <button type="button" class="min-w-0 truncate text-left font-mono text-xs text-muted-foreground hover:text-foreground" title={t('lib.copy_path_title', { path: m.path })} onclick={copyPath}>
      {m.path}
    </button>
    </div>
  </details>
  </AdvancedOnly>

  <!-- Primary action: load the chat model and start talking to it. -->
  {#if isChatLoadable(m) || mlxChat}
    <div class="flex items-center gap-2 border-t border-border pt-2">
      <Button size="xs" onclick={startChat} disabled={startingChat || !!platformBlocked}>
        {#if startingChat}<LoaderCircle class="animate-spin" />{:else}<MessageSquare />{/if}{running.length > 0 ? t('hub.chat_now') : t('hub.load_and_chat')}
      </Button>
    </div>
  {:else if playgroundTab}
    <!-- Image, speech, voice, embedding and rerank files: open the Playground tab with the file selected (its definition is made when missing). -->
    <div class="flex items-center gap-2 border-t border-border pt-2">
      <Button size="xs" onclick={openPlayground} disabled={openingPlayground}>
        {#if openingPlayground}<LoaderCircle class="animate-spin" />{:else}<SquareArrowOutUpRight />{/if}{t('lib.open_in_playground')}
      </Button>
    </div>
  {/if}

  <!-- Definitions (aliases) -->
  {#if !sherpaAsr && ['llm', 'embedding', 'reranker', 'asr', 'image'].includes(m.kind ?? '')}
    {#if !uiLevel.advanced}
      <div class="flex flex-wrap items-center gap-2 border-t border-border pt-2">
        {#if definitions.length}
          {@const def = definitions.find(d => d.id === entry.last_definition_id) ?? definitions[0]}
          {@const state = domain.state(def.id)?.state ?? 'idle'}
          {#if state === 'idle' && !GATEWAY_LOADED.includes(m.kind ?? '')}
            {@render loadMenu(def)}
          {:else}
            <Button variant="outline" size="xs" disabled><Play />{t('pgm.load')}</Button>
            <span class="text-xs text-muted-foreground">{m.kind === 'asr' ? t('lib.asr_load_note') : m.kind === 'image' ? t('lib.image_load_note') : t(STATE_LABEL[state])}</span>
          {/if}
        {:else if !playgroundTab}
          <Button size="xs" variant="outline" onclick={() => onNewDefinition(entry)} disabled={!m.is_ready}><Plus />{t('lib.new_definition')}</Button>
          {#if !m.is_ready}<p class="text-xs text-muted-foreground">{t('lib.incomplete_hint')}</p>{/if}
        {/if}
      </div>
    {/if}
    <AdvancedOnly>
    <div class="flex flex-col gap-1 border-t border-border pt-2">
      <div class="flex items-center justify-between gap-2">
        <span class="text-xs font-medium text-muted-foreground">{t('lib.definitions')}</span>
        <Button size="xs" variant="ghost" onclick={() => onNewDefinition(entry)} disabled={!m.is_ready}><Plus />{t('lib.new_definition')}</Button>
      </div>
      {#if definitions.length === 0}
        <p class="text-xs text-muted-foreground">{m.is_ready ? t('lib.no_definitions') : t('lib.incomplete_hint')}</p>
      {/if}
      {#each definitions as def (def.id)}
        {@const st = domain.state(def.id)}
        {@const state = st?.state ?? 'idle'}
        <div class="flex min-w-0 items-center gap-1.5 rounded-md px-1 py-0.5 hover:bg-muted/40">
          <div class="flex min-w-0 flex-1 flex-col">
            <span class="truncate font-mono text-xs" title={def.description || def.alias}>{def.alias}</span>
            <span class="truncate text-xs">
              <span class={STATE_TONE[state]}>{t(STATE_LABEL[state])}</span>
              {#if st?.servers.length}
                <span class="text-muted-foreground"> · {st.servers.map((s) => `${app.serverLabel(s.server_id)} :${s.port}`).join(', ')}</span>
              {:else if def.default_preset_id}
                <span class="text-muted-foreground"> · {presetLabel(def.default_preset_id)}</span>
              {/if}
            </span>
          </div>
          {#if busy[def.id] || state === 'loading'}
            <LoaderCircle class="size-3.5 shrink-0 animate-spin text-muted-foreground" aria-label={t('pgm.state_loading')} />
          {/if}
          {#if state === 'running' && st}
            {#each st.servers.filter((s) => s.status === 'Running') as s (s.server_id)}
              <Tooltip.Root>
                <Tooltip.Trigger>
                  {#snippet child({ props })}
                    <Button {...props} size="icon-xs" variant="ghost" disabled={busy[def.id]} aria-label={t('lib.reload')} onclick={() => reload(def, s.server_id)}><RotateCw /></Button>
                  {/snippet}
                </Tooltip.Trigger>
                <Tooltip.Content>{t('lib.reload_title', { server: app.serverLabel(s.server_id) })}</Tooltip.Content>
              </Tooltip.Root>
              <Button size="xs" variant="ghost" disabled={busy[def.id]} onclick={() => unload(def, s.server_id)} title={t('pgm.unload_title', { server: app.serverLabel(s.server_id) })}>
                <Square />{t('pgm.unload')}
              </Button>
            {/each}
          {:else if state === 'idle' && !GATEWAY_LOADED.includes(m.kind ?? '')}
            <!-- A whisper model is loaded by the gateway on the first speech request, not by hand. -->
            {@render loadMenu(def)}
          {/if}
          <DropdownMenu.Root>
            <DropdownMenu.Trigger class={cn(buttonVariants({ variant: 'ghost', size: 'icon-xs' }))} aria-label={t('lib.definition_menu', { alias: def.alias })}>
              <Pencil />
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="end" class="w-60">
              <DropdownMenu.Item onclick={() => onEditDefinition(def)}>{t('lib.edit_definition')}</DropdownMenu.Item>
              {#if !GATEWAY_LOADED.includes(m.kind ?? '')}
                <DropdownMenu.Item onclick={() => openLoadSettings(def)}>{t('lib.load_settings')}</DropdownMenu.Item>
              {/if}
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </div>
      {/each}
    </div>
    </AdvancedOnly>
  {/if}
</article>
