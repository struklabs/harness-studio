<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import HelpPopover from '../common/HelpPopover.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { toast } from 'svelte-sonner';
  import { ChevronDown, ExternalLink, FlaskConical, History, KeyRound, Play, RefreshCw, RotateCcw, TriangleAlert, SlidersHorizontal } from '@lucide/svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';
  import { invoke } from '#lib/ipc.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Collapsible from '#lib/components/ui/collapsible/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { cn } from '#lib/utils.js';
  import CopyButton from './CopyButton.svelte';
  import {
    applyIntegration,
    baseCatalog,
    checkAliasDrift,
    checkEndpointDrift,
    type AliasDrift,
    type EndpointDrift,
    listIntegrations,
    OPENCODE_FORMAT,
    openOpencodeDesktop,
    previewIntegration,
    revertIntegration,
    testConnection,
    type ApplyContext,
    type FileChange,
    type Integration,
    type IntegrationId,
  } from './integrations';
  import AgentModelNotes from './AgentModelNotes.svelte';
  import { findAgentModel, listAgentModels, chatModelsOnly, hasChatContext, opencodePromptTokens, type AgentModel } from './agentModels';
  import { buildSnippet, PLACEHOLDER_KEY } from './snippets';
  import { buildGuide } from './guides';
  import LaunchDialog from './LaunchDialog.svelte';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { diffStats, lineDiff } from './lineDiff';
  import { applyTuning, getRunningConfig, isTuned, kvText, planTuning, servedModel, type RunningConfig, type TuningPlan } from './agentTuning';
  import PrepareOptions from './PrepareOptions.svelte';
  import { pathExample } from '#lib/platform.js';
  import type { ServerStatus } from '#lib/types.js';

  interface Props {
    /** Model ID reported by the running server (used in snippets and written configs). */
    modelId: string;
    contextSize: number;
    page?: 'config' | 'guide' | 'sdk';
  }

  let { modelId, contextSize, page = 'config' }: Props = $props();

  let items = $state<Integration[]>(baseCatalog());
  let loading = $state(true);
  let loadError = $state('');
  /** Id of the integration with a backend call in flight (preview, apply or revert). */
  let busy = $state<IntegrationId | null>(null);
  let feedback = $state<Record<IntegrationId, { message: string; isError: boolean }>>({});
  let open = $state<Record<IntegrationId, boolean>>({});
  /** Why the preview's model was preselected (null = the user picked it). */
  type ModelReason = 'served' | 'loaded' | 'configured' | 'first' | null;
  interface PreviewChoice {
    alias: string;
    /** Point the tool's default model at Harness. */
    setDefault: boolean;
    reason: ModelReason;
  }
  let preview = $state<{
    item: Integration;
    changes: FileChange[];
    ctx: ApplyContext;
    /** The request before the model choice was applied (a new choice starts from it). */
    base: ApplyContext;
    /** OpenCode-format tools: the model choice shown above the diff. */
    choice: PreviewChoice | null;
    /** Open this OpenCode desktop app after applying. */
    openDesktop: string | null;
    hideExternalSkills: boolean;
  } | null>(null);
  /** Chat models with tool-calling verdicts and prompt speed (empty until loaded). */
  let agentModels = $state<AgentModel[]>([]);
  /** Launch config of the selected server's current run (null = not running). */
  let running = $state<RunningConfig | null>(null);
  let tuning = $state<{ item: Integration; plan: TuningPlan; sel: number } | null>(null);
  /** What the target server reports at /v1/models right now (null = unreachable / not running). */
  let served = $state<{ id: string; nCtx: number } | null>(null);
  /** Tools whose saved config names models Harness no longer serves (by integration id). */
  let drift = $state<Record<string, AliasDrift>>({});
  /** Tools set up for another address than the running gateway's (by integration id). */
  let moved = $state<Record<string, EndpointDrift>>({});
  let alive = true;

  // While the gateway runs, integrations are written for the managed endpoint (the model alias
  // routes to the server); the server's own key is not needed there.
  // Through the gateway the model name is a definition alias. The default is the served one;
  // the reason is shown in the apply dialog, where the user can pick another.
  let gatewayPick = $derived.by((): { def: (typeof domain.definitions)[number]; reason: ModelReason } | undefined => {
    if (!app.gatewayRunning) return undefined;
    // Embedding/rerank definitions cannot drive a chat tool.
    const chat = (d: (typeof domain.definitions)[number]) => findAgentModel(chatModelsOnly(agentModels), d.alias) !== undefined;
    const defs = domain.definitions.filter(chat);
    const byFile = findAgentModel(agentModels, modelId)?.alias;
    const served =
      defs.find((d) => d.alias === modelId) ??
      // A second server of a shared definition serves `<alias>-<port>`: match it by server.
      defs.find((d) => domain.state(d.id)?.servers.some((s) => s.server_id === app.selectedServerId)) ??
      // A server started without `--alias` reports the file name.
      defs.find((d) => d.alias === byFile);
    if (served) return { def: served, reason: 'served' };
    const loaded = defs.find((d) => domain.state(d.id)?.state === 'running');
    if (loaded) return { def: loaded, reason: 'loaded' };
    const first = defs[0];
    return first ? { def: first, reason: 'first' } : undefined;
  });
  let gatewayDef = $derived(gatewayPick?.def);
  let chatContextAvailable = $derived(hasChatContext(
    app.gatewayRunning,
    !!gatewayDef && domain.state(gatewayDef.id)?.state === 'running',
    app.isRunning,
    app.activeServer?.serve_mode,
  ));
  let ctx = $derived({
    baseUrl: app.inferenceUrl,
    apiKey: app.gatewayRunning ? null : (app.apiKey ?? null),
    modelId: gatewayDef?.alias ?? modelId,
    contextSize: (app.gatewayRunning ? Number(gatewayDef?.load?.context_size) || 0 : 0) || contextSize || 32768,
  });
  /** Key the connection test sends (the gateway client token while the gateway runs). */
  let testKey = $derived(app.gatewayRunning ? (app.gateway?.client_token ?? null) : (app.apiKey ?? null));
  let launching = $state<Integration | null>(null);
  let testing = $state<IntegrationId | null>(null);
  /** LibreChat: librechat.yaml path typed by the user. */
  let paths = $state<Record<IntegrationId, string>>({});
  let sections = $derived([
    { kind: 'config', title: t('intg.section_config'), desc: t('intg.section_config_desc'), list: items.filter((i) => i.kind === 'config') },
    { kind: 'guide', title: t('intg.section_guide'), desc: t('intg.section_guide_desc'), list: items.filter((i) => i.kind === 'guide') },
    { kind: 'sdk', title: t('intg.section_sdk'), desc: t('intg.section_sdk_desc'), list: items.filter((i) => i.kind === 'sdk') },
  ]);

  onDestroy(() => (alive = false));
  onMount(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const refresh = () => {
      if (alive && app.gatewayRunning) {
        void domain.refreshDefinitions();
        void domain.refreshStates();
      }
    };
    const visible = () => {
      if (document.visibilityState === 'visible') refresh();
    };
    for (const event of ['definitions-changed', 'servers-changed', 'gateway-changed', 'window-visible']) {
      void listen(event, refresh).then((unlisten) => {
        if (disposed) unlisten();
        else cleanups.push(unlisten);
      }).catch(() => {});
    }
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', visible);
    refresh();
    return () => {
      disposed = true;
      for (const cleanup of cleanups) cleanup();
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', visible);
    };
  });
  $effect(() => {
    if (app.gatewayRunning) {
      void domain.refreshDefinitions();
      void domain.refreshStates();
    }
  });
  // Re-detect whenever the running server's model or context size changes.
  $effect(() => {
    void ctx.modelId;
    void ctx.contextSize;
    void ctx.baseUrl;
    untrack(load);
  });

  let loadSequence = 0;
  async function load() {
    const sequence = ++loadSequence;
    loading = true;
    loadError = '';
    try {
      const [list, cfg, live, models] = await Promise.all([
        listIntegrations(ctx),
        app.isRunning ? getRunningConfig(app.selectedServerId).catch(() => null) : Promise.resolve(null),
        app.isRunning ? servedModel(app.baseUrl, app.authHeaders()).catch(() => null) : Promise.resolve(null),
        listAgentModels().catch(() => [] as AgentModel[]),
      ]);
      const names = [app.activeServer?.serve_mode === 'chat' ? live?.id : null, ...(app.gatewayRunning ? chatModelsOnly(models).map((m) => m.alias) : [])].filter((n): n is string => !!n);
      const drifted = await checkAliasDrift(names).catch(() => [] as AliasDrift[]);
      // While the gateway runs every tool should use its address (the one port Harness keeps
      // working when it has to move).
      const gwBase = app.gatewayRunning ? (app.gateway?.status.endpoint?.base_url ?? null) : null;
      const away = gwBase ? await checkEndpointDrift(gwBase).catch(() => [] as EndpointDrift[]) : [];
      if (alive && sequence === loadSequence) {
        items = list;
        const chatEndpoint = app.activeServer?.serve_mode === 'chat';
        running = chatEndpoint ? cfg : null;
        served = chatEndpoint ? live : null;
        agentModels = models;
        drift = Object.fromEntries(drifted.map((d) => [d.id, d]));
        moved = Object.fromEntries(away.map((d) => [d.id, d]));
      }
    } catch (e) {
      if (alive && sequence === loadSequence) {
        items = baseCatalog();
        loadError = String(e);
      }
    } finally {
      if (alive && sequence === loadSequence) loading = false;
    }
  }

  async function reloadAgentModels() {
    const models = await listAgentModels().catch(() => null);
    if (alive && models) agentModels = models;
  }

  /** Chat models the gateway serves (what an OpenCode-format provider block lists). */
  let chatModels = $derived(app.gatewayRunning ? chatModelsOnly(agentModels) : []);

  function isHarnessDefault(model: string | null): boolean {
    return !model || model.startsWith('harness/') || model.startsWith('harness:');
  }

  /** The request for an OpenCode-format tool: the chosen model, every gateway chat model, the default-model choice. */
  function choiceCtx(base: ApplyContext, choice: PreviewChoice): ApplyContext {
    const own = findAgentModel(chatModels, choice.alias);
    return {
      ...base,
      modelId: choice.alias,
      contextSize: (app.gatewayRunning ? own?.context_size : null) || base.contextSize,
      models: chatModels.map((m) => ({ id: m.alias, context_size: m.context_size ?? 0 })),
      setDefaultModel: choice.setDefault,
    };
  }

  function reasonText(item: Integration, reason: ModelReason): string {
    switch (reason) {
      case 'served':
        return t('agent.model_reason_served');
      case 'loaded':
        return t('agent.model_reason_loaded');
      case 'configured':
        return t('agent.model_reason_configured', { tool: item.name });
      case 'first':
        return t('agent.model_reason_first');
      default:
        return '';
    }
  }

  /** Re-renders the diff for a changed model or default-model choice. */
  async function updateChoice(patch: Partial<PreviewChoice>) {
    if (!preview?.choice || busy) return;
    const choice: PreviewChoice = { ...preview.choice, ...patch, reason: patch.alias !== undefined ? null : preview.choice.reason };
    const item = preview.item;
    const useCtx = choiceCtx(preview.base, choice);
    busy = item.id;
    try {
      const changes = await previewIntegration(item.id, useCtx);
      if (alive && preview && preview.item.id === item.id) preview = { ...preview, changes, ctx: useCtx, choice };
    } catch (e) {
      if (alive) fail(item.id, e);
    } finally {
      if (alive) busy = null;
    }
  }

  function fail(id: IntegrationId, e: unknown) {
    const message = (e instanceof Error ? localizeError(e.message) : localizeError(e)) || t('conn.error_generic');
    feedback[id] = { message, isError: true };
    toast.error(message);
  }

  /** The model id the tool has saved when it differs from what the target server serves now. */
  function staleModel(item: Integration): string | null {
    const old = item.configuredModel;
    return old && served && old !== served.id ? old : null;
  }

  /** Moves a tool off a model name that no longer exists (opens the usual preview first). */
  function fixDrift(item: Integration, d: AliasDrift) {
    const alias = d.suggestion ?? ctx.modelId;
    return openPreview(item, { ...ctx, modelId: alias }, { pick: alias });
  }

  /** Rewrites the tool's config with the model id the server serves now. */
  function refreshModel(item: Integration) {
    if (!served) return;
    return openPreview(item, { ...ctx, modelId: served.id, contextSize: served.nCtx || ctx.contextSize });
  }

  /**
   * Step 1: show which files will change before writing anything. `pick` = a model the user
   * already chose (launch dialog); `openDesktop` = open that OpenCode desktop app after applying.
   */
  async function openPreview(item: Integration, useCtx: ApplyContext = ctx, opts: { pick?: string; openDesktop?: string; hideExternalSkills?: boolean } = {}) {
    if (busy) return;
    let choice: PreviewChoice | null = null;
    const base = useCtx;
    const available = chatModelsOnly(agentModels);
    if (app.gatewayRunning ? !findAgentModel(available, opts.pick ?? useCtx.modelId) : !chatContextAvailable) {
      feedback[item.id] = { message: t('agent.no_chat_model'), isError: true };
      return;
    }
    useCtx = { ...useCtx, models: app.gatewayRunning ? available.map((m) => ({ id: m.alias, context_size: m.context_size ?? 0 })) : [] };
    if (OPENCODE_FORMAT.has(item.id)) {
      const alias = opts.pick ?? useCtx.modelId;
      const reason: ModelReason = opts.pick
        ? null
        : useCtx.modelId !== ctx.modelId
          ? 'served'
          : item.configuredModel === alias
            ? 'configured'
            : (gatewayPick?.reason ?? 'served');
      choice = { alias, setDefault: isHarnessDefault(item.defaultModel), reason };
      useCtx = choiceCtx(useCtx, choice);
    }
    if (item.needsPath) {
      const p = (paths[item.id] ?? '').trim();
      if (!p) {
        feedback[item.id] = { message: t('intg.path_required'), isError: true };
        open[item.id] = true;
        return;
      }
      useCtx = { ...useCtx, targetPath: p };
    }
    busy = item.id;
    delete feedback[item.id];
    try {
      const changes = await previewIntegration(item.id, useCtx);
      if (alive) preview = { item, changes, ctx: useCtx, base, choice, openDesktop: opts.openDesktop ?? null, hideExternalSkills: opts.hideExternalSkills ?? true };
    } catch (e) {
      if (alive) fail(item.id, e);
    } finally {
      if (alive) busy = null;
    }
  }

  /** Step 2: write the configs the user just reviewed. */
  async function confirmApply() {
    if (!preview || busy) return;
    const id = preview.item.id;
    busy = id;
    try {
      const applied = preview;
      const result = await applyIntegration(id, applied.ctx);
      if (!alive) return;
      preview = null;
      const warn = result.warnings.length ? `
${t('intg.warnings', { warnings: result.warnings.join(' · ') })}` : '';
      const listed = applied.choice
        ? `
${t('agent.applied_models', {
            count: Math.max(1, applied.ctx.models?.length ?? 0),
            state: applied.choice.setDefault ? t('agent.applied_default_set', { model: applied.choice.alias }) : t('agent.applied_default_kept'),
          })}`
        : '';
      const message = id === 'zed'
        ? `${t('intg.configured_summary', { name: applied.item.name, model: applied.ctx.modelId, context: applied.ctx.contextSize })}\n${t('intg.zed_key_required')}`
        : result.message;
      feedback[id] = { message: message + listed + warn, isError: false };
      toast.success(t('conn.client_applied'));
      if (applied.openDesktop) {
        try {
          await openOpencodeDesktop(applied.openDesktop, applied.hideExternalSkills);
          toast.success(t('agent.desktop_opened'));
        } catch (e) {
          fail(id, e);
        }
      }
      await load();
    } catch (e) {
      if (!alive) return;
      preview = null;
      fail(id, e);
    } finally {
      if (alive) busy = null;
    }
  }

  /** Shows what "prepare the server" would change. */
  async function openTuning(item: Integration) {
    if (busy || !running) return;
    busy = item.id;
    try {
      const plan = await planTuning(running, app.selectedServerId, item.id, item.minCtx);
      if (alive) tuning = { item, plan, sel: plan.selected };
    } catch (e) {
      if (alive) fail(item.id, e);
    } finally {
      if (alive) busy = null;
    }
  }

  async function waitUntilRunning(serverId: string): Promise<void> {
    const deadline = Date.now() + 180_000;
    while (Date.now() < deadline) {
      const status = await invoke<ServerStatus>('get_server_status', { serverId });
      if (status === 'Running') return;
      if (typeof status === 'object' && 'Error' in status) throw new Error(status.Error);
      await new Promise((r) => setTimeout(r, 500));
    }
    throw new Error(t('conn.tune_timeout'));
  }

  /** Restarts the server with the plan, then offers to write the tool's config for it. */
  async function confirmTuning() {
    if (!tuning || busy || tuning.sel < 0) return;
    const { item, plan } = tuning;
    const option = plan.options[tuning.sel];
    if (!option) return;
    const serverId = app.selectedServerId;
    busy = item.id;
    try {
      await applyTuning(serverId, option, plan.from);
      await waitUntilRunning(serverId);
      if (!alive) return;
      tuning = null;
      toast.success(t('conn.tune_done', { name: item.name }));
      busy = null;
      await load();
      // The tool's own config carries the model id and context size. The page's cached
      // model id is stale right after a restart, so ask the server directly.
      const served = await servedModel(app.baseUrl, app.authHeaders()).catch(() => null);
      if (!alive || !served) return;
      await openPreview(item, { ...ctx, modelId: served.id, contextSize: served.nCtx || option.context_size });
    } catch (e) {
      if (!alive) return;
      tuning = null;
      fail(item.id, e);
    } finally {
      if (alive) busy = null;
    }
  }

  async function runTest(item: Integration) {
    if (!item.testApi || testing) return;
    if (!app.gatewayRunning && !app.isRunning) {
      feedback[item.id] = { message: t('intg.test_no_server'), isError: true };
      return;
    }
    testing = item.id;
    delete feedback[item.id];
    try {
      const r = await testConnection(item.testApi, { ...ctx, apiKey: testKey });
      if (!alive) return;
      const api = r.api === 'anthropic' ? 'Anthropic Messages' : r.api === 'responses' ? 'Responses' : 'Chat Completions';
      feedback[item.id] = r.ok
        ? { message: t(r.tool_called ? 'intg.test_ok_tool' : 'intg.test_ok', { api, ms: r.latency_ms }), isError: false }
        : { message: t('intg.test_fail', { api, detail: localizeError(r.detail) }), isError: true };
    } catch (e) {
      if (alive) fail(item.id, e);
    } finally {
      if (alive) testing = null;
    }
  }

  async function revert(item: Integration) {
    if (busy) return;
    busy = item.id;
    delete feedback[item.id];
    try {
      const outcome = await revertIntegration(item.id);
      if (!alive) return;
      toast.success(t('conn.client_reverted'));
      if (outcome.warnings.length) {
        feedback[item.id] = { message: t('intg.warnings', { warnings: outcome.warnings.join(' · ') }), isError: false };
      }
      await load();
    } catch (e) {
      if (alive) fail(item.id, e);
    } finally {
      if (alive) busy = null;
    }
  }
</script>

<section class="flex min-w-0 flex-col gap-3" aria-labelledby="conn-clients-title">
  <div class="flex items-center justify-between gap-3">
    <div class="flex min-w-0 flex-col">
      <h2 id="conn-clients-title" class="flex items-center gap-1 text-base font-semibold">{t('conn.clients_title')}<HelpPopover text={t('conn.clients_desc')} label={t('conn.clients_title')} /></h2>
    </div>
    <Button variant="ghost" size="sm" onclick={load} disabled={loading}>
      <RefreshCw class={cn(loading && 'animate-spin')} />
      {t('conn.clients_rescan')}
    </Button>
  </div>

  <!-- Which server the configs are written for -->
  <p class="rounded-lg border border-border bg-card px-3 py-2 text-xs text-muted-foreground" role="status">
    {t('conn.target_server', {
      name: app.gatewayRunning ? `${app.serverLabel(app.selectedServerId)} · ${t('gw.ep_via')}` : app.serverLabel(app.selectedServerId),
      url: app.inferenceUrl,
    })}
  </p>
  {#if app.gatewayRunning && app.gateway?.status.require_token}
    <p class="rounded-lg border border-warning/40 bg-warning/5 px-3 py-2 text-xs" role="note">{t('gw.integrations_token_note')}</p>
  {/if}

  <!-- API key situation -->
  <div
    class={cn(
      'flex items-start gap-2 rounded-lg border px-3 py-2 text-xs',
      app.apiKey ? 'border-warning/40 bg-warning/5 text-foreground' : 'border-border bg-card text-muted-foreground'
    )}
  >
    <KeyRound class={cn('mt-0.5 size-3.5 shrink-0', app.apiKey ? 'text-warning' : 'text-muted-foreground')} />
    <p class="min-w-0">
      {#if app.apiKey}
        {t('conn.key_set_note')}
      {:else}
        {t('conn.key_none_note', { placeholder: PLACEHOLDER_KEY })}
      {/if}
    </p>
  </div>

  {#if loading}
    <SectionLoading shape="inline" label={t('sl.conn_label')} phases={[t('sl.conn_1'), t('sl.conn_2')]} />
  {/if}

  {#if loadError}
    <p class="text-xs text-destructive" role="alert">{t('conn.clients_detect_failed', { message: loadError })}</p>
  {/if}

  {#each sections as section (section.kind)}
    {#if section.list.length && section.kind === page}
      <div class="flex flex-col gap-1.5">
        <div class="flex items-center gap-1.5">{#if uiLevel.advanced}<HelpPopover text={section.desc} label={t('conn.clients_title')} />{:else}<FieldHelp text={section.desc} />{/if}</div>
        <ul class="flex flex-col divide-y divide-border overflow-hidden rounded-lg border border-border bg-card">
          {#each section.list as item (item.id)}
            {@render row(item)}
          {/each}
        </ul>
      </div>
    {/if}
  {/each}
</section>

{#snippet row(item: Integration)}
  {@const fb = feedback[item.id]}
  {@const guide = item.kind === 'guide' ? buildGuide(item.id, ctx) : null}
  {@const snippet = buildSnippet(item.id, ctx, item.backendSnippet)}
  <li>
    <Collapsible.Root bind:open={() => open[item.id] ?? false, (v) => (open[item.id] = v)}>
      <div class="flex flex-wrap items-center gap-3 p-3">
        <div class="flex min-w-0 flex-1 flex-col gap-1">
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="text-sm font-medium">{item.name}</span>
            {#if item.installed === true}
              <Badge variant="outline" class="text-success">{t('conn.client_installed')}</Badge>
            {:else if item.installed === false}
              <Badge variant="outline" class="text-muted-foreground">{t('conn.client_not_installed')}</Badge>
            {/if}
            {#if item.configured}
              <Badge variant="outline" class="border-success/40 text-success">{t('conn.client_configured')}</Badge>
            {/if}
            {#if item.drifted.length}
              <Badge variant="outline" class="border-warning/40 text-warning" title={t('intg.drifted_tip', { files: item.drifted.join(', ') })}>{t('intg.drifted')}</Badge>
            {/if}
            {#if item.backups.length}
              <Badge variant="outline" class="text-muted-foreground" title={t('intg.backups_tip', { files: item.backups.join('\n') })}>
                <History class="size-3" />
                {t('intg.backups', { count: item.backups.length })}
              </Badge>
            {/if}
          </div>
          {#if item.configPaths.length}<AdvancedOnly>
            <span class="truncate font-mono text-xs text-muted-foreground" title={item.configPaths.join('\n')}>
              {item.configPaths[0]}{item.configPaths.length > 1 ? ` +${item.configPaths.length - 1}` : ''}
            </span></AdvancedOnly>
          {/if}
        </div>

        <div class="flex shrink-0 flex-wrap items-center gap-1.5">
          {#if item.applyable && item.configured && staleModel(item) && !drift[item.id]}
            <Button size="sm" variant="outline" onclick={() => refreshModel(item)} disabled={busy !== null}>
              {t('agent.update_model')}
            </Button>
          {/if}
          {#if item.minCtx > 0 && running && !isTuned(running, item.minCtx) && !app.gatewayRunning}
            <Button size="sm" variant={item.blockedByContext ? 'default' : 'outline'} onclick={() => openTuning(item)} disabled={busy !== null} title={t('conn.tune_tip')}>
              <SlidersHorizontal />
              {busy === item.id && !tuning ? t('conn.tune_checking') : t('conn.tune_button')}
            </Button>
          {/if}
          {#if item.testApi && item.kind !== 'sdk'}
            <Button size="sm" variant="ghost" onclick={() => runTest(item)} disabled={testing !== null} title={t('intg.test')}>
              <FlaskConical class={cn(testing === item.id && 'animate-pulse')} />
              {testing === item.id ? t('intg.testing') : t('intg.test')}
            </Button>
          {/if}
          {#if item.launch}
            <Button size="sm" variant="outline" onclick={() => (launching = item)} disabled={busy !== null} title={t('intg.launch_tip')}>
              <Play />
              {t('intg.launch')}
            </Button>
          {/if}
          {#if item.applyable}
            {#if item.configured}
              <Tooltip.Root>
                <Tooltip.Trigger>
                  {#snippet child({ props })}
                    <Button {...props} variant="ghost" size="icon-sm" onclick={() => revert(item)} disabled={busy !== null} aria-label={t('conn.client_revert')}>
                      <RotateCcw />
                    </Button>
                  {/snippet}
                </Tooltip.Trigger>
                <Tooltip.Content>{t('conn.client_revert_tip')}</Tooltip.Content>
              </Tooltip.Root>
            {/if}
            <Button size="sm" variant="outline" onclick={() => openPreview(item)} disabled={busy !== null}>
              {busy === item.id && !preview ? t('conn.client_configuring') : item.configured ? t('conn.client_update') : t('conn.client_configure')}
            </Button>
          {:else if item.blockedByContext && chatContextAvailable}
            <Badge variant="outline" class="text-warning" title={t('conn.client_ctx_too_small')}>{t('conn.client_ctx_short')}</Badge>
          {/if}
          {#if !guide}
            <CopyButton text={snippet} label={t('conn.copy_snippet')} copiedLabel={t('conn.snippet_copied')} variant="outline" size="sm" />
          {/if}
          <Collapsible.Trigger>
            {#snippet child({ props })}
              <Button {...props} variant="ghost" size="icon-sm" aria-label={t('conn.snippet_toggle_aria', { name: item.name })}>
                <ChevronDown class={cn('transition-transform', open[item.id] && 'rotate-180')} />
              </Button>
            {/snippet}
          </Collapsible.Trigger>
        </div>
      </div>

      {#if (item.configured || drift[item.id]?.user_blocks.length) && drift[item.id]}
        {@const d = drift[item.id]}
        <div class="flex flex-wrap items-start gap-x-2 gap-y-1.5 px-3 pb-3 text-xs text-foreground" role="status">
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span class="min-w-0 flex-1 break-words">
            {t('conn.alias_drift', { missing: d.missing.join(', ') })}
            {#if d.user_blocks.length}{t('conn.alias_drift_user', { blocks: d.user_blocks.join(', ') })}{/if}
            {#if d.suggestion}{t('conn.alias_drift_suggest', { alias: d.suggestion })}{/if}
          </span>
          {#if item.applyable}
            <Button size="xs" variant="outline" onclick={() => fixDrift(item, d)} disabled={busy !== null}>{t('conn.alias_drift_fix')}</Button>
          {/if}
        </div>
      {:else if item.configured && moved[item.id]}
        {@const d = moved[item.id]}
        <div class="flex flex-wrap items-start gap-x-2 gap-y-1.5 px-3 pb-3 text-xs text-foreground" role="status">
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span class="min-w-0 flex-1 break-words">{t('conn.endpoint_drift', { old: d.configured, current: d.current })}</span>
          {#if item.applyable}
            <Button size="xs" variant="outline" onclick={() => openPreview(item)} disabled={busy !== null}>{t('conn.alias_drift_fix')}</Button>
          {/if}
        </div>
      {:else if item.configured && staleModel(item)}
        <p class="flex items-start gap-1.5 px-3 pb-3 text-xs text-foreground" role="status">
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span class="min-w-0 break-words">{t('conn.stale_model', { old: item.configuredModel ?? '', current: served?.id ?? '' })}</span>
        </p>
      {/if}

      {#if item.id === 'zed'}
        <div class="flex flex-wrap items-start gap-2 px-3 pb-3 text-xs" role="note">
          <p class="min-w-0 flex-1 text-muted-foreground">{t('intg.zed_key_required')}</p>
          <CopyButton text={testKey?.trim() || PLACEHOLDER_KEY} label={t('intg.copy_key')} variant="outline" size="sm" />
        </div>
      {/if}

      {#if item.id === 'vscode-copilot'}
        <p class="px-3 pb-3 text-xs text-muted-foreground" role="note">{t('intg.vscode_config_note')}</p>
      {/if}

      {#if item.id === 'opencode'}
        <p class="px-3 pb-3 text-xs text-muted-foreground" role="note">{t('agent.opencode_delay_note')}</p>
        <AdvancedOnly><p class="px-3 pb-3 text-xs text-muted-foreground">{t('agent.opencode_card_hint')}</p></AdvancedOnly>
      {/if}

      {#if item.id === 'opendesign'}
        <p class="flex items-start gap-1.5 px-3 pb-3 text-xs text-muted-foreground">
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span class="min-w-0">{t('conn.opendesign_ctx_warning')}</span>
        </p>
      {/if}

      {#if fb}
        <p
          class={cn('px-3 pb-3 text-xs break-words whitespace-pre-line', fb.isError ? 'text-destructive' : 'text-success')}
          role={fb.isError ? 'alert' : 'status'}
        >
          {fb.message}
        </p>
      {/if}

      <Collapsible.Content>
        <div class="mx-3 mb-3 flex flex-col gap-2">
          {#if item.needsPath}
            <label class="flex flex-col gap-1 text-xs">
              <span class="font-medium">{t('intg.path_label')}</span>
              <FieldHelp text={t('help.client_path')}><input
                class="h-8 rounded-md border border-border bg-background px-2 font-mono text-xs"
                placeholder={pathExample('C:\\LibreChat\\librechat.yaml', '~/LibreChat/librechat.yaml')}
                bind:value={() => paths[item.id] ?? '', (v) => (paths[item.id] = v)}
              /></FieldHelp>
            </label>
          {/if}
          {#if guide}
            {#if guide.warn}
              <p class="flex items-start gap-1.5 rounded-md border border-warning/40 bg-warning/5 px-3 py-2 text-xs" role="note">
                <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
                <span class="min-w-0">{guide.warn}</span>
              </p>
            {/if}
            <ol class="flex list-decimal flex-col gap-1 pl-5 text-xs">
              {#each guide.steps as step, i (i)}
                <li>{step}</li>
              {/each}
            </ol>
            <dl class="grid grid-cols-[auto_1fr_auto] items-center gap-x-3 gap-y-1 rounded-md border border-border bg-muted/40 p-2 text-xs">
              {#each guide.fields as f (f.label)}
                <dt class="text-muted-foreground">{f.label}</dt>
                <dd class="min-w-0 truncate font-mono" title={f.value}>{f.value}</dd>
                <CopyButton text={f.value} ariaLabel={t('intg.copy_field_aria', { label: f.label })} variant="ghost" size="icon-sm" />
              {/each}
            </dl>
          {:else}
            <pre class="max-h-64 overflow-auto rounded-md border border-border bg-muted p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap break-all"><code>{snippet}</code></pre>
          {/if}
          {#if item.docsUrl}
            <Button variant="link" size="sm" class="h-auto self-start p-0 text-xs" onclick={() => item.docsUrl && openUrl(item.docsUrl)}>
              <ExternalLink class="size-3" />
              {t('intg.docs')}
            </Button>
          {/if}
        </div>
      </Collapsible.Content>
    </Collapsible.Root>
  </li>
{/snippet}

<LaunchDialog
  item={launching}
  {served}
  {agentModels}
  onModelsChanged={reloadAgentModels}
  onClose={() => (launching = null)}
  onTune={(i) => {
    launching = null;
    void openTuning(i);
  }}
  onDesktop={(i, alias, path, hideExternalSkills) => {
    launching = null;
    // The desktop app reads only the persistent config: write it (reviewed first), then open.
    const target = items.find((x) => x.id === i.id) ?? i;
    void openPreview(target, ctx, { pick: alias, openDesktop: path, hideExternalSkills });
  }}
/>

<!-- Prepare the server for an agent tool -->
<Dialog.Root
  bind:open={
    () => tuning !== null,
    (v) => {
      if (!v && !busy) tuning = null;
    }
  }
>
  <Dialog.Content class="sm:max-w-lg">
    {#if tuning}
      {@const p = tuning.plan}
      {@const pp = p.plan}
      {@const sel = tuning.sel >= 0 ? p.options[tuning.sel] : null}
      <Dialog.Header>
        <Dialog.Title>{t('conn.tune_title', { name: tuning.item.name })}</Dialog.Title>
        <Dialog.Description>{t('conn.tune_desc', { name: tuning.item.name })}</Dialog.Description>
      </Dialog.Header>
      {#if pp}
        <PrepareOptions {pp} options={p.options} bind:selected={tuning.sel} name={tuning.item.name} disabled={busy !== null} />
        {#if pp.suggest_smaller_model}
          <p class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span>{t('conn.tune_smaller')}</span>
            <Button size="sm" variant="outline" onclick={() => { app.requestedTab = 'hub'; tuning = null; }}>{t('conn.tune_open_hub')}</Button>
          </p>
        {/if}
      {/if}
      {#if sel}
        <AdvancedOnly>
          <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 rounded-md border border-border bg-muted/40 p-3 text-xs">
            <dt class="text-muted-foreground">{t('conn.tune_ctx')}</dt>
            <dd class="font-num">{p.from.context_size.toLocaleString()} → <span class="text-foreground">{sel.context_size.toLocaleString()}</span></dd>
            <dt class="text-muted-foreground">Flash Attention</dt>
            <dd class="font-mono">{p.from.flash_attn ? t('conn.tune_on') : t('conn.tune_off')} → <span class="text-foreground">{t('conn.tune_on')}</span></dd>
            <dt class="text-muted-foreground">{t('conn.tune_kv')}</dt>
            <dd class="font-mono">{p.from.cache_type_k || p.from.cache_type_v ? kvText({ cache_type_k: p.from.cache_type_k || 'f16', cache_type_v: p.from.cache_type_v || 'f16' }) : t('kv.detail_auto')} → <span class="text-foreground">{kvText(sel)}</span></dd>
            <dt class="text-muted-foreground">{t('conn.tune_slots')}</dt>
            <dd class="font-num">{p.from.parallel_slots ?? t('conn.tune_slots_auto')} → <span class="text-foreground">{t('conn.tune_slots_agents')}</span></dd>
            {#if sel.gpu_layers !== null && !pp?.cpu_only}
              <dt class="text-muted-foreground">{t('conn.tune_layers')}</dt>
              <dd class="font-num">{p.from.gpu_layers} → <span class="text-foreground">{sel.gpu_layers}</span></dd>
            {/if}
          </dl>
        </AdvancedOnly>
      {/if}
      <p class="flex items-start gap-1.5 text-xs text-muted-foreground">
        <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
        <span>{t('conn.tune_restart_note')}</span>
      </p>
      <Dialog.Footer>
        <Button variant="outline" onclick={() => (tuning = null)} disabled={busy !== null}>{t('conn.mcp_cancel')}</Button>
        <Button onclick={confirmTuning} disabled={busy !== null || tuning.sel < 0}>
          {busy ? t('conn.tune_restarting') : t('conn.tune_confirm')}
        </Button>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<!-- Review before writing -->
<Dialog.Root
  bind:open={
    () => preview !== null,
    (v) => {
      if (!v && !busy) preview = null;
    }
  }
>
  <Dialog.Content class="sm:max-w-2xl">
    {#if preview}
      <Dialog.Header>
        <Dialog.Title>{t('conn.preview_title', { name: preview.item.name })}</Dialog.Title>
        <Dialog.Description>{t('conn.preview_desc')}</Dialog.Description>
      </Dialog.Header>
      <div class="flex max-h-[55vh] min-w-0 flex-col gap-3 overflow-auto">
        {#if preview.choice}
          {@const choice = preview.choice}
          {@const item = preview.item}
          <div class="flex flex-col gap-2 rounded-md border border-border p-3 text-xs">
            <label class="flex flex-col gap-1">
              <span class="font-medium">{t('agent.model_label')}</span>
              {#if chatModels.length > 1}
                <FieldHelp text={t('help.model_choice')}><select
                  class="h-8 rounded-md border border-border bg-background px-2 text-xs"
                  value={choice.alias}
                  onchange={(e) => updateChoice({ alias: e.currentTarget.value })}
                  disabled={busy !== null}
                >
                  {#each chatModels as m (m.alias)}
                    {@const st = m.definition_id ? domain.state(m.definition_id)?.state : undefined}
                    <option value={m.alias}>
                      {m.alias}{st === 'running' ? ` · ${t('agent.model_loaded')}` : ''}{m.tools === 'verified'
                        ? ` · ${t('agent.model_tools_ok')}`
                        : m.tools === 'failed' || m.tools === 'no_template'
                          ? ` · ${t('agent.model_tools_bad')}`
                          : ''}
                    </option>
                  {/each}
                </select></FieldHelp>
              {:else}
                <span class="font-mono">{choice.alias}</span>
              {/if}
            </label>
            {#if choice.reason}
              <p class="text-muted-foreground">{reasonText(item, choice.reason)}</p>
            {/if}
            <AgentModelNotes
              alias={choice.alias}
              models={agentModels}
              toolName={item.name}
              promptTokens={item.id === 'opencode' ? opencodePromptTokens(!!preview.openDesktop && preview.hideExternalSkills) : null}
              checkUrl={app.gatewayRunning || app.isRunning ? app.inferenceUrl : null}
              checkKey={testKey}
              disabled={busy !== null}
              onPick={chatModels.length > 1 ? (a: string) => updateChoice({ alias: a }) : null}
              onChecked={reloadAgentModels}
            />
            <p class="break-words text-muted-foreground">
              {app.gatewayRunning && chatModels.length
                ? t('agent.models_listed', { tool: item.name, count: chatModels.length, list: chatModels.map((m) => m.alias).join(', ') })
                : t('agent.models_direct', { tool: item.name })}
            </p>
            {#if item.id !== 'opendesign'}
              <label class="flex cursor-pointer items-start gap-2">
                <FieldHelp text={t('help.set_default')}><Checkbox checked={choice.setDefault} onCheckedChange={(v) => updateChoice({ setDefault: v === true })} disabled={busy !== null} /></FieldHelp>
                <span class="flex min-w-0 flex-col gap-0.5">
                  <span>{t('agent.default_model', { tool: item.name })}</span>
                  <span class="break-words text-muted-foreground">
                    {item.defaultModel ? t('agent.default_current', { model: item.defaultModel }) : t('agent.default_none')}
                  </span>
                </span>
              </label>
              {#if !choice.setDefault && item.defaultModel && !isHarnessDefault(item.defaultModel)}
                <p class="break-words text-muted-foreground">{t('agent.default_kept', { tool: item.name, model: item.defaultModel })}</p>
              {/if}
            {/if}
          </div>
        {/if}
        {#if preview.openDesktop}
          <p class="text-xs text-muted-foreground">{t('agent.desktop_open_after')}</p>
        {/if}
        {#each preview.changes as change (change.path)}
          {@const diff = lineDiff(change.before, change.after)}
          {@const stats = diffStats(diff)}
          <div class="flex min-w-0 flex-col gap-1.5">
            <div class="flex min-w-0 items-center gap-2">
              <span class="truncate font-mono text-xs text-muted-foreground" title={change.path}>{change.path}</span>
              <Badge
                variant="outline"
                class={cn(
                  'shrink-0',
                  change.before === null ? 'text-success' : stats.added + stats.removed === 0 ? 'text-muted-foreground' : 'text-info'
                )}
              >
                {change.before === null
                  ? t('conn.preview_new')
                  : stats.added + stats.removed === 0
                    ? t('conn.preview_same')
                    : t('conn.preview_modified')}
              </Badge>
            </div>
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
              <span class="font-num text-diff-add">+{stats.added}</span>
              <span class="font-num text-diff-remove">−{stats.removed}</span>
              <span class="text-muted-foreground">{t('conn.preview_legend')}</span>
            </div>
            <div class="max-h-72 overflow-auto rounded-md border border-border bg-muted/40 py-1 font-mono text-xs leading-relaxed">
              {#each diff as line, idx (idx)}
                {#if line.kind === 'skip'}
                  <div class="px-3 py-0.5 text-xs text-muted-foreground select-none">⋯ {t('conn.preview_unchanged', { count: line.count })}</div>
                {:else}
                  <div
                    class={cn(
                      'flex gap-2 px-3 break-all whitespace-pre-wrap',
                      line.kind === 'add' && 'bg-diff-add/15 text-diff-add',
                      line.kind === 'del' && 'bg-diff-remove/15 text-diff-remove line-through decoration-diff-remove/40',
                      line.kind === 'same' && 'text-muted-foreground'
                    )}
                  >
                    <span class="w-3 shrink-0 select-none" aria-hidden="true">{line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' '}</span>
                    <span class="sr-only">{line.kind === 'add' ? t('conn.preview_sr_added') : line.kind === 'del' ? t('conn.preview_sr_removed') : ''}</span>
                    <span class="min-w-0">{line.text || ' '}</span>
                  </div>
                {/if}
              {/each}
            </div>
            {#if change.before !== null}
              <p class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                <span class="shrink-0">{t('conn.preview_backup')}</span>
                <span class="truncate font-mono text-foreground" title={change.path}>{t('intg.backup_pattern', { path: change.path })}</span>
              </p>
            {/if}
          </div>
        {:else}
          <p class="text-xs text-muted-foreground">{t('conn.preview_none')}</p>
        {/each}
      </div>
      <Dialog.Footer>
        <Button variant="outline" onclick={() => (preview = null)} disabled={busy !== null}>{t('conn.mcp_cancel')}</Button>
        <Button onclick={confirmApply} disabled={busy !== null || preview.changes.length === 0}>
          {busy ? t('conn.client_configuring') : t('conn.preview_apply')}
        </Button>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>
