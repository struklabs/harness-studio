<script lang="ts">
  // Coding: project-scoped code work with a local or remote model through the Gateway, with
  // workspace permissions, command approval, task progress, a change-review view and undoable
  // checkpoints. Not an IDE: it works in the project folder and shows what changed.
  import './common/polished-pages.css';
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { projects } from '#lib/stores/projects.svelte.js';
  import { projectApi, type Project } from '#lib/projects.js';
  import {
    codingApi, applyEvent, diffLines, totals, starters, warningKey,
    type Checkpoint, type CodingEvent, type FileChange, type Step, type WorkspacePermissions,
  } from '#lib/coding.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Code, FolderKanban, Play, Square, ShieldCheck, Undo2, RefreshCw, Maximize2, Minimize2, GitBranch, Cable, MessageSquare, Plus, FileDiff } from '@lucide/svelte';

  interface Model { alias: string; chat: boolean | null; tools: string; remote?: boolean }

  let all = $state<Project[]>([]);
  let selectedId = $state<string | null>(null);
  let models = $state<Model[]>([]);
  let model = $state('');
  let prompt = $state('');
  let permissions = $state<WorkspacePermissions | null>(null);
  let checkpoints = $state<Checkpoint[]>([]);
  let reviewId = $state<string | null>(null);
  let changes = $state<FileChange[]>([]);
  let openFile = $state<string | null>(null);
  let reviewFocus = $state(false);
  let error = $state('');
  let notice = $state('');
  let busy = $state(false);

  // The run in progress (or the last one).
  let pendingStart = false;
  let runId = $state<string | null>(null);
  let steps = $state<Step[]>([]);
  let approval = $state<{ id: string; command: string; warnings: string[] } | null>(null);
  let runState = $state<'idle' | 'running' | 'answered' | 'round_limit' | 'cancelled' | 'error'>('idle');
  let runText = $state('');
  let runError = $state('');

  let patchOpen = $state(false);
  let patchText = $state('');
  let patchError = $state('');

  const withFolder = $derived(all.filter((p) => !p.archived && p.folder));
  const project = $derived(all.find((p) => p.id === selectedId) ?? null);
  const running = $derived(runState === 'running');
  const review = $derived(checkpoints.find((c) => c.id === reviewId) ?? null);
  const sum = $derived(totals(changes));
  const shown = $derived(changes.find((c) => c.path === openFile) ?? null);
  const canRun = $derived(!!project?.folder && !!model && !!prompt.trim() && !running && app.gatewayRunning && !permissions?.folder_missing);

  async function load() {
    try {
      all = await projectApi.list(false);
      if (!selectedId || !all.some((p) => p.id === selectedId)) {
        selectedId = (projects.activeId && withFolder.find((p) => p.id === projects.activeId)?.id) || withFolder[0]?.id || null;
      }
      error = '';
    } catch (e) { error = errorText(e); }
  }

  async function loadModels() {
    try {
      const list = (await invoke<Model[]>('list_agent_choices')).filter((m) => m.chat !== false);
      list.sort((a, b) => Number(b.tools === 'verified') - Number(a.tools === 'verified') || a.alias.localeCompare(b.alias));
      models = list;
      if (!model || !list.some((m) => m.alias === model)) model = list[0]?.alias ?? '';
    } catch (e) { error = errorText(e); }
  }

  async function loadProject(id: string) {
    permissions = null; checkpoints = []; reviewId = null; changes = []; openFile = null;
    steps = []; runState = 'idle'; runText = ''; runError = ''; approval = null; runId = null;
    try {
      const [perm, cps] = await Promise.all([codingApi.permissions(id), codingApi.checkpoints(id)]);
      if (selectedId !== id) return;
      permissions = perm; checkpoints = cps;
      if (cps[0]) await selectReview(cps[0].id);
    } catch (e) { error = errorText(e); }
  }

  async function selectReview(id: string) {
    if (!selectedId) return;
    reviewId = id; openFile = null;
    try {
      const list = await codingApi.changes(selectedId, id);
      if (reviewId !== id) return;
      changes = list;
      openFile = list[0]?.path ?? null;
    } catch (e) { changes = []; error = errorText(e); }
  }

  async function refreshCheckpoints(select?: string) {
    if (!selectedId) return;
    checkpoints = await codingApi.checkpoints(selectedId);
    await selectReview(select ?? reviewId ?? checkpoints[0]?.id ?? '');
  }

  $effect(() => {
    const id = selectedId;
    if (id) void loadProject(id);
  });

  function onEvent(event: CodingEvent) {
    if (pendingStart && runId === null) runId = event.run_id;
    if (event.run_id !== runId) return;
    steps = applyEvent(steps, event);
    if (event.type === 'started') {
      runState = 'running';
      void refreshCheckpoints(event.checkpoint.id).catch((e) => (error = errorText(e)));
    } else if (event.type === 'approval') {
      approval = { id: event.approval_id, command: event.summary, warnings: event.warnings };
    } else if (event.type === 'call' && event.status !== 'running') {
      approval = null;
    } else if (event.type === 'finished') {
      pendingStart = false;
      approval = null;
      runState = (['answered', 'round_limit', 'cancelled', 'error'].includes(event.stop) ? event.stop : 'answered') as typeof runState;
      runText = event.text;
      runError = event.error ? errorText(event.error) : '';
      void refreshCheckpoints().catch((e) => (error = errorText(e)));
    }
  }

  onMount(() => {
    void load();
    void loadModels();
    void app.refreshGateway();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen<CodingEvent>('coding-event', (e) => onEvent(e.payload)).then((fn) => { if (disposed) fn(); else unlisten = fn; });
    return () => { disposed = true; unlisten?.(); };
  });

  async function start() {
    if (!project || !canRun) return;
    const base = app.gatewayBaseUrl;
    if (!base) return;
    error = ''; notice = '';
    steps = []; runText = ''; runError = ''; approval = null; runId = null;
    pendingStart = true; runState = 'running';
    try {
      await codingApi.run(project.id, base, app.gateway?.client_token ?? null, model, prompt.trim());
    } catch (e) {
      pendingStart = false; runState = 'error'; runError = errorText(e);
    }
  }

  async function stop() {
    if (runId) await codingApi.cancel(runId).catch(() => false);
  }

  async function answer(allow: boolean) {
    const a = approval;
    if (!a) return;
    approval = null;
    await codingApi.approve(a.id, allow).catch((e) => (error = errorText(e)));
  }

  async function act(fn: () => Promise<unknown>) {
    busy = true; error = ''; notice = '';
    try { await fn(); } catch (e) { error = errorText(e); } finally { busy = false; }
  }

  function undo(files: string[] | null) {
    const id = selectedId; const cp = reviewId;
    if (!id || !cp) return;
    void act(async () => {
      const done = await codingApi.undo(id, cp, files);
      notice = t('coding.undone', { count: done.length });
      await refreshCheckpoints(cp);
    });
  }

  function createCheckpoint() {
    const id = selectedId;
    if (!id) return;
    void act(async () => {
      const cp = await codingApi.createCheckpoint(id, t('coding.checkpoint_default_label'));
      await refreshCheckpoints(cp.id);
    });
  }

  function removeCheckpoint(cp: Checkpoint) {
    const id = selectedId;
    if (!id) return;
    void act(async () => {
      await codingApi.deleteCheckpoint(id, cp.id);
      if (reviewId === cp.id) reviewId = null;
      await refreshCheckpoints();
    });
  }

  async function applyPatch() {
    const id = selectedId;
    if (!id || !patchText.trim()) return;
    busy = true; patchError = '';
    try {
      const done = await codingApi.applyPatch(id, patchText);
      patchOpen = false; patchText = '';
      notice = t('coding.patch_applied', { count: done.files.length });
      await refreshCheckpoints(done.checkpoint.id);
    } catch (e) { patchError = errorText(e); }
    finally { busy = false; }
  }

  function setStarter(key: string) {
    prompt = t(key as Parameters<typeof t>[0]);
  }

  const statusLabel = (s: Step['status']) =>
    t(({ running: 'coding.step_running', done: 'coding.step_done', error: 'coding.step_error', denied: 'coding.step_denied' } as const)[s]);
  const fileLabel = (s: FileChange['status']) =>
    t(({ added: 'coding.file_added', modified: 'coding.file_modified', deleted: 'coding.file_deleted' } as const)[s]);
  const stateText = $derived(runState === 'idle' ? '' : t(({
    running: 'coding.state_running', answered: 'coding.state_answered', round_limit: 'coding.state_round_limit',
    cancelled: 'coding.state_cancelled', error: 'coding.state_error',
  } as const)[runState]));
  const decisionLabel = (name: string, d: string) =>
    d === 'refuse' ? t('coding.perm_refuse') : d === 'ask' ? t('coding.perm_ask') : name === 'write_file' || name === 'edit_file' ? t('coding.perm_run_edit') : t('coding.perm_run');
  const badgeClass = (k: string) => k === 'add' ? 'text-success' : k === 'del' ? 'text-destructive' : k === 'hunk' ? 'text-muted-foreground' : '';
  const modelLabel = (m: Model) => (m.remote ? `${m.alias} · ${t('coding.model_remote')}` : m.alias);

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && reviewFocus) reviewFocus = false;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="polished-page flex h-full min-h-0">
  <aside class="flex w-64 shrink-0 flex-col gap-2 border-r border-border p-3">
    <h1 class="text-sm font-semibold">{t('coding.projects')}</h1>
    <nav class="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto" aria-label={t('coding.projects')}>
      {#each withFolder as p (p.id)}
        <button type="button" class="flex flex-col rounded-md px-2 py-1.5 text-left hover:bg-muted/60 {selectedId === p.id ? 'bg-muted/60' : ''}" aria-current={selectedId === p.id ? 'page' : undefined} onclick={() => (selectedId = p.id)}>
          <span class="truncate text-sm">{p.name}</span>
          <span class="truncate text-xs text-muted-foreground">{p.folder}</span>
        </button>
      {:else}
        <p class="px-2 text-xs text-muted-foreground">{t('coding.no_projects')}</p>
      {/each}
    </nav>
    <Button size="sm" variant="outline" onclick={() => (app.requestedTab = 'projects')}><FolderKanban />{t('coding.open_projects')}</Button>
  </aside>

  <main class="min-w-0 flex-1 overflow-y-auto p-6">
    {#if error}<p class="mb-3 text-sm text-destructive" role="alert">{error}</p>{/if}
    {#if notice}<p class="mb-3 text-sm text-muted-foreground" role="status">{notice}</p>{/if}

    {#if !project}
      <div class="flex flex-col items-start gap-3">
        <Code class="size-8 text-muted-foreground" />
        <h2 class="text-lg font-semibold">{t('coding.title')}</h2>
        <p class="max-w-prose text-sm text-muted-foreground">{t('coding.intro')}</p>
        <p class="text-sm text-muted-foreground">{t('coding.no_projects')}</p>
        <Button onclick={() => (app.requestedTab = 'projects')}><FolderKanban />{t('coding.open_projects')}</Button>
      </div>
    {:else}
      <header class="flex flex-col gap-2">
        <div class="flex flex-wrap items-center gap-2">
          <h2 class="text-lg font-semibold">{project.name}</h2>
          {#if permissions}<Badge variant="outline"><GitBranch class="size-3" />{t(permissions.backend === 'git' ? 'coding.backend_git' : 'coding.backend_snapshot')}</Badge>{/if}
        </div>
        <p class="max-w-prose text-sm text-muted-foreground">{t('coding.intro')}</p>
      </header>

      <section class="mt-6 flex flex-col gap-2 rounded-lg border border-border p-4" aria-labelledby="coding-perms">
        <h3 id="coding-perms" class="flex items-center gap-2 text-sm font-semibold"><ShieldCheck class="size-4" />{t('coding.permissions')}</h3>
        {#if permissions}
          {#if permissions.folder_missing}
            <p class="text-xs text-warning">{t('coding.folder_missing')}</p>
          {:else if permissions.folder}
            <p class="text-xs"><code class="break-all">{t('coding.perm_folder', { path: permissions.folder })}</code></p>
          {:else}
            <p class="text-xs text-warning">{t('coding.no_folder')}</p>
          {/if}
          {#if permissions.extra_folders.length}
            <p class="text-xs text-muted-foreground">{t('coding.perm_extra')}</p>
            {#each permissions.extra_folders as folder (folder)}<code class="break-all text-xs text-muted-foreground">{folder}</code>{/each}
          {/if}
          <p class="text-xs text-muted-foreground">{t('coding.perm_protected', { names: permissions.protected.join(', ') })}</p>
          <p class="text-xs text-muted-foreground">{t(permissions.backend === 'git' ? 'coding.perm_backend_git' : 'coding.perm_backend_snapshot')}</p>
          {#if permissions.policy === 'off'}<p class="text-xs text-warning">{t('coding.perm_policy_off')}</p>{/if}
          <ul class="flex flex-wrap gap-1.5" aria-label={t('coding.permissions')}>
            {#each permissions.tools as tool (tool.name)}
              <li><Badge variant={tool.decision === 'ask' ? 'secondary' : 'outline'} title={decisionLabel(tool.name, tool.decision)}>{tool.name} · {decisionLabel(tool.name, tool.decision)}</Badge></li>
            {/each}
          </ul>
          <p class="text-xs text-muted-foreground">{t('coding.perm_hint')}</p>
        {/if}
      </section>

      <section class="mt-6 flex flex-col gap-3" aria-labelledby="coding-task">
        <h3 id="coding-task" class="text-sm font-semibold">{t('coding.task')}</h3>
        {#if !app.gatewayRunning}
          <p class="text-xs text-warning">{t('coding.gateway_off')}</p>
          <div><Button size="sm" variant="outline" onclick={() => (app.requestedTab = 'connections')}><Cable />{t('coding.open_connections')}</Button></div>
        {/if}
        <label class="flex max-w-md flex-col gap-1 text-sm" for="coding-model">{t('coding.model')}
          <select id="coding-model" class="w-full rounded border bg-background p-2" bind:value={model} disabled={running}>
            {#each models as m (m.alias)}<option value={m.alias}>{modelLabel(m)}</option>{/each}
          </select>
        </label>
        {#if models.length === 0}<p class="text-xs text-muted-foreground">{t('coding.model_none')}</p>{/if}
        <div class="flex flex-wrap gap-2" role="group" aria-label={t('coding.task')}>
          {#each starters as s (s.id)}
            <Button size="xs" variant="outline" disabled={running} onclick={() => setStarter(s.prompt)}>{t(s.label)}</Button>
          {/each}
        </div>
        <label class="flex flex-col gap-1 text-sm" for="coding-prompt">{t('coding.prompt')}
          <Textarea id="coding-prompt" bind:value={prompt} rows={4} disabled={running} placeholder={t('coding.prompt_placeholder')} />
        </label>
        <div class="flex gap-2">
          <Button disabled={!canRun} onclick={start}><Play />{t('coding.run')}</Button>
          {#if running}<Button variant="outline" onclick={stop}><Square />{t('coding.stop')}</Button>{/if}
          <Button variant="ghost" onclick={() => { projects.setActive(project.id); app.requestedTab = 'playground'; }}><MessageSquare />{t('coding.use_in_playground')}</Button>
        </div>
      </section>

      <section class="mt-6 flex flex-col gap-2" aria-labelledby="coding-progress">
        <h3 id="coding-progress" class="text-sm font-semibold">{t('coding.progress')}</h3>
        {#if runState === 'idle' && steps.length === 0}
          <p class="text-xs text-muted-foreground">{t('coding.progress_empty')}</p>
        {:else}
          <p class="text-xs text-muted-foreground" role="status">{stateText}</p>
        {/if}
        {#if approval}
          <div class="flex flex-col gap-2 rounded-lg border border-warning p-3" role="alertdialog" aria-labelledby="coding-approval">
            <h4 id="coding-approval" class="text-sm font-semibold">{t('coding.approval_title')}</h4>
            <p class="text-xs text-muted-foreground">{t('coding.approval_text')}</p>
            <pre class="overflow-x-auto whitespace-pre-wrap break-all rounded bg-muted/60 p-2 text-xs">{approval.command}</pre>
            {#if approval.warnings.length}
              <div class="flex flex-wrap gap-1">{#each approval.warnings as w (w)}<Badge variant="outline" class="text-warning">{t(warningKey(w) as Parameters<typeof t>[0])}</Badge>{/each}</div>
            {/if}
            <div class="flex gap-2">
              <Button size="sm" onclick={() => answer(true)}>{t('coding.allow')}</Button>
              <Button size="sm" variant="outline" onclick={() => answer(false)}>{t('coding.deny')}</Button>
            </div>
          </div>
        {/if}
        <ol class="flex flex-col divide-y divide-border rounded-lg border border-border empty:hidden">
          {#each steps as step (step.key)}
            <li class="flex flex-wrap items-center gap-2 p-2 text-xs">
              <Badge variant={step.status === 'error' || step.status === 'denied' ? 'destructive' : 'outline'}>{statusLabel(step.status)}</Badge>
              <code>{step.tool}</code>
              <span class="min-w-0 flex-1 truncate font-mono text-muted-foreground" title={step.summary}>{step.summary}</span>
            </li>
          {/each}
        </ol>
        {#if runError}<p class="text-sm text-destructive" role="alert">{runError}</p>{/if}
        {#if runText}<p class="max-w-prose whitespace-pre-wrap text-sm">{runText}</p>{/if}
      </section>

      <section class="mt-6 flex flex-col gap-2 {reviewFocus ? 'fixed inset-0 z-40 m-0 overflow-y-auto bg-background p-6' : ''}" aria-labelledby="coding-review">
        <div class="flex flex-wrap items-center gap-2">
          <h3 id="coding-review" class="flex items-center gap-2 text-sm font-semibold"><FileDiff class="size-4" />{t('coding.review')}</h3>
          {#if changes.length}<span class="text-xs text-muted-foreground">{t('coding.review_summary', { files: sum.files, additions: sum.additions, deletions: sum.deletions })}</span>{/if}
          <span class="flex-1"></span>
          <Button size="xs" variant="ghost" disabled={!reviewId || busy} onclick={() => reviewId && selectReview(reviewId)}><RefreshCw />{t('coding.review_refresh')}</Button>
          <Button size="xs" variant="outline" disabled={!changes.length || busy} onclick={() => undo(null)}><Undo2 />{t('coding.undo_all')}</Button>
          <Button size="xs" variant="ghost" aria-pressed={reviewFocus} aria-label={t('coding.review')} onclick={() => (reviewFocus = !reviewFocus)}>{#if reviewFocus}<Minimize2 />{:else}<Maximize2 />{/if}</Button>
        </div>
        {#if checkpoints.length}
          <label class="flex max-w-md flex-col gap-1 text-xs" for="coding-review-cp">{t('coding.review_checkpoint')}
            <select id="coding-review-cp" class="w-full rounded border bg-background p-1.5 text-sm" value={reviewId ?? ''} onchange={(e) => selectReview((e.currentTarget as HTMLSelectElement).value)}>
              {#each checkpoints as cp (cp.id)}<option value={cp.id}>{new Date(cp.created_at).toLocaleString()} · {cp.label}{cp.undone ? ` · ${t('coding.checkpoint_undone')}` : ''}</option>{/each}
            </select>
          </label>
        {/if}
        {#if changes.length === 0}
          <p class="text-xs text-muted-foreground">{t('coding.review_empty')}</p>
        {:else}
          <div class="grid min-h-0 gap-3 {reviewFocus ? 'lg:grid-cols-[18rem_1fr]' : 'lg:grid-cols-[16rem_1fr]'}">
            <ul class="flex max-h-96 flex-col divide-y divide-border overflow-y-auto rounded-lg border border-border" aria-label={t('coding.review')}>
              {#each changes as c (c.path)}
                <li class="flex items-center gap-1 {openFile === c.path ? 'bg-muted/60' : ''}">
                  <button type="button" class="flex min-w-0 flex-1 flex-col px-2 py-1.5 text-left text-xs" aria-current={openFile === c.path ? 'true' : undefined} onclick={() => (openFile = c.path)}>
                    <span class="truncate font-mono" title={c.path}>{c.path}</span>
                    <span class="text-muted-foreground">{fileLabel(c.status)} · +{c.additions} −{c.deletions}</span>
                  </button>
                  <Button size="icon-xs" variant="ghost" disabled={busy} aria-label={t('coding.undo_file')} title={t('coding.undo_file')} onclick={() => undo([c.path])}><Undo2 /></Button>
                </li>
              {/each}
            </ul>
            <div class="min-w-0 overflow-x-auto rounded-lg border border-border">
              {#if !shown}
                <p class="p-3 text-xs text-muted-foreground">{t('coding.review_pick')}</p>
              {:else if shown.binary}
                <p class="p-3 text-xs text-muted-foreground">{t('coding.review_binary')}</p>
              {:else}
                <pre class="p-2 text-xs leading-5" aria-label={shown.path}>{#each diffLines(shown.diff) as line, i (i)}<span class="block whitespace-pre {badgeClass(line.kind)} {line.kind === 'add' ? 'bg-success/10' : line.kind === 'del' ? 'bg-destructive/10' : ''}">{line.text || ' '}</span>{/each}</pre>
              {/if}
            </div>
          </div>
        {/if}
      </section>

      <section class="mt-6 flex flex-col gap-2" aria-labelledby="coding-checkpoints">
        <div class="flex flex-wrap items-center gap-2">
          <h3 id="coding-checkpoints" class="text-sm font-semibold">{t('coding.checkpoints')}</h3>
          <span class="flex-1"></span>
          <Button size="xs" variant="outline" disabled={busy || running || !!permissions?.folder_missing} onclick={createCheckpoint}><Plus />{t('coding.checkpoint_create')}</Button>
          <Button size="xs" variant="outline" disabled={busy || running || !!permissions?.folder_missing} onclick={() => { patchOpen = true; patchError = ''; }}>{t('coding.patch')}</Button>
        </div>
        {#if checkpoints.length === 0}<p class="text-xs text-muted-foreground">{t('coding.checkpoints_empty')}</p>{/if}
        <ul class="flex flex-col divide-y divide-border rounded-lg border border-border empty:hidden">
          {#each checkpoints as cp (cp.id)}
            <li class="flex flex-wrap items-center gap-2 p-2 text-xs">
              <div class="flex min-w-0 flex-1 flex-col">
                <span class="truncate">{t('coding.checkpoint_label', { label: cp.label, files: cp.files, backend: t(cp.backend === 'git' ? 'coding.backend_git' : 'coding.backend_snapshot') })}</span>
                <span class="text-muted-foreground">{new Date(cp.created_at).toLocaleString()}</span>
              </div>
              {#if cp.undone}<Badge variant="outline">{t('coding.checkpoint_undone')}</Badge>{/if}
              <Button size="xs" variant="ghost" disabled={busy} onclick={() => selectReview(cp.id)}>{t('coding.checkpoint_review')}</Button>
              <Button size="xs" variant="ghost" disabled={busy || running} onclick={() => removeCheckpoint(cp)}>{t('coding.checkpoint_delete')}</Button>
            </li>
          {/each}
        </ul>
      </section>

      <section class="mt-6 flex flex-col gap-2 rounded-lg border border-border p-4" aria-labelledby="coding-others">
        <h3 id="coding-others" class="text-sm font-semibold">{t('coding.others')}</h3>
        <p class="max-w-prose text-xs text-muted-foreground">{t('coding.others_text')}</p>
        <p class="text-xs text-muted-foreground">{t('coding.others_docs')}</p>
        <div><Button size="sm" variant="outline" onclick={() => (app.requestedTab = 'connections')}><Cable />{t('coding.others_open')}</Button></div>
      </section>
    {/if}
  </main>
</div>

<Dialog.Root bind:open={patchOpen}>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>{t('coding.patch')}</Dialog.Title>
      <Dialog.Description>{t('coding.patch_hint')}</Dialog.Description>
    </Dialog.Header>
    <Textarea bind:value={patchText} rows={12} class="font-mono text-xs" aria-label={t('coding.patch')} placeholder={t('coding.patch_placeholder')} disabled={busy} />
    {#if patchError}<p class="text-sm text-destructive" role="alert">{patchError}</p>{/if}
    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={() => (patchOpen = false)}>{t('coding.cancel')}</Button>
      <Button disabled={busy || !patchText.trim()} onclick={applyPatch}>{t('coding.patch_apply')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
