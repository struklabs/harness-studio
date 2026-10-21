<script lang="ts">
  import { fixed } from '#lib/format.js';
  let { page = 'gateway-endpoint' }: { page?: string } = $props();
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Settings > Gateway: the managed endpoint (harness-gateway, off by default), its scheduler
  // limits, the models it serves and the explicit "ignore this tool" consent per model.
  // Saved through its own command (validated in the core), not the general settings form.
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { Copy, Pin, ShieldAlert, Trash2 } from '@lucide/svelte';
  import type {
    GatewayOverview,
    GatewayServerTools,
    GatewaySettings,
    ModelDefinition,
    QueueHint,
    SlotState,
  } from '#lib/types.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { windowActive } from '#lib/windowActivity.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import NativeSelect from '#lib/components/dashboard/parts/NativeSelect.svelte';
  import { cn } from '#lib/utils.js';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import NumberField from './NumberField.svelte';
  import OllamaSettings from './OllamaSettings.svelte';
  import PortFallbackNote from '#lib/components/connections/PortFallbackNote.svelte';
  import ResponsesSettings from './ResponsesSettings.svelte';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';

  let overview = $state<GatewayOverview | null>(null);
  let draft = $state<GatewaySettings | null>(null);
  let corsText = $state('');
  // A number input binds a number (or undefined when empty), never a string.
  let ttl = $state<number | null | undefined>(null);
  let busy = $state(false);
  let applyError = $state<string | null>(null);
  let ignoreDef = $state('');
  let ignoreTools = $state('');
  let serverDef = $state('');
  let serverWebSearch = $state(false);
  let serverToolNames = $state('');
  let timer: ReturnType<typeof setInterval> | undefined;

  const STATE_LABEL: Record<SlotState, TKey> = {
    unloaded: 'gw.state_unloaded',
    loading: 'gw.state_loading',
    ready: 'gw.state_ready',
    draining: 'gw.state_draining',
  };

  /** The week's queue history asks for more slots for this definition. */
  function queueHint(definitionId: string): QueueHint | undefined {
    return overview?.queue_hints?.find((h) => h.definition_id === definitionId && h.suggested_slots);
  }

  /** Saves the suggested slot count in the definition; it applies at its next load (a running
   * server is not touched, so no running generation is interrupted). */
  async function applyQueueHint(h: QueueHint) {
    if (!h.suggested_slots || busy) return;
    busy = true;
    try {
      const full = await invoke<ModelDefinition>('get_model_definition', { id: h.definition_id });
      await invoke('save_model_definition', {
        definition: { ...full, load: { ...(full.load ?? {}), parallel_slots: h.suggested_slots } },
      });
      await domain.refreshDefinitions();
      toast.success(t('gw.queue_applied', { alias: h.alias, n: h.suggested_slots }));
    } catch (e) {
      applyError = localizeError(e);
    } finally {
      busy = false;
    }
  }

  function adopt(o: GatewayOverview, keepDraft = false) {
    overview = o;
    app.setGateway(o);
    if (keepDraft && draft) return;
    draft = structuredClone($state.snapshot(o.settings) as GatewaySettings);
    corsText = o.settings.cors_origins.join('\n');
    ttl = o.settings.default_ttl_secs ?? null;
  }

  async function refresh(keepDraft = false) {
    try {
      adopt(await invoke<GatewayOverview>('get_gateway_status'), keepDraft);
    } catch (e) {
      applyError = localizeError(e);
    }
  }

  onMount(() => {
    void refresh();
    void domain.refreshDefinitions();
    // The model table follows loads, TTL unloads and evictions.
    timer = setInterval(() => windowActive() && void refresh(true), 2000);
  });
  onDestroy(() => clearInterval(timer));

  function corsLines(): string[] {
    return corsText
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean);
  }

  function ttlValue(): number | null {
    return ttl != null && Number.isFinite(ttl) && ttl > 0 ? Math.round(ttl) : null;
  }

  function current(): GatewaySettings | null {
    if (!draft) return null;
    return { ...draft, cors_origins: corsLines(), default_ttl_secs: ttlValue() };
  }

  let dirty = $derived.by(() => {
    if (!overview || !draft) return false;
    void corsText;
    void ttl;
    const cur = current();
    return JSON.stringify({ ...cur, enabled: false, tool_ignores: {}, server_tools: {}, text_tool_calls: {} }) !==
      JSON.stringify({ ...overview.settings, enabled: false, tool_ignores: {}, server_tools: {}, text_tool_calls: {} });
  });

  async function apply(next: GatewaySettings) {
    busy = true;
    applyError = null;
    try {
      adopt(await invoke<GatewayOverview>('set_gateway_settings', { settings: next }));
      toast.success(t('gw.applied'));
    } catch (e) {
      applyError = localizeError(e);
    } finally {
      busy = false;
    }
  }

  function toggleEnabled(on: boolean) {
    const cur = current();
    if (cur) apply({ ...cur, enabled: on });
  }

  function applyDraft() {
    const cur = current();
    if (cur && overview) {
      apply({
        ...cur,
        enabled: overview.settings.enabled,
        tool_ignores: overview.settings.tool_ignores,
        server_tools: overview.settings.server_tools ?? {},
        text_tool_calls: overview.settings.text_tool_calls ?? {},
      });
    }
  }

  async function copy(text: string | null | undefined) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t('gw.copied'));
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function unload(definitionId: string) {
    try {
      adopt(await invoke<GatewayOverview>('unload_gateway_model', { definitionId }), true);
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  function toggleTextCalls(defId: string, on: boolean) {
    if (!overview) return;
    const next = { ...(overview.settings.text_tool_calls ?? {}) };
    // On by default: only the opt-out is stored.
    if (on) delete next[defId];
    else next[defId] = false;
    void apply({ ...overview.settings, text_tool_calls: next });
  }

  function aliasOf(id: string) {
    return domain.definition(id)?.alias ?? id;
  }

  function saveIgnores(next: Record<string, string[]>) {
    if (!overview) return;
    apply({ ...overview.settings, tool_ignores: next });
  }

  function addIgnore() {
    if (!overview || !ignoreDef) return;
    const tools = ignoreTools
      .split(/[,\s]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (!tools.length) return;
    const next = structuredClone($state.snapshot(overview.settings.tool_ignores) as Record<string, string[]>);
    next[ignoreDef] = [...new Set([...(next[ignoreDef] ?? []), ...tools])];
    ignoreTools = '';
    saveIgnores(next);
  }

  function removeIgnore(defId: string) {
    if (!overview) return;
    const next = structuredClone($state.snapshot(overview.settings.tool_ignores) as Record<string, string[]>);
    delete next[defId];
    saveIgnores(next);
  }

  // Server-side tools (T1c): saved at once, like the ignores.
  function saveServerTools(next: Record<string, GatewayServerTools>) {
    if (!overview) return;
    apply({ ...overview.settings, server_tools: next });
  }

  function addServerTools() {
    if (!overview || !serverDef) return;
    const tools = serverToolNames
      .split(/[,\s]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (!tools.length && !serverWebSearch) return;
    const next = structuredClone($state.snapshot(overview.settings.server_tools ?? {}) as Record<string, GatewayServerTools>);
    next[serverDef] = { web_search: serverWebSearch, tools: [...new Set(tools)] };
    serverToolNames = '';
    serverWebSearch = false;
    saveServerTools(next);
  }

  function removeServerTools(defId: string) {
    if (!overview) return;
    const next = structuredClone($state.snapshot(overview.settings.server_tools ?? {}) as Record<string, GatewayServerTools>);
    delete next[defId];
    saveServerTools(next);
  }
</script>

<div class="flex flex-col gap-6">
  {#if overview && !overview.owner}
    <div class="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/5 p-3 text-xs" role="status">
      <ShieldAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
      <p>{t('gw.not_owner')}</p>
    </div>
  {/if}

<div hidden={page !== 'gateway-endpoint'} class="flex flex-col gap-3">
  <SettingGroup title={t('gw.title')} hint={t('gw.desc')}>
    {#snippet actions()}
      {#if overview?.status.running}
        <Badge variant="outline" class="text-success">{t('gw.status_running')}</Badge>
      {:else}
        <Badge variant="outline">{t('gw.status_stopped')}</Badge>
      {/if}
    {/snippet}
    {#if draft && overview}
      <SettingRow label={t('gw.enable')} hint={t('gw.enable_hint')} for="gw-enabled">
        <Switch
          id="gw-enabled"
          disabled={busy || !overview.owner}
          bind:checked={() => overview?.settings.enabled ?? false, (v) => toggleEnabled(v)}
        />
      </SettingRow>
      {#if overview.status.running && overview.status.endpoint}
        <SettingRow label={t('gw.base_url')}>
          <code class="truncate font-mono text-xs">{overview.status.endpoint.base_url}</code>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('gw.copy')}
            title={t('gw.copy')}
            onclick={() => copy(overview?.status.endpoint?.base_url)}
          >
            <Copy />
          </Button>
        </SettingRow>
      {/if}
      <SettingRow help={t('help.gateway_port')} label={t('gw.port')} for="gw-port">
        <NumberField id="gw-port" value={draft.port} min={1024} max={65535} onchange={(v) => draft && (draft.port = v)} />
      </SettingRow>
      {#if overview.status.running && overview.status.port_fallback}
        <PortFallbackNote status={overview.status} class="px-4 py-2" />
      {/if}
      <SettingRow label={t('gw.host')} hint={t('gw.host_hint')} for="gw-host">
        <Input id="gw-host" class="h-8 w-40 font-mono text-xs" spellcheck={false} autocomplete="off" bind:value={draft.host} />
      </SettingRow>
      <SettingRow label={t('gw.allow_network')} note={t('gw.allow_network_hint')} help={t('help.bind_host')} for="gw-lan">
        <Switch id="gw-lan" bind:checked={draft.allow_network} />
      </SettingRow>
      <SettingRow label={t('gw.require_token')} note={t('gw.require_token_hint')} help={t('gw.require_token_help')} for="gw-token">
        <Switch id="gw-token" bind:checked={draft.require_token} />
      </SettingRow>
      <SettingRow label={t('gw.cors')} hint={t('gw.cors_hint')} help={t('help.cors')} for="gw-cors">
        <Textarea
          id="gw-cors"
          rows={2}
          class="font-mono text-xs"
          spellcheck={false}
          placeholder="http://localhost:5173"
          bind:value={corsText}
        />
      </SettingRow>
      {#if applyError || overview.status.error}
        <p class="px-4 py-2 text-xs text-destructive" role="alert">
          {applyError ?? t('gw.status_error', { error: localizeError(overview.status.error) })}
        </p>
      {/if}
    {:else if applyError}
      <p class="px-4 py-3 text-xs text-destructive" role="alert">{applyError}</p>
    {:else}
      <SectionLoading shape="rows" rows={3} class="m-3" label={t('sl.settings_label')} phases={[t('sl.settings_1'), t('sl.settings_2')]} />
    {/if}
  </SettingGroup>

</div>
<div hidden={page !== 'runtime-ollama'} class="flex flex-col gap-3">
  <OllamaSettings {overview} {busy} onapply={(next) => apply(next)} />

</div>
<div hidden={page !== 'gateway-responses'} class="flex flex-col gap-3">
  <ResponsesSettings {overview} {busy} onapply={(next) => apply(next)} onadopt={(o) => adopt(o, true)} />

</div>
<div hidden={page !== 'gateway-routing'} class="flex flex-col gap-6">  <SettingGroup title={t('gw.limits_title')} note={t('gw.limits_desc')}>
    {#if draft}
      <SettingRow label={t('gw.max_loaded')} hint={t('gw.max_loaded_hint')} for="gw-max-loaded">
        <NumberField id="gw-max-loaded" value={draft.max_loaded_models} min={0} onchange={(v) => draft && (draft.max_loaded_models = v)} />
      </SettingRow>
      <SettingRow label={t('gw.max_queue')} hint={t('gw.max_queue_hint')} for="gw-queue">
        <NumberField id="gw-queue" value={draft.max_queue} min={1} onchange={(v) => draft && (draft.max_queue = v)} />
      </SettingRow>
      <SettingRow label={t('gw.timeout')} hint={t('gw.timeout_hint')} for="gw-timeout">
        <NumberField
          id="gw-timeout"
          value={draft.request_timeout_secs}
          min={0}
          onchange={(v) => draft && (draft.request_timeout_secs = v)}
        />
      </SettingRow>
      <SettingRow label={t('gw.default_ttl')} hint={t('gw.default_ttl_hint')} for="gw-ttl">
        <Input id="gw-ttl" type="number" min={1} class="h-8 w-28 text-right font-num" bind:value={ttl} />
      </SettingRow>
      <SettingRow label={t('gw.memory_aware')} note={t('gw.memory_aware_hint')} help={t('gw.memory_aware_help')} for="gw-mem">
        <Switch id="gw-mem" bind:checked={draft.memory_aware} />
      </SettingRow>
      <SettingRow label={t('gw.decision_emulation')} note={t('gw.decision_emulation_hint')} for="gw-decisions">
        <Switch id="gw-decisions" bind:checked={draft.decision_emulation} />
      </SettingRow>
      <SettingRow label={t('gw.headroom')} hint={t('gw.headroom_hint')} for="gw-headroom">
        <NumberField
          id="gw-headroom"
          value={draft.memory_headroom_mib}
          min={0}
          onchange={(v) => draft && (draft.memory_headroom_mib = v)}
        />
      </SettingRow>
    {/if}

  </SettingGroup>

  <SettingGroup title={t('gw.models_title')} hint={t('gw.models_desc')}>
    {#each overview?.models ?? [] as m (m.definition_id)}
      <SettingRow label={m.alias}>
        {#snippet extra()}
          <div class="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
            <Badge variant="outline" class={cn(m.state === 'ready' && 'text-success', m.state === 'loading' && 'text-warning')}>
              {t(STATE_LABEL[m.state])}
            </Badge>
            <Badge variant="outline" title={m.managed ? undefined : t('gw.external_hint')}>
              {m.managed ? t('gw.managed') : t('gw.external')}
            </Badge>
            {#if m.pinned}<Badge variant="outline"><Pin class="size-3" />{t('gw.pinned')}</Badge>{/if}
            {#if m.ttl_secs}<Badge variant="outline">{t('gw.ttl_value', { secs: m.ttl_secs })}</Badge>{/if}
            {#if m.server_id}<span class="font-mono">{m.server_id}{m.port ? `:${m.port}` : ''}</span>{/if}
            <span>{t('gw.activity', { running: m.in_flight, queued: m.queued, slots: m.concurrency })}</span>
            {#if (m.token_pool ?? 0) > 0}
              <span class="font-num">{t('gw.tokens_budget', { used: m.tokens_in_use ?? 0, pool: m.token_pool ?? 0 })}</span>
            {/if}
            {#if m.idle_secs !== null && m.state === 'ready'}<span>{t('gw.idle', { secs: m.idle_secs })}</span>{/if}
          </div>
          {#if (m.waited ?? 0) > 0 && m.concurrency < 4}
            <p class="mt-1 text-xs leading-snug text-warning" role="status">{t('gw.waited_hint', { n: m.waited ?? 0 })}</p>
          {/if}
          {#if m.adapted && m.state !== 'unloaded'}
            <p class="mt-1 text-xs leading-snug text-warning" role="status">{t('gw.adapted_hint', { k: m.adapted.cache_type_k, v: m.adapted.cache_type_v, ctx: m.adapted.context_size, requested: m.adapted.requested_context })}</p>
          {/if}
          {#if (m.waited_tokens ?? 0) > 0}
            <p class="mt-1 text-xs leading-snug text-muted-foreground" role="status">{t('gw.waited_tokens_hint', { n: m.waited_tokens ?? 0 })}</p>
          {/if}
          {#each [queueHint(m.definition_id)] as h (h?.definition_id)}
            {#if h}
              <div class="mt-1 flex flex-wrap items-center gap-2 text-xs leading-snug text-warning" role="status">
                <span>
                  {t('gw.queue_hint', {
                    requests: h.requests,
                    share: Math.round(h.wait_share * 100),
                    secs: fixed(h.avg_wait_ms / 1000, 1),
                    n: h.suggested_slots ?? 0,
                  })}
                </span>
                <Button variant="outline" size="sm" disabled={busy} onclick={() => applyQueueHint(h)}>
                  {t('gw.queue_apply', { n: h.suggested_slots ?? 0 })}
                </Button>
              </div>
            {/if}
          {/each}
        {/snippet}
        {#if m.state === 'ready' || m.state === 'loading'}
          <Button variant="outline" size="sm" onclick={() => unload(m.definition_id)}>{t('gw.unload')}</Button>
        {/if}
      </SettingRow>
    {:else}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('gw.models_none')}</p>
    {/each}
  </SettingGroup>

  <SettingGroup title={t('gw.ignores_title')} hint={t('gw.ignores_desc')}>
    {#each Object.entries(overview?.settings.tool_ignores ?? {}) as [defId, tools] (defId)}
      <SettingRow label={aliasOf(defId)}>
        {#snippet extra()}
          <div class="flex flex-wrap gap-1">
            {#each tools as tool (tool)}<Badge variant="outline" class="font-mono">{tool}</Badge>{/each}
          </div>
        {/snippet}
        <Button variant="outline" size="sm" disabled={busy} onclick={() => removeIgnore(defId)}>
          <Trash2 />
          {t('gw.ignores_remove')}
        </Button>
      </SettingRow>
    {:else}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('gw.ignores_none')}</p>
    {/each}
    <div class="flex flex-wrap items-end gap-2 px-4 py-3">
      <div class="flex flex-col gap-1">
        <label for="gw-ignore-def" class="text-xs text-muted-foreground">{t('gw.ignores_model')}</label>
        <FieldHelp text={t('help.ignore_model')}><NativeSelect id="gw-ignore-def" class="h-8 w-48 text-xs" bind:value={ignoreDef}>
          <option value="">—</option>
          {#each domain.definitions as d (d.id)}<option value={d.id}>{d.alias}</option>{/each}
        </NativeSelect></FieldHelp>
      </div>
      <div class="flex min-w-48 flex-1 flex-col gap-1">
        <label for="gw-ignore-tools" class="text-xs text-muted-foreground">{t('gw.ignores_tools')}</label>
        <FieldHelp text={t('gw.ignores_desc')}><Input
          id="gw-ignore-tools"
          class="h-8 font-mono text-xs"
          spellcheck={false}
          placeholder={t('gw.ignores_placeholder')}
          bind:value={ignoreTools}
        /></FieldHelp>
      </div>
      <Button size="sm" variant="outline" disabled={busy || !ignoreDef || !ignoreTools.trim()} onclick={addIgnore}>
        {t('gw.ignores_add')}
      </Button>
    </div>
  </SettingGroup>

  <SettingGroup title={t('gw.text_calls_title')} >
    {#each domain.definitions as d (d.id)}
      <SettingRow label={d.alias} help={t('gw.text_calls_desc')}>
        <Switch checked={overview?.settings.text_tool_calls?.[d.id] ?? true}
          disabled={busy || !overview} aria-label={t('gw.text_calls_model', { alias: d.alias })}
          onCheckedChange={(on) => toggleTextCalls(d.id, on)} />
      </SettingRow>
    {:else}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('gw.models_none')}</p>
    {/each}
  </SettingGroup>

  <SettingGroup title={t('gw.server_tools_title')} hint={t('gw.server_tools_desc')}>
    {#each Object.entries(overview?.settings.server_tools ?? {}) as [defId, st] (defId)}
      <SettingRow label={aliasOf(defId)}>
        {#snippet extra()}
          <div class="flex flex-wrap gap-1">
            {#if st.web_search}<Badge variant="secondary" class="font-mono">{t('gw.server_tools_hosted')}</Badge>{/if}
            {#each st.tools as tool (tool)}<Badge variant="outline" class="font-mono">{tool}</Badge>{/each}
          </div>
        {/snippet}
        <Button variant="outline" size="sm" disabled={busy} onclick={() => removeServerTools(defId)}>
          <Trash2 />
          {t('gw.server_tools_remove')}
        </Button>
      </SettingRow>
    {:else}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('gw.server_tools_none')}</p>
    {/each}
    <div class="flex flex-wrap items-end gap-2 px-4 py-3">
      <div class="flex flex-col gap-1">
        <label for="gw-st-def" class="text-xs text-muted-foreground">{t('gw.ignores_model')}</label>
        <FieldHelp text={t('help.server_tool_model')}><NativeSelect id="gw-st-def" class="h-8 w-48 text-xs" bind:value={serverDef}>
          <option value="">—</option>
          {#each domain.definitions as d (d.id)}<option value={d.id}>{d.alias}</option>{/each}
        </NativeSelect></FieldHelp>
      </div>
      <div class="flex min-w-48 flex-1 flex-col gap-1">
        <label for="gw-st-tools" class="text-xs text-muted-foreground">{t('gw.server_tools_names')}</label>
        <FieldHelp text={t('gw.server_tools_desc')}><Input
          id="gw-st-tools"
          class="h-8 font-mono text-xs"
          spellcheck={false}
          placeholder={t('gw.server_tools_placeholder')}
          bind:value={serverToolNames}
        /></FieldHelp>
      </div>
      <div class="flex items-center gap-2 pb-1.5">
        <FieldHelp text={t('help.hosted_web')}><Switch id="gw-st-web" bind:checked={serverWebSearch} /></FieldHelp>
        <label for="gw-st-web" class="text-xs">{t('gw.server_tools_web')}</label>
      </div>
      <Button
        size="sm"
        variant="outline"
        disabled={busy || !serverDef || (!serverToolNames.trim() && !serverWebSearch)}
        onclick={addServerTools}
      >
        {t('gw.ignores_add')}
      </Button>
    </div>
  </SettingGroup>
</div>
    {#if dirty}
      <div class="flex justify-end px-4 py-2">
        <Button size="sm" disabled={busy} onclick={applyDraft}>{t('gw.apply')}</Button>
      </div>
    {/if}
</div>
