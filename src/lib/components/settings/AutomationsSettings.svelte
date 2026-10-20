<script lang="ts">
  // Settings > Automations: the user's own n8n instance (its public REST API only; Harness never
  // installs or runs n8n). The API key is saved through its own command into the OS keychain and
  // never comes back. Starting a workflow (webhook trigger) asks for a confirmation first.
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { CircleCheck, Copy, ExternalLink, Play, RefreshCw, Send } from '@lucide/svelte';
  import type {
    CreatedApiToken,
    N8nConnection,
    N8nExecution,
    N8nExecutionError,
    N8nOpenAiConnection,
    N8nTriggerResult,
    N8nView,
    N8nWorkflow,
  } from '#lib/types.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import type { TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';

  let view = $state<N8nView | null>(null);
  let url = $state('');
  /** New key typed by the user; empty = keep the saved key. */
  let apiKey = $state('');
  let busy = $state(false);
  let testing = $state(false);
  let error = $state<string | null>(null);
  let testOk = $state<N8nConnection | null>(null);

  let workflows = $state<N8nWorkflow[]>([]);
  let executions = $state<N8nExecution[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let confirming = $state<string | null>(null);
  let running = $state<string | null>(null);
  let errors = $state<Record<string, N8nExecutionError | 'loading'>>({});

  let openai = $state<N8nOpenAiConnection | null>(null);
  let newToken = $state<string | null>(null);
  let tokenBusy = $state(false);
  let tokenError = $state<string | null>(null);

  let connected = $derived(!!view?.url && view.key_set);
  let dirty = $derived(!!view && (url.trim() !== view.url || apiKey.trim() !== ''));
  let names = $derived(Object.fromEntries(workflows.map((w) => [w.id, w.name])));

  async function loadView() {
    try {
      view = await invoke<N8nView>('n8n_get_settings');
      url = view.url;
    } catch (e) {
      error = localizeError(e);
    }
  }

  async function loadData() {
    if (!connected) return;
    loading = true;
    loadError = null;
    try {
      [workflows, executions] = await Promise.all([
        invoke<N8nWorkflow[]>('n8n_list_workflows'),
        invoke<N8nExecution[]>('n8n_list_executions', { limit: 25 }),
      ]);
      errors = {};
    } catch (e) {
      loadError = localizeError(e);
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    invoke<N8nOpenAiConnection>('n8n_openai_connection')
      .then((o) => (openai = o))
      .catch(() => {});
    await loadView();
    await loadData();
  });

  async function save() {
    busy = true;
    error = null;
    try {
      view = await invoke<N8nView>('n8n_save_settings', { url, apiKey: apiKey.trim() ? apiKey : null });
      url = view.url;
      apiKey = '';
      testOk = null;
      toast.success(t('n8nui.saved'));
      await loadData();
    } catch (e) {
      error = localizeError(e);
    } finally {
      busy = false;
    }
  }

  async function disconnect() {
    busy = true;
    error = null;
    try {
      view = await invoke<N8nView>('n8n_save_settings', { url: '', apiKey: null });
      url = '';
      apiKey = '';
      testOk = null;
      workflows = [];
      executions = [];
      toast.success(t('n8nui.disconnected'));
    } catch (e) {
      error = localizeError(e);
    } finally {
      busy = false;
    }
  }

  async function runTest() {
    testing = true;
    testOk = null;
    error = null;
    try {
      testOk = await invoke<N8nConnection>('n8n_test_connection', {
        url: url.trim() || null,
        apiKey: apiKey.trim() || null,
      });
    } catch (e) {
      error = localizeError(e);
    } finally {
      testing = false;
    }
  }

  async function run(w: N8nWorkflow) {
    running = w.id;
    try {
      const r = await invoke<N8nTriggerResult>('n8n_trigger_workflow', { id: w.id, confirmed: true });
      if (r.ok) toast.success(t('n8nui.run_ok', { name: w.name, status: r.status }));
      else toast.error(t('n8nui.run_failed', { name: w.name, status: r.status }));
      confirming = null;
      invoke<N8nExecution[]>('n8n_list_executions', { limit: 25 })
        .then((e) => (executions = e))
        .catch(() => {});
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      running = null;
    }
  }

  async function showError(ex: N8nExecution) {
    errors[ex.id] = 'loading';
    try {
      errors[ex.id] = await invoke<N8nExecutionError>('n8n_execution_error', { id: ex.id });
    } catch (e) {
      errors[ex.id] = { message: localizeError(e), node: null };
    }
  }

  async function createToken() {
    tokenBusy = true;
    tokenError = null;
    try {
      const created = await invoke<CreatedApiToken>('n8n_create_gateway_token');
      newToken = created.token;
      toast.success(t('n8nui.token_created'));
    } catch (e) {
      tokenError = localizeError(e);
    } finally {
      tokenBusy = false;
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t('n8nui.copied'));
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  const open = (link: string) => openUrl(link).catch((e) => toast.error(localizeError(e)));
  const statusLabel = (s: string) => t(`n8nui.status_${['success', 'error', 'crashed', 'running', 'waiting', 'canceled', 'new'].includes(s) ? s : 'unknown'}` as TKey);
  const statusClass = (s: string) =>
    s === 'success' ? 'text-success' : s === 'error' || s === 'crashed' ? 'text-destructive' : '';
  const fmtTime = (iso: string | null) => (iso ? new Date(iso).toLocaleString(locale()) : '');
  /** A webhook is only a "run" when the workflow is active and the trigger is a plain webhook. */
  const runnable = (w: N8nWorkflow) => w.active && !!w.webhook;
</script>

<div class="flex flex-col gap-6">
  <SettingGroup title={t('n8nui.connection_title')} hint={t('n8nui.connection_desc')}>
    {#snippet actions()}
      <Badge variant="outline" class={connected ? 'text-success' : ''}>
        {connected ? t('n8nui.status_connected') : t('n8nui.status_off')}
      </Badge>
    {/snippet}
    <SettingRow label={t('n8nui.url')} hint={t('n8nui.url_hint')} for="n8n-url">
      <Input
        id="n8n-url"
        class="h-8 font-mono text-xs"
        spellcheck={false}
        autocomplete="off"
        placeholder="https://n8n.example.com"
        bind:value={url}
      />
    </SettingRow>
    <SettingRow label={t('n8nui.key')} hint={t('n8nui.key_hint')} note={view?.key_set ? t('n8nui.key_saved') : undefined} for="n8n-key">
      <Input
        id="n8n-key"
        type="password"
        class="h-8 font-mono text-xs"
        spellcheck={false}
        autocomplete="off"
        placeholder={t('n8nui.key_placeholder')}
        bind:value={apiKey}
      />
    </SettingRow>
    <div class="flex flex-col gap-2 px-4 py-3">
      <div class="flex flex-wrap items-center justify-end gap-2">
        <div class="flex shrink-0 flex-wrap gap-2">
          <Button variant="outline" size="sm" disabled={testing || (!url.trim() && !view?.url)} onclick={runTest}>
            <Send />
            {testing ? t('n8nui.testing') : t('n8nui.test')}
          </Button>
          {#if view?.url}
            <Button variant="ghost" size="sm" disabled={busy} onclick={disconnect}>{t('n8nui.disconnect')}</Button>
          {/if}
          <Button size="sm" disabled={busy || !dirty} onclick={save}>{t('n8nui.save')}</Button>
        </div>
      </div>
      {#if testOk}
        <p class="flex items-start gap-1.5 text-xs break-all text-success" role="status">
          <CircleCheck class="mt-px size-3.5 shrink-0" />
          {t('n8nui.test_ok', { url: testOk.url })}
        </p>
      {/if}
      {#if error}<p class="text-xs break-words text-destructive" role="alert">{error}</p>{/if}
    </div>
  </SettingGroup>

  {#if connected}
    <SettingGroup title={t('n8nui.workflows_title')} hint={t('n8nui.workflows_desc')}>
      {#snippet actions()}
        <Button variant="outline" size="sm" disabled={loading} onclick={loadData}>
          <RefreshCw class={loading ? 'animate-spin' : ''} />
          {t('n8nui.refresh')}
        </Button>
      {/snippet}
      {#if loadError}
        <p class="px-4 py-3 text-xs break-words text-destructive" role="alert">{loadError}</p>
      {:else if !workflows.length}
        <p class="px-4 py-3 text-xs text-muted-foreground">{loading ? '…' : t('n8nui.workflows_empty')}</p>
      {/if}
      {#each workflows as w (w.id)}
        <div class="flex flex-col gap-2 px-4 py-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex min-w-0 flex-wrap items-center gap-2">
              <span class="min-w-0 truncate text-sm font-medium">{w.name}</span>
              <Badge variant="outline" class={w.active ? 'text-success' : 'text-muted-foreground'}>
                {w.active ? t('n8nui.active') : t('n8nui.inactive')}
              </Badge>
              {#each w.triggers as trig (trig)}
                <Badge variant="secondary" class="font-mono text-[11px]">{trig}</Badge>
              {:else}
                <span class="text-xs text-muted-foreground">{t('n8nui.no_trigger')}</span>
              {/each}
            </div>
            <div class="flex shrink-0 flex-wrap gap-2">
              <Button variant="ghost" size="sm" onclick={() => open(w.editor_url)}>
                <ExternalLink />
                {t('n8nui.open_editor')}
              </Button>
              {#if w.webhook && confirming !== w.id}
                <Button variant="outline" size="sm" disabled={!runnable(w)} onclick={() => (confirming = w.id)}>
                  <Play />
                  {t('n8nui.run')}
                </Button>
              {/if}
            </div>
          </div>
          {#if w.webhook && !w.active}
            <p class="text-xs text-muted-foreground">{t('n8nui.run_inactive')}</p>
          {:else if !w.webhook}
            <p class="text-xs text-muted-foreground">{t('n8nui.run_unavailable')}</p>
          {/if}
          {#if confirming === w.id && w.webhook}
            <div class="flex flex-wrap items-center justify-between gap-2 rounded-md border border-warning/40 bg-warning/5 p-3" role="alert">
              <p class="min-w-0 text-xs break-words">{t('n8nui.run_confirm_hint', { name: w.name, method: w.webhook.method })}</p>
              <div class="flex shrink-0 gap-2">
                <Button variant="ghost" size="sm" disabled={running === w.id} onclick={() => (confirming = null)}>
                  {t('n8nui.run_cancel')}
                </Button>
                <Button size="sm" disabled={running === w.id} onclick={() => run(w)}>{t('n8nui.run_confirm')}</Button>
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </SettingGroup>

    <SettingGroup title={t('n8nui.executions_title')} hint={t('n8nui.executions_desc')}>
      {#if !executions.length && !loadError}
        <p class="px-4 py-3 text-xs text-muted-foreground">{loading ? '…' : t('n8nui.executions_empty')}</p>
      {/if}
      {#each executions as ex (ex.id)}
        {@const detail = errors[ex.id]}
        <div class="flex flex-col gap-1.5 px-4 py-2.5">
          <div class="flex flex-wrap items-center justify-between gap-2 text-xs">
            <div class="flex min-w-0 flex-wrap items-center gap-2">
              <Badge variant="outline" class={statusClass(ex.status)}>{statusLabel(ex.status)}</Badge>
              <span class="min-w-0 truncate font-medium">{names[ex.workflow_id] ?? ex.workflow_id}</span>
              <span class="tabular text-muted-foreground">#{ex.id} · {ex.mode} · {fmtTime(ex.started_at)}</span>
            </div>
            <div class="flex shrink-0 gap-1">
              {#if ex.status === 'error' || ex.status === 'crashed'}
                <Button variant="ghost" size="sm" disabled={detail === 'loading'} onclick={() => showError(ex)}>
                  {t('n8nui.show_error')}
                </Button>
              {/if}
              <Button variant="ghost" size="sm" onclick={() => open(ex.editor_url)}>
                <ExternalLink />
                {t('n8nui.open_editor')}
              </Button>
            </div>
          </div>
          {#if detail && detail !== 'loading'}
            <p class="rounded-md bg-destructive/5 p-2 text-xs break-words text-destructive">
              {detail.message ?? t('n8nui.error_none')}
              {#if detail.node}<span class="block text-muted-foreground">{t('n8nui.error_node', { node: detail.node })}</span>{/if}
            </p>
          {/if}
        </div>
      {/each}
    </SettingGroup>
  {/if}

  <SettingGroup title={t('n8nui.openai_title')} hint={t('n8nui.openai_desc')}>
    {#if openai?.running && openai.base_url}
      <SettingRow label={t('n8nui.base_url')} hint={t('n8nui.base_url_hint')} for="n8n-base-url">
        <div class="flex min-w-0 items-center gap-2">
          <Input id="n8n-base-url" readonly class="h-8 font-mono text-xs" value={openai.base_url} />
          <Button variant="outline" size="sm" onclick={() => openai?.base_url && copy(openai.base_url)}>
            <Copy />
            {t('n8nui.copy')}
          </Button>
        </div>
      </SettingRow>
      <div class="flex flex-col gap-2 px-4 py-3">
        <p class="text-sm font-medium">{t('n8nui.token')}</p>
        <p class="text-xs text-muted-foreground">
          {openai.requires_token ? t('n8nui.token_hint') : t('n8nui.token_not_required')}
        </p>
        {#if newToken}
          <div class="flex min-w-0 items-center gap-2">
            <Input readonly class="h-8 font-mono text-xs" value={newToken} aria-label={t('n8nui.token')} />
            <Button variant="outline" size="sm" onclick={() => newToken && copy(newToken)}>
              <Copy />
              {t('n8nui.copy')}
            </Button>
          </div>
        {:else}
          <div>
            <Button variant="outline" size="sm" disabled={tokenBusy} onclick={createToken}>{t('n8nui.token_create')}</Button>
          </div>
        {/if}
        {#if tokenError}<p class="text-xs text-destructive" role="alert">{tokenError}</p>{/if}
      </div>
    {:else}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('n8nui.gateway_off')}</p>
    {/if}
  </SettingGroup>
</div>
