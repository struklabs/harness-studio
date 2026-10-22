<script lang="ts">
  // Agents > Paperclip: the user's own Paperclip install (REST API only; Harness never runs it).
  // Teams, agents, tasks, runs and budgets are read from Paperclip; tasks can be linked to a
  // Harness Project (kept in Harness only); an opencode_local agent can be pointed at a Harness
  // model through the gateway. The board key goes to the OS keychain and never comes back.
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { CircleCheck, ExternalLink, RefreshCw, Send } from '@lucide/svelte';
  import type {
    PaperclipAgent,
    PaperclipConnection,
    PaperclipModelApplied,
    PaperclipOverview,
    PaperclipView,
  } from '#lib/types.js';
  import type { Project } from '#lib/projects.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import type { TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';

  interface Model { alias: string; chat: boolean | null }

  let view = $state<PaperclipView | null>(null);
  let url = $state('');
  /** New key typed by the user; empty = keep the saved key. */
  let apiKey = $state('');
  let busy = $state(false);
  let testing = $state(false);
  let error = $state<string | null>(null);
  let testOk = $state<PaperclipConnection | null>(null);
  let companies = $state<{ id: string; name: string }[]>([]);

  let overview = $state<PaperclipOverview | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let projects = $state<Project[]>([]);
  let models = $state<Model[]>([]);
  /** Model picked per agent id. */
  let picked = $state<Record<string, string>>({});
  let applying = $state<string | null>(null);

  let connected = $derived(!!view?.url);
  let dirty = $derived(!!view && (url.trim() !== view.url || apiKey.trim() !== ''));
  let agentNames = $derived(Object.fromEntries((overview?.agents ?? []).map((a) => [a.id, a.name])));
  let taskTitles = $derived(Object.fromEntries((overview?.tasks ?? []).map((x) => [x.id, x.title])));
  let spendByAgent = $derived(Object.fromEntries((overview?.costs ?? []).map((c) => [c.agent_id, c.cost_cents])));

  async function loadView() {
    try {
      view = await invoke<PaperclipView>('paperclip_get_settings');
      url = view.url;
    } catch (e) {
      error = localizeError(e);
    }
  }

  async function loadData() {
    if (!view?.url) return;
    loading = true;
    loadError = null;
    try {
      companies = await invoke<{ id: string; name: string }[]>('paperclip_list_companies');
      overview = await invoke<PaperclipOverview>('paperclip_overview');
      if (!view.company_id && overview) view.company_id = overview.company.id;
    } catch (e) {
      overview = null;
      loadError = localizeError(e);
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    invoke<Project[]>('projects_list', { archived: false })
      .then((p) => (projects = p))
      .catch(() => {});
    invoke<Model[]>('list_agent_choices')
      .then((m) => (models = m.filter((x) => x.chat !== false)))
      .catch(() => {});
    await loadView();
    await loadData();
  });

  async function save() {
    busy = true;
    error = null;
    try {
      view = await invoke<PaperclipView>('paperclip_save_settings', { url, apiKey: apiKey.trim() ? apiKey : null });
      url = view.url;
      apiKey = '';
      testOk = null;
      toast.success(t('pcui.saved'));
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
      view = await invoke<PaperclipView>('paperclip_save_settings', { url: '', apiKey: null });
      url = '';
      apiKey = '';
      testOk = null;
      overview = null;
      companies = [];
      toast.success(t('pcui.disconnected'));
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
      testOk = await invoke<PaperclipConnection>('paperclip_test_connection', {
        url: url.trim() || null,
        apiKey: apiKey.trim() || null,
      });
    } catch (e) {
      error = localizeError(e);
    } finally {
      testing = false;
    }
  }

  async function chooseCompany(id: string) {
    try {
      view = await invoke<PaperclipView>('paperclip_set_company', { companyId: id });
      await loadData();
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  async function linkTask(taskId: string, projectId: string) {
    try {
      view = await invoke<PaperclipView>('paperclip_link_task', { taskId, projectId: projectId || null });
      const task = overview?.tasks.find((x) => x.id === taskId);
      if (task) task.harness_project_id = projectId || null;
      const project = projects.find((p) => p.id === projectId);
      toast.success(project ? t('pcui.project_linked', { project: project.name }) : t('pcui.project_unlinked'));
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  async function useModel(agent: PaperclipAgent) {
    const model = picked[agent.id];
    if (!model) return;
    applying = agent.id;
    try {
      const r = await invoke<PaperclipModelApplied>('paperclip_use_model', { agentId: agent.id, model });
      toast.success(t('pcui.use_model_done', { name: agent.name, model: r.model }));
      await loadData();
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      applying = null;
    }
  }

  const open = (link: string) => openUrl(link).catch((e) => toast.error(localizeError(e)));
  const stateKey = (s: string) =>
    `pcui.st_${['idle', 'running', 'paused', 'error', 'terminated', 'queued', 'succeeded', 'failed', 'timed_out', 'cancelled', 'backlog', 'todo', 'in_progress', 'blocked', 'in_review', 'done'].includes(s) ? s : 'unknown'}` as TKey;
  const stateClass = (s: string) =>
    ['succeeded', 'done', 'running', 'idle'].includes(s) ? 'text-success' : ['failed', 'error', 'timed_out', 'blocked'].includes(s) ? 'text-destructive' : '';
  const money = (cents: number) =>
    new Intl.NumberFormat(locale(), { style: 'currency', currency: 'USD' }).format(cents / 100);
  const fmtTime = (iso: string | null) => (iso ? new Date(iso).toLocaleString(locale()) : '');
  const healthKey = (s: string) => `pcui.health_${['ok', 'starting', 'unhealthy'].includes(s) ? s : 'unhealthy'}` as TKey;
  const canUseModel = (a: PaperclipAgent) => a.adapter_type === 'opencode_local' && a.status !== 'terminated';
</script>

<div class="flex flex-col gap-4">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <h4 class="font-semibold">{t('pcui.title')}</h4>
    <Badge variant="outline" class={connected ? 'text-success' : ''}>
      {connected ? t('pcui.status_connected') : t('pcui.status_off')}
    </Badge>
  </div>
  <p class="text-sm text-muted-foreground">{t('pcui.desc')}</p>

  <div class="flex flex-col gap-1">
    <label class="text-sm" for="pc-url">{t('pcui.url')}</label>
    <Input id="pc-url" class="h-8 font-mono text-xs" spellcheck={false} autocomplete="off" placeholder="http://localhost:3100" bind:value={url} />
    <p class="text-xs text-muted-foreground">{t('pcui.url_hint')}</p>
  </div>
  <div class="flex flex-col gap-1">
    <label class="text-sm" for="pc-key">{t('pcui.key')}</label>
    <Input id="pc-key" type="password" class="h-8 font-mono text-xs" spellcheck={false} autocomplete="off" placeholder={t('pcui.key_placeholder')} bind:value={apiKey} />
    <p class="text-xs text-muted-foreground">{t('pcui.key_hint')}</p>
    {#if view?.key_set}<p class="text-xs text-muted-foreground">{t('pcui.key_saved')}</p>{/if}
  </div>
  <div class="flex flex-wrap gap-2">
    <Button variant="outline" size="sm" disabled={testing || (!url.trim() && !view?.url)} onclick={runTest}>
      <Send />
      {testing ? t('pcui.testing') : t('pcui.test')}
    </Button>
    {#if view?.url}
      <Button variant="ghost" size="sm" disabled={busy} onclick={disconnect}>{t('pcui.disconnect')}</Button>
    {/if}
    <Button size="sm" disabled={busy || !dirty} onclick={save}>{t('pcui.save')}</Button>
  </div>
  {#if testOk}
    <p class="flex items-start gap-1.5 text-xs break-all text-success" role="status">
      <CircleCheck class="mt-px size-3.5 shrink-0" />
      {t('pcui.test_ok', { url: testOk.url, status: testOk.health.status, count: testOk.companies.length })}
    </p>
  {/if}
  {#if error}<p class="text-xs break-words text-destructive" role="alert">{error}</p>{/if}

  {#if connected}
    <div class="flex flex-wrap items-center gap-2">
      {#if companies.length > 1}
        <label class="text-sm" for="pc-company">{t('pcui.company')}</label>
        <select id="pc-company" class="rounded border bg-background p-1.5 text-sm" value={view?.company_id} onchange={(e) => chooseCompany(e.currentTarget.value)}>
          {#each companies as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
        </select>
      {/if}
      <Button variant="outline" size="sm" disabled={loading} onclick={loadData}>
        <RefreshCw class={loading ? 'animate-spin' : ''} />
        {t('pcui.refresh')}
      </Button>
      {#if view?.url}
        <Button variant="ghost" size="sm" onclick={() => view && open(view.url)}>
          <ExternalLink />
          {t('pcui.open')}
        </Button>
      {/if}
    </div>
    {#if companies.length > 1}<p class="text-xs text-muted-foreground">{t('pcui.company_hint')}</p>{/if}
    {#if loadError}<p class="text-xs break-words text-destructive" role="alert">{loadError}</p>{/if}

    {#if overview}
      <section class="flex flex-col gap-1.5" aria-label={t('pcui.health_title')}>
        <h5 class="text-sm font-semibold">{t('pcui.health_title')}</h5>
        <div class="flex flex-wrap items-center gap-2 text-xs">
          <span>{t('pcui.health')}:</span>
          <Badge variant="outline" class={overview.health.status === 'ok' ? 'text-success' : 'text-destructive'}>{t(healthKey(overview.health.status))}</Badge>
          {#if overview.health.version}<span class="tabular text-muted-foreground">v{overview.health.version}</span>{/if}
          {#if overview.health.deployment_mode}
            <Badge variant="secondary">{overview.health.deployment_mode === 'local_trusted' ? t('pcui.mode_local_trusted') : t('pcui.mode_authenticated')}</Badge>
          {/if}
        </div>
        {#if overview.budget}
          <p class="text-xs">
            {overview.budget.month_budget_cents > 0
              ? t('pcui.budget_month', { spent: money(overview.budget.month_spend_cents), budget: money(overview.budget.month_budget_cents) })
              : t('pcui.budget_none', { spent: money(overview.budget.month_spend_cents) })}
          </p>
          <p class="text-xs text-muted-foreground">
            {t('pcui.budget_flags', { approvals: overview.budget.pending_approvals, paused: overview.budget.paused_agents, incidents: overview.budget.active_incidents })}
          </p>
        {:else}
          <p class="text-xs text-muted-foreground">{t('pcui.budget_unreadable')}</p>
        {/if}
      </section>

      <section class="flex flex-col gap-2" aria-label={t('pcui.agents_title')}>
        <h5 class="text-sm font-semibold">{t('pcui.agents_title')}</h5>
        <p class="text-xs text-muted-foreground">{t('pcui.agents_desc')}</p>
        {#if !overview.agents.length}<p class="text-xs text-muted-foreground">{t('pcui.agents_empty')}</p>{/if}
        {#each overview.agents as a (a.id)}
          <div class="flex flex-col gap-1.5 rounded border p-3">
            <div class="flex flex-wrap items-center gap-2">
              <span class="min-w-0 truncate text-sm font-medium">{a.name}</span>
              <Badge variant="outline" class={stateClass(a.status)}>{t(stateKey(a.status))}</Badge>
              <Badge variant="secondary" class="font-mono text-[11px]">{a.adapter_type || '—'}</Badge>
              {#if a.uses_harness}<Badge variant="outline" class="text-success">{t('pcui.harness_model')}</Badge>{/if}
            </div>
            <p class="text-xs text-muted-foreground">
              {t('pcui.role')}: {a.role}{a.title ? ` · ${a.title}` : ''}
              {#if a.model} · {t('pcui.model')}: <span class="font-mono">{a.model}</span>{/if}
            </p>
            <p class="tabular text-xs text-muted-foreground">
              {a.budget_monthly_cents > 0
                ? t('pcui.spent', { spent: money(Math.max(a.spent_monthly_cents, spendByAgent[a.id] ?? 0)), budget: money(a.budget_monthly_cents) })
                : t('pcui.spent_open', { spent: money(Math.max(a.spent_monthly_cents, spendByAgent[a.id] ?? 0)) })}
            </p>
            {#if canUseModel(a)}
              <div class="flex flex-wrap items-center gap-2">
                <select class="rounded border bg-background p-1.5 text-xs" aria-label={t('pcui.use_model_pick')} bind:value={picked[a.id]}>
                  <option value="">{t('pcui.use_model_pick')}</option>
                  {#each models as m (m.alias)}<option value={m.alias}>{m.alias}</option>{/each}
                </select>
                <Button variant="outline" size="sm" disabled={!picked[a.id] || applying === a.id} onclick={() => useModel(a)}>{t('pcui.use_model')}</Button>
              </div>
              <p class="text-xs text-muted-foreground">{t('pcui.use_model_hint')}</p>
            {/if}
          </div>
        {/each}
      </section>

      <section class="flex flex-col gap-1.5" aria-label={t('pcui.teams_title')}>
        <h5 class="text-sm font-semibold">{t('pcui.teams_title')}</h5>
        <p class="text-xs text-muted-foreground">{t('pcui.teams_desc')}</p>
        {#each overview.teams as team (team.lead_id)}
          <p class="text-xs">{t('pcui.team_members', { lead: team.lead_name, members: team.member_ids.map((id) => agentNames[id] ?? id).join(', ') })}</p>
        {:else}
          <p class="text-xs text-muted-foreground">{t('pcui.teams_empty')}</p>
        {/each}
      </section>

      <section class="flex flex-col gap-2" aria-label={t('pcui.tasks_title')}>
        <h5 class="text-sm font-semibold">{t('pcui.tasks_title')}</h5>
        <p class="text-xs text-muted-foreground">{t('pcui.tasks_desc')}</p>
        {#if !overview.tasks.length}<p class="text-xs text-muted-foreground">{t('pcui.tasks_empty')}</p>{/if}
        {#each overview.tasks as task (task.id)}
          <div class="flex flex-wrap items-center justify-between gap-2 rounded border px-3 py-2">
            <div class="flex min-w-0 flex-wrap items-center gap-2 text-xs">
              <Badge variant="outline" class={stateClass(task.status)}>{t(stateKey(task.status))}</Badge>
              {#if task.identifier}<span class="font-mono text-muted-foreground">{task.identifier}</span>{/if}
              <span class="min-w-0 truncate text-sm font-medium">{task.title}</span>
              <span class="text-muted-foreground">{task.assignee_agent_id ? (agentNames[task.assignee_agent_id] ?? task.assignee_agent_id) : t('pcui.unassigned')}</span>
            </div>
            <select
              class="rounded border bg-background p-1.5 text-xs"
              aria-label={t('pcui.project')}
              value={task.harness_project_id ?? ''}
              onchange={(e) => linkTask(task.id, e.currentTarget.value)}
            >
              <option value="">{t('pcui.project_none')}</option>
              {#each projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
            </select>
          </div>
        {/each}
      </section>

      <section class="flex flex-col gap-1.5" aria-label={t('pcui.runs_title')}>
        <h5 class="text-sm font-semibold">{t('pcui.runs_title')}</h5>
        <p class="text-xs text-muted-foreground">{t('pcui.runs_desc')}</p>
        {#if !overview.runs.length}<p class="text-xs text-muted-foreground">{t('pcui.runs_empty')}</p>{/if}
        {#each overview.runs as run (run.id)}
          <div class="flex flex-col gap-1 text-xs">
            <div class="flex flex-wrap items-center gap-2">
              <Badge variant="outline" class={stateClass(run.status)}>{t(stateKey(run.status))}</Badge>
              <span class="font-medium">{agentNames[run.agent_id] ?? run.agent_id}</span>
              {#if run.issue_id && taskTitles[run.issue_id]}<span class="min-w-0 truncate text-muted-foreground">{taskTitles[run.issue_id]}</span>{/if}
              <span class="tabular text-muted-foreground">{fmtTime(run.started_at)}</span>
            </div>
            {#if run.error}<p class="rounded-md bg-destructive/5 p-2 break-words text-destructive">{run.error}</p>{/if}
          </div>
        {/each}
      </section>
    {/if}
  {/if}
</div>
