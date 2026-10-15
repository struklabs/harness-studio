<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Activity, Loader } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import type { AppSettings, ServerStatus } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { cn } from '#lib/utils.js';
  import ClientIntegrations from './connections/ClientIntegrations.svelte';
  import EndpointList from './connections/EndpointList.svelte';
  import HarnessMcp from './connections/HarnessMcp.svelte';
  import HarnessLinks from './connections/HarnessLinks.svelte';
  import McpServers from './connections/McpServers.svelte';
  import { fetchErrorText, fetchWithTimeout } from './connections/net';
  import AdvancedPageNotice from './common/AdvancedPageNotice.svelte';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { initialPage, pageLevel } from '#lib/uiLevel.js';
  import SectionSidebar from './common/SectionSidebar.svelte';
  import { panes } from '#lib/stores/panes.svelte.js';
  import { CONNECTION_GROUPS, CONNECTION_PAGES } from './connections/navigation';
  import { resolveConnectionPage, connectionPageFromUrl } from './connections/navigationResolver.js';
  import OllamaEndpointCard from './connections/OllamaEndpointCard.svelte';
  import PortFallbackNote from './connections/PortFallbackNote.svelte';
  import ToolCallingWorkbench from './connections/ToolCallingWorkbench.svelte';

  interface Props {
    settings: AppSettings;
    serverStatus: ServerStatus;
  }

  let { settings, serverStatus }: Props = $props();

  const navPane = panes.connectionsNav;
  let active = $state('endpoints');
  let selected = false;
  let visited = $state<Record<string, boolean>>({ endpoints: true });
  const activePage = $derived(CONNECTION_PAGES.find((p) => p.id === active)!);
  const clientPage = $derived(['config', 'guide', 'sdk'].includes(active));
  const mcpPage = $derived(['servers', 'import', 'status'].includes(active));
  function select(value: string) {
    const id = resolveConnectionPage(value);
    if (!id) return;
    selected = true;
    active = id;
    visited[id] = true;
    try { localStorage.setItem('harness_connections_page', id); } catch { /* unavailable */ }
    if (navPane.floating) navPane.close();
  }
  onMount(() => {
    let saved: string | null = null;
    try { saved = localStorage.getItem('harness_connections_page'); } catch { /* unavailable */ }
    const explicit = connectionPageFromUrl(location.href) ?? app.requestedConnectionPage;
    if (explicit) select(explicit);
    else if (!selected) {
      active = initialPage(CONNECTION_GROUPS, resolveConnectionPage(saved ?? '') ?? 'endpoints', uiLevel.level);
      visited[active] = true;
    }
    const navigate = () => { const id = connectionPageFromUrl(location.href); if (id) select(id); };
    window.addEventListener('hashchange', navigate);
    window.addEventListener('popstate', navigate);
    return () => { window.removeEventListener('hashchange', navigate); window.removeEventListener('popstate', navigate); };
  });
  $effect(() => {
    const requested = app.requestedConnectionPage;
    if (requested) { select(requested); app.requestedConnectionPage = null; }
  });

  let ping = $state<{ loading: boolean; success?: boolean; message?: string } | null>(null);
  let alive = true;
  onDestroy(() => (alive = false));

  let isRunning = $derived(serverStatus === 'Running');

  // Model IDs reported by the running server (/v1/models); used in snippets and tests.
  let serverModels = $state<string[]>([]);
  let modelId = $derived(serverModels[0] ?? 'default');
  /** Context size the running server actually uses (0 = unknown). */
  let serverCtx = $state(0);

  $effect(() => {
    if (!isRunning) {
      serverModels = [];
      serverCtx = 0;
      return;
    }
    const controller = new AbortController();
    fetchWithTimeout(`${app.baseUrl}/models`, { headers: app.authHeaders(), signal: controller.signal }, 5000)
      .then((r) => (r.ok ? r.json() : null))
      .then((data) => {
        const list: { id: string; meta?: { n_ctx?: number } }[] = data?.data ?? [];
        serverModels = list.map((m) => m.id).filter(Boolean);
        serverCtx = list[0]?.meta?.n_ctx ?? 0;
      })
      .catch(() => {});
    return () => controller.abort();
  });

  async function testHealthPing() {
    if (ping?.loading) return;
    ping = { loading: true };
    const requestUrl = `${app.hostUrl}/health`;
    const startTime = performance.now();
    try {
      const res = await fetchWithTimeout(requestUrl, { method: 'GET' }, 5000);
      const elapsed = Math.round(performance.now() - startTime);
      if (!alive) return;
      if (res.ok) {
        const data = await res.json().catch(() => ({ status: 'ok' }));
        if (!alive) return;
        ping = { loading: false, success: true, message: t('conn.ping_success', { elapsed, status: data.status || 'OK' }) };
      } else {
        ping = { loading: false, success: false, message: t('conn.ping_bad_status', { status: res.status }) };
      }
    } catch (e) {
      if (!alive) return;
      ping = {
        loading: false,
        success: false,
        message: isRunning ? t('conn.ping_conn_failed', { message: fetchErrorText(e, requestUrl) }) : t('conn.ping_not_started'),
      };
    }
  }
</script>

<div class="flex h-full min-h-0 w-full overflow-hidden bg-background">
  <SectionSidebar pane={navPane} label={t('conn.nav_label')} groups={CONNECTION_GROUPS} {active} onSelect={select} />
  <main class="min-w-0 flex-1 overflow-y-auto">
    <div class="flex w-full max-w-5xl flex-col gap-6 px-6 py-6 max-md:px-4">
      <AdvancedPageNotice level={pageLevel(CONNECTION_GROUPS, active)} />
      <header class="flex flex-col gap-1">
        <p class="text-xs text-muted-foreground">{t('nav.connections')} · {t(CONNECTION_GROUPS.find((g) => g.pages.some((p) => p.id === active))!.label!)}</p>
        <h1 class="text-lg font-semibold">{t(activePage.label)}</h1>
      </header>
      <div hidden={active !== 'endpoints'}>{#if visited.endpoints}<EndpointList />{/if}</div>
      <div hidden={active !== 'ollama'}>{#if visited.ollama}<OllamaEndpointCard />{/if}</div>
      <div hidden={active !== 'ports'}>
        {#if visited.ports}
          <p class="text-xs text-muted-foreground">{t('conn.nav_ports_desc')}</p>
          {#if app.portPending}
            <p class="mt-3 text-xs text-muted-foreground">{t('conn.ep_port_auto')}</p>
          {:else}
            <p class="mt-3 font-mono text-xs">{app.hostUrl}</p>
          {/if}
          {#if app.gateway}
            <p class="mt-3 font-mono text-xs">{app.gateway.status.endpoint?.base_url ?? ''}</p>
            <PortFallbackNote status={app.gateway.status} class="mt-3" />
            {#if !app.gateway.status.port_fallback}<p class="mt-3 text-xs text-muted-foreground">{t('conn.nav_ports_ok')}</p>{/if}
          {/if}
        {/if}
      </div>
      <div hidden={!clientPage}>
        {#if visited.config || visited.guide || visited.sdk}
          <ClientIntegrations {modelId} contextSize={serverCtx || settings.server.default_ctx || 32768} page={active === 'guide' ? 'guide' : active === 'sdk' ? 'sdk' : 'config'} />
        {/if}
      </div>
      <div hidden={active !== 'harness-link'}>{#if visited['harness-link']}<HarnessLinks />{/if}</div>
      <div hidden={active !== 'harness-mcp'}>{#if visited['harness-mcp']}<HarnessMcp />{/if}</div>
      <div hidden={!mcpPage}>
        {#if visited.servers || visited.import || visited.status}<McpServers onNavigate={select} page={active === 'import' ? 'import' : active === 'status' ? 'status' : 'servers'} />{/if}
      </div>
      <div hidden={active !== 'workbench'}>{#if visited.workbench}<ToolCallingWorkbench {modelId} />{/if}</div>
      <div hidden={active !== 'health'}>
  <!-- Server status + health ping -->
  <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 rounded-lg border border-border bg-card px-3 py-2.5">
    <div class="flex min-w-0 items-center gap-2.5">
      <span class={cn('size-2 shrink-0 rounded-full', isRunning ? 'bg-success' : 'bg-muted-foreground/60')} aria-hidden="true"></span>
      <div class="flex min-w-0 flex-col">
        <span class="truncate text-sm font-medium">
          {app.serverLabel(app.selectedServerId)} · {isRunning ? t('conn.server_active_title') : t('conn.server_stopped_title')}
        </span>
        <span class="truncate text-xs text-muted-foreground">
          {#if isRunning}
            {t('conn.server_listening_desc')} <span class="font-mono text-foreground">{app.hostUrl}</span>
          {:else}
            {t('conn.server_stopped_desc')}
          {/if}
        </span>
      </div>
    </div>
    <!-- The server is picked once, in the title bar; this row only reports and tests it. -->
    <div class="flex items-center gap-3">
      {#if ping && !ping.loading}
        <span class={cn('text-xs', ping.success ? 'text-success' : 'text-destructive')} role="status">{ping.message}</span>
      {/if}
      <Button variant="outline" size="sm" onclick={testHealthPing} disabled={ping?.loading}>
        {#if ping?.loading}<Loader class="animate-spin" />{:else}<Activity />{/if}
        {ping?.loading ? t('conn.testing_ellipsis') : t('conn.test_health')}
      </Button>
    </div>
  </div>

      </div>
    </div>
  </main>
</div>
