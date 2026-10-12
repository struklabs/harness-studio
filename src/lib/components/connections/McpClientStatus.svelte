<script lang="ts">
  import HelpPopover from '../common/HelpPopover.svelte';

  // Harness's own MCP client (T3): the state of each configured server (started on first use) and,
  // per tool, the permission class: from the server's annotations unless the user sets one.
  import { onDestroy, onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { ChevronDown, ChevronRight, KeyRound, LogOut, RefreshCw, RotateCcw, ShieldAlert } from '@lucide/svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import {
    authorizeMcpServer,
    cancelMcpAuthorize,
    logoutMcpServer,
    getMcpStatus,
    getToolSettings,
    listCoreTools,
    setMcpToolClass,
    type CoreTool,
    type McpServerState,
    type McpServerStatus,
    type PermissionClass,
  } from '$lib/tools-client';
  import { cn } from '$lib/utils';
  import NativeSelect from '../dashboard/parts/NativeSelect.svelte';
  import McpResourceList from './McpResourceList.svelte';

  interface Props {
    /** Changes whenever the saved server list changed: the state is read again. */
    reloadKey?: number;
  }

  let { reloadKey = 0 }: Props = $props();

  const CLASSES: PermissionClass[] = ['safe', 'restricted', 'dangerous'];
  const classLabel = (c: PermissionClass) => t(`tools.class_${c}` as TKey);
  const stateKey: Record<McpServerState, TKey> = {
    connected: 'conn.mcpc_state_connected',
    failed: 'conn.mcpc_state_failed',
    idle: 'conn.mcpc_state_idle',
    connecting: 'conn.mcpc_state_connecting',
    disabled: 'conn.mcpc_state_disabled',
  };

  let servers = $state<McpServerStatus[]>([]);
  let tools = $state<CoreTool[]>([]);
  /** The user's own classes (`settings.tools.mcp_classes`). */
  let overrides = $state<Record<string, PermissionClass>>({});
  let loading = $state(false);
  let error = $state('');
  let open = $state<Record<string, boolean>>({});
  let resOpen = $state<Record<string, boolean>>({});
  /** The server whose browser sign-in is waiting for the user. */
  let authorizing = $state<string | null>(null);
  let authError = $state<Record<string, string>>({});
  let alive = true;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let polls = 0;
  let refreshPending = false;

  onDestroy(() => {
    alive = false;
    if (timer) clearTimeout(timer);
  });

  async function load(retry = false) {
    if (loading) {
      refreshPending = true;
      return;
    }
    loading = true;
    error = '';
    try {
      const [st, listing, cfg] = await Promise.all([getMcpStatus(retry), listCoreTools(), getToolSettings()]);
      if (!alive) return;
      servers = st;
      tools = listing.tools.filter((tl) => tl.source === 'mcp');
      overrides = { ...(cfg.mcp_classes ?? {}) };
      // A first `npx` start can take a while: look again until nothing is starting.
      if (timer) clearTimeout(timer);
      timer = null;
      if (st.some((s) => s.state === 'connecting') && polls++ < 20) {
        timer = setTimeout(() => void load(false), 3000);
      } else {
        polls = 0;
      }
    } catch (e) {
      if (alive) error = e instanceof Error ? e.message : String(e);
    } finally {
      if (alive) {
        loading = false;
        if (refreshPending) {
          refreshPending = false;
          void load();
        }
      }
    }
  }

  onMount(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void listen('tools-changed', () => {
      if (alive) void load();
    }).then(unlisten => {
      if (disposed) unlisten();
      else cleanup = unlisten;
    }).catch(() => {});
    void load();
    return () => {
      disposed = true;
      cleanup?.();
    };
  });
  $effect(() => {
    void reloadKey;
    if (reloadKey > 0) void load();
  });

  const toolsOf = (server: string) => tools.filter((tl) => tl.server === server);

  /** Opens the browser for the server's OAuth sign-in and waits for the approval. */
  async function authorize(name: string) {
    if (authorizing) return;
    authorizing = name;
    authError = { ...authError, [name]: '' };
    try {
      servers = await authorizeMcpServer(name);
      if (alive) void load();
    } catch (e) {
      if (alive) authError = { ...authError, [name]: localizeError(e) };
    } finally {
      if (alive) authorizing = null;
    }
  }

  async function signOut(name: string) {
    try {
      servers = await logoutMcpServer(name);
      if (alive) void load();
    } catch (e) {
      if (alive) authError = { ...authError, [name]: localizeError(e) };
    }
  }

  /** The class the server's own hints give a tool (what it has when the user sets none). */
  const serverClass = (tl: CoreTool): PermissionClass => tl.default_class ?? tl.class;

  async function setClass(tl: CoreTool, value: string) {
    try {
      const cls = value === '' ? null : (value as PermissionClass);
      const saved = await setMcpToolClass(tl.name, cls);
      overrides = { ...(saved.mcp_classes ?? {}) };
      const listing = await listCoreTools();
      if (alive) tools = listing.tools.filter((x) => x.source === 'mcp');
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
  const existingHelpId = $props.id();
</script>

  <section class="flex min-w-0 flex-col gap-2" aria-labelledby="mcpc-title">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h3 id="mcpc-title" class="text-sm font-medium">{t('conn.mcpc_title')}<HelpPopover text={t('conn.mcpc_desc')} /></h3>
      <Button variant="ghost" size="sm" disabled={loading} onclick={() => load(true)}>
        <RefreshCw class={cn(loading && 'animate-spin')} /> {t('conn.mcpc_refresh')}
      </Button>
    </div>

    {#if error}
      <p class="text-xs text-destructive" role="alert">{t('conn.mcpc_load_failed', { message: error })}</p>
    {/if}

    {#if !loading && !error && servers.length === 0}<p class="text-xs text-muted-foreground">{t('conn.nav_no_mcp')}</p>{/if}
    <ul class="flex flex-col divide-y divide-border overflow-hidden rounded-lg border border-border bg-card">
      {#each servers as s (s.name)}
        {@const list = toolsOf(s.name)}
        <li class="flex min-w-0 flex-col gap-2 p-3">
          <div class="flex min-w-0 flex-wrap items-center gap-1.5">
            <span class="font-mono text-sm font-medium">{s.name}</span>
            <Badge
              variant="outline"
              class={cn(s.state === 'connected' && 'text-success', s.state === 'failed' && 'text-destructive', 'text-muted-foreground')}
            >
              {t(stateKey[s.state])}
            </Badge>
            {#if s.transport === 'sse'}
              <Badge variant="outline" class="text-muted-foreground">{t('conn.mcpc_transport_sse')}</Badge>
            {/if}
            {#if s.auth === 'required'}
              <Badge variant="outline" class="text-warning">{t('conn.mcpc_auth_required')}</Badge>
            {:else if s.auth === 'authorized'}
              <Badge variant="outline" class="text-success">{t('conn.mcpc_auth_authorized')}</Badge>
            {/if}
            {#if s.state === 'connected'}
              <Badge variant="outline" class="font-num text-muted-foreground">{t('conn.mcpc_tools', { n: s.tools })}</Badge>
              {#if s.protocol}
                <Badge variant="outline" class="font-num text-muted-foreground">{t('conn.mcpc_protocol', { version: s.protocol })}</Badge>
              {/if}
              {#if s.server}
                <span class="min-w-0 truncate text-xs text-muted-foreground" title={s.server}>{s.server}</span>
              {/if}
            {/if}
            <span class="flex-1"></span>
            {#if authorizing === s.name}
              <span class="text-xs text-muted-foreground" role="status">{t('conn.mcpc_authorizing')}</span>
              <Button variant="ghost" size="xs" onclick={() => void cancelMcpAuthorize(s.name)}>{t('conn.mcpc_authorize_cancel')}</Button>
            {:else if s.auth === 'required'}
              <Button size="xs" disabled={authorizing !== null} onclick={() => authorize(s.name)}>
                <KeyRound /> {t('conn.mcpc_authorize')}
              </Button>
            {:else if s.auth === 'authorized'}
              <Button variant="ghost" size="xs" onclick={() => signOut(s.name)}>
                <LogOut /> {t('conn.mcpc_signout')}
              </Button>
            {/if}
            {#if s.state === 'failed' && s.auth !== 'required'}
              <Button variant="outline" size="xs" disabled={loading} onclick={() => load(true)}>{t('conn.mcpc_retry')}</Button>
            {/if}
            {#if s.state === 'connected' && s.resources}
              <Button variant="ghost" size="xs" onclick={() => (resOpen[s.name] = !resOpen[s.name])} aria-expanded={!!resOpen[s.name]}>
                {#if resOpen[s.name]}<ChevronDown />{:else}<ChevronRight />{/if}
                {t('conn.mcpc_show_resources')}
              </Button>
            {/if}
            {#if s.state === 'connected'}
              <Button variant="ghost" size="xs" onclick={() => (open[s.name] = !open[s.name])} aria-expanded={!!open[s.name]}>
                {#if open[s.name]}<ChevronDown />{:else}<ChevronRight />{/if}
                {t('conn.mcpc_show_tools')}
              </Button>
            {/if}
          </div>
          {#if s.auth === 'required' && authorizing !== s.name}
            <p class="text-xs text-muted-foreground">{t('conn.mcpc_authorize_hint')}</p>
          {/if}
          {#if authError[s.name]}
            <p class="text-xs break-words whitespace-pre-line text-destructive" role="alert">{t('conn.mcpc_auth_failed', { message: authError[s.name] })}</p>
          {/if}
          {#if s.state === 'failed' && s.error && s.auth !== 'required'}
            <p class="text-xs break-words whitespace-pre-line text-destructive" role="status">{localizeError(s.error)}</p>
          {/if}
          {#if s.state === 'connected' && s.resources && resOpen[s.name]}
            <McpResourceList server={s.name} />
          {/if}
          {#if s.state === 'connected' && open[s.name]}
            {#if list.length === 0}
              <p class="text-xs text-muted-foreground">{t('conn.mcpc_no_tools')}</p>
            {:else}
              <p id={existingHelpId + '-existing-conn-mcpc_perm_hint'} data-field-help-text class="flex items-start gap-1.5 text-xs text-muted-foreground">
                <ShieldAlert class="mt-0.5 size-3 shrink-0" />
                {t('conn.mcpc_perm_hint')}
              </p>
              <ul class="flex flex-col gap-1">
                {#each list as tl (tl.name)}
                  <li class="flex min-w-0 items-center gap-2">
                    <span class="min-w-0 flex-1 truncate font-mono text-xs" title={tl.description}>{tl.name}</span>
                    <NativeSelect aria-describedby={existingHelpId + '-existing-conn-mcpc_perm_hint'}
                      class="h-7 w-44 text-xs"
                      value={overrides[tl.name] ?? ''}
                      aria-label={t('conn.mcpc_class_aria', { tool: tl.name })}
                      onchange={(e) => setClass(tl, e.currentTarget.value)}
                    >
                      <option value="">{t('conn.mcpc_class_server', { class: classLabel(overrides[tl.name] ? serverClass(tl) : tl.class) })}</option>
                      {#each CLASSES as c (c)}
                        <option value={c}>{classLabel(c)}</option>
                      {/each}
                    </NativeSelect>
                    {#if overrides[tl.name]}
                      <Button variant="ghost" size="icon-sm" aria-label={t('conn.mcpc_reset')} title={t('conn.mcpc_reset')} onclick={() => setClass(tl, '')}>
                        <RotateCcw />
                      </Button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
  </section>
