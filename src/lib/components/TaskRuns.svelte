<script lang="ts">
  // Task runs of a project: agent tasks and analysis sessions that can be drafted, stepped,
  // rewound, stopped, resumed after a restart and moved to another device as a bundle.
  import { onMount } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { app } from '#lib/stores/app.svelte.js';
  import {
    taskRunApi, emptyDraft, parseSteps, stepsText, lines, formatCost, RESUMABLE,
    type RunStatus, type StepStatus, type TaskRunRow, type TaskRunDetail, type TaskRunDraft, type RunKind, type ModelCheck, type CompatNote, type Endpoint,
  } from '#lib/taskruns.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Play, StepForward, Square, Download, Upload, Pencil, Trash2, Plus, RotateCcw, Check, X, ChevronDown, ChevronRight } from '@lucide/svelte';

  let { projectId, archived = false }: { projectId: string; archived?: boolean } = $props();
  // The page re-mounts this component per project (`{#key}`), so the id is fixed for its life.

  let rows = $state<TaskRunRow[]>([]);
  let open = $state<Record<string, TaskRunDetail | null>>({});
  let error = $state('');
  let notice = $state('');
  let busy = $state(false);
  let embedFiles = $state(true);

  // editor
  let editOpen = $state(false);
  let editingId = $state<string | null>(null);
  // svelte-ignore state_referenced_locally
  let draft = $state<TaskRunDraft>(emptyDraft(projectId, 'agent_task'));
  let modelsText = $state('');
  let filesText = $state('');
  let stepsTextValue = $state('');
  let minContext = $state('');
  let formError = $state('');
  let gatewayModels = $state<string[]>([]);

  // start / resume dialog
  let launch = $state<{ row: TaskRunRow; step: boolean } | null>(null);
  let checks = $state<ModelCheck[]>([]);
  let compat = $state<CompatNote[]>([]);
  let chosenModel = $state('');
  let launchBusy = $state(false);
  let launchError = $state('');

  function endpoint(): Endpoint {
    return { base_url: app.inferenceUrl, headers: app.inferenceHeaders(), model: null };
  }

  async function load() {
    try {
      rows = await taskRunApi.list(projectId);
      for (const id of Object.keys(open)) if (open[id]) open[id] = await taskRunApi.get(id).catch(() => null);
      error = '';
    } catch (e) { error = errorText(e); }
  }

  async function loadModels() {
    if (!app.gatewayRunning) return;
    try {
      const res = await fetch(`${app.inferenceUrl.replace(/\/$/, '')}/models`, { headers: app.inferenceHeaders() });
      const body = await res.json() as { data?: { id: string }[] };
      gatewayModels = (body.data ?? []).map((m) => m.id).sort();
    } catch { gatewayModels = []; }
  }

  onMount(() => {
    void load();
    void loadModels();
    // A run moves on its own; poll quickly while one is active.
    const timer = setInterval(() => { if (!busy) void load(); }, 1500);
    return () => clearInterval(timer);
  });

  async function act(fn: () => Promise<unknown>) {
    busy = true; error = ''; notice = '';
    try { await fn(); await load(); } catch (e) { error = errorText(e); } finally { busy = false; }
  }

  async function toggle(id: string) {
    open[id] = open[id] ? null : await taskRunApi.get(id).catch(() => null);
  }

  function openEditor(kind: RunKind, detail: TaskRunDetail | null) {
    editingId = detail?.run.id ?? null;
    draft = detail
      ? { project_id: projectId, kind: detail.run.kind, title: detail.run.title, goal: detail.run.goal, mode: detail.run.mode, policy: structuredClone($state.snapshot(detail.run.policy)), files: detail.run.file_refs.map((f) => f.path), steps: detail.steps.map((s) => ({ title: s.title, prompt: s.prompt, gate: s.gate })) }
      : emptyDraft(projectId, kind);
    if (!detail && kind === 'analysis') {
      stepsTextValue = ['Inspect the referenced files and list what they contain.', 'List the key findings with evidence from the files.', '! Write a short summary with open questions.'].join('\n');
    } else stepsTextValue = stepsText(draft.steps);
    modelsText = draft.policy.models.join('\n') || gatewayModels[0] || '';
    filesText = draft.files.join('\n');
    minContext = draft.policy.min_context ? String(draft.policy.min_context) : '';
    formError = '';
    editOpen = true;
    void loadModels();
  }

  async function save() {
    busy = true; formError = '';
    try {
      const body: TaskRunDraft = {
        ...$state.snapshot(draft),
        policy: { ...$state.snapshot(draft.policy), models: lines(modelsText), min_context: Number(minContext) > 0 ? Number(minContext) : null },
        files: lines(filesText),
        steps: parseSteps(stepsTextValue),
      };
      if (editingId) await taskRunApi.update(editingId, body); else await taskRunApi.create(body);
      editOpen = false;
      await load();
    } catch (e) { formError = errorText(e); } finally { busy = false; }
  }

  async function beginLaunch(row: TaskRunRow, step: boolean) {
    launch = { row, step };
    checks = []; compat = []; launchError = '';
    chosenModel = row.policy.models[0] ?? '';
    await refreshLaunch();
  }

  async function refreshLaunch() {
    if (!launch) return;
    launchBusy = true; launchError = '';
    try {
      const ep = endpoint();
      [checks, compat] = await Promise.all([
        taskRunApi.preflight(launch.row.id, ep),
        taskRunApi.compat(launch.row.id, ep, chosenModel || null),
      ]);
    } catch (e) { launchError = errorText(e); } finally { launchBusy = false; }
  }

  async function confirmLaunch() {
    const l = launch;
    if (!l) return;
    launchBusy = true; launchError = '';
    try {
      const row = l.row;
      if (chosenModel && chosenModel !== row.policy.models[0]) {
        // Resuming on another model: it goes first, the others stay as fallbacks.
        const detail = await taskRunApi.get(row.id);
        const models = [chosenModel, ...row.policy.models.filter((m) => m !== chosenModel)];
        await taskRunApi.update(row.id, {
          project_id: projectId, kind: row.kind, title: row.title, goal: row.goal, mode: row.mode,
          policy: { ...row.policy, models }, files: row.file_refs.map((f) => f.path),
          steps: detail.steps.map((s) => ({ title: s.title, prompt: s.prompt, gate: s.gate })),
        });
      }
      await taskRunApi.start(row.id, endpoint(), l.step);
      launch = null;
      await load();
    } catch (e) { launchError = errorText(e); } finally { launchBusy = false; }
  }

  async function exportRun(row: TaskRunRow) {
    await act(async () => {
      const r = await taskRunApi.exportBundle(row.id, embedFiles);
      if (r) notice = t('tr.exported', { path: r.path, steps: r.steps, files: r.embedded_files });
    });
  }

  async function importRun() {
    await act(async () => {
      const r = await taskRunApi.importBundle(projectId);
      if (r) notice = t('tr.imported', { title: r.run.title, restored: r.restored_files });
    });
  }

  const statusKey = (s: RunStatus) => ({ draft: 'tr.status_draft', running: 'tr.status_running', paused: 'tr.status_paused', awaiting_approval: 'tr.status_awaiting_approval', completed: 'tr.status_completed', failed: 'tr.status_failed', interrupted: 'tr.status_interrupted' } as const)[s];
  const stepKey = (s: StepStatus) => ({ pending: 'tr.step_pending', running: 'tr.step_running', done: 'tr.step_done', skipped: 'tr.step_skipped', failed: 'tr.step_failed' } as const)[s];

  function tally(r: TaskRunRow): string {
    return t('tr.tally', { input: r.tokens_in, output: r.tokens_out, cost: formatCost(r.cost_micro) || t('tr.cost_free') });
  }

  function reasons(text: string | null): string[] {
    return text ? text.split('\n').filter(Boolean).map((l) => errorText(l)) : [];
  }

  function eventText(e: { kind: string; message: string }): string {
    return e.message ? `${e.kind}: ${errorText(e.message)}` : e.kind;
  }

  function tools(b: boolean | null | undefined): string {
    return b === true ? t('tr.yes') : b === false ? t('tr.no') : t('tr.unknown');
  }
</script>

<section class="mt-6 flex flex-col gap-2" aria-labelledby="proj-taskruns">
  <h3 id="proj-taskruns" class="text-sm font-semibold">{t('tr.title')}</h3>
  <p class="text-xs text-muted-foreground">{t('tr.hint')}</p>
  <p class="text-xs text-muted-foreground">{t('tr.agents_note')}</p>
  {#if !app.gatewayRunning}<p class="text-xs text-warning">{t('tr.gateway_off')}</p>{/if}
  {#if error}<p class="text-sm text-destructive" role="alert">{error}</p>{/if}
  {#if notice}<p class="text-sm text-muted-foreground" role="status">{notice}</p>{/if}
  {#if !archived}
    <div class="flex flex-wrap items-center gap-2">
      <Button size="sm" variant="outline" disabled={busy} onclick={() => openEditor('agent_task', null)}><Plus />{t('tr.new_agent')}</Button>
      <Button size="sm" variant="outline" disabled={busy} onclick={() => openEditor('analysis', null)}><Plus />{t('tr.new_analysis')}</Button>
      <Button size="sm" variant="outline" disabled={busy} onclick={importRun}><Upload />{t('tr.import')}</Button>
      <label class="flex items-center gap-1 text-xs text-muted-foreground"><input type="checkbox" bind:checked={embedFiles} />{t('tr.embed_files')}</label>
    </div>
  {/if}
  {#if rows.length === 0}<p class="text-xs text-muted-foreground">{t('tr.empty')}</p>{/if}
  <ul class="flex flex-col gap-2">
    {#each rows as r (r.id)}
      {@const detail = open[r.id]}
      <li class="rounded-lg border border-border p-2 text-xs">
        <div class="flex flex-wrap items-center gap-2">
          <button type="button" class="flex items-center gap-1 text-left text-sm font-medium" aria-expanded={!!detail} onclick={() => toggle(r.id)}>
            {#if detail}<ChevronDown class="size-4" />{:else}<ChevronRight class="size-4" />{/if}{r.title}
          </button>
          <Badge variant="outline">{t(r.kind === 'analysis' ? 'tr.kind_analysis' : 'tr.kind_agent_task')}</Badge>
          <Badge variant={r.status === 'failed' || r.status === 'interrupted' ? 'destructive' : 'secondary'}>{t(statusKey(r.status))}</Badge>
          <span class="text-muted-foreground">{t('tr.progress', { done: r.steps_done, total: r.steps_total })} · {tally(r)}</span>
          {#if r.active}<Badge variant="outline">{t('tr.active_here')}</Badge>{/if}
        </div>
        {#if r.imported_from}<p class="mt-1 text-muted-foreground">{t('tr.imported_from', { from: r.imported_from })}</p>{/if}
        {#if r.error}<p class="mt-1 text-destructive">{errorText(r.error)}</p>{/if}
        <div class="mt-2 flex flex-wrap gap-1">
          {#if r.active || r.status === 'running'}
            <Button size="xs" variant="outline" disabled={busy} onclick={() => act(() => taskRunApi.stop(r.id))}><Square />{t('tr.stop')}</Button>
          {:else if r.status !== 'completed' && !archived}
            <Button size="xs" disabled={busy || r.status === 'awaiting_approval'} onclick={() => beginLaunch(r, false)}><Play />{t(r.status === 'draft' ? 'tr.start' : 'tr.resume')}</Button>
            <Button size="xs" variant="outline" disabled={busy || r.status === 'awaiting_approval'} onclick={() => beginLaunch(r, true)}><StepForward />{t('tr.step_forward')}</Button>
          {/if}
          {#if !r.active && r.status !== 'running'}
            <Button size="xs" variant="ghost" disabled={busy} onclick={() => exportRun(r)}><Download />{t('tr.export')}</Button>
            {#if ['draft', 'paused', 'interrupted', 'failed'].includes(r.status) && !archived}
              <Button size="xs" variant="ghost" disabled={busy} onclick={async () => { const d = await taskRunApi.get(r.id); openEditor(r.kind, d); }}><Pencil />{t('tr.edit')}</Button>
            {/if}
            <Button size="xs" variant="ghost" disabled={busy} aria-label={t('tr.delete')} title={t('tr.delete')} onclick={() => { if (confirm(t('tr.delete_confirm'))) void act(() => taskRunApi.remove(r.id)); }}><Trash2 /></Button>
          {/if}
        </div>
        {#if detail}
          <div class="mt-3 flex flex-col gap-3">
            {#if r.goal}<p class="whitespace-pre-wrap text-muted-foreground">{r.goal}</p>{/if}
            <div>
              <h4 class="mb-1 font-semibold">{t('tr.sec_steps')}</h4>
              <ol class="flex flex-col gap-2">
                {#each detail.steps as s (s.idx)}
                  <li class="rounded-md border border-border p-2">
                    <div class="flex flex-wrap items-center gap-2">
                      <span class="font-medium">{s.idx + 1}. {s.title}</span>
                      <Badge variant={s.status === 'failed' ? 'destructive' : 'outline'}>{t(stepKey(s.status))}</Badge>
                      {#if s.gate}<Badge variant="outline">{t('tr.gate_badge')}</Badge>{/if}
                      {#if s.status === 'done'}
                        <span class="text-muted-foreground">{t('tr.model_used', { model: s.model ?? '', attempts: s.attempts })}</span>
                        {#if !r.active && !archived}
                          <Button size="xs" variant="ghost" disabled={busy} onclick={() => act(() => taskRunApi.rewind(r.id, s.idx))}><RotateCcw />{t('tr.rewind_here')}</Button>
                        {/if}
                      {/if}
                      {#if s.status === 'failed' && !r.active}
                        <Button size="xs" variant="ghost" disabled={busy} onclick={() => act(() => taskRunApi.skip(r.id, s.idx))}>{t('tr.skip')}</Button>
                      {/if}
                    </div>
                    <p class="mt-1 whitespace-pre-wrap text-muted-foreground">{s.prompt}</p>
                    {#if s.approval === 'pending' && r.status === 'awaiting_approval'}
                      <p class="mt-1 text-warning">{t('tr.gate_pending')}</p>
                      <div class="mt-1 flex gap-1">
                        <Button size="xs" disabled={busy} onclick={() => act(() => taskRunApi.approve(r.id, s.idx, true))}><Check />{t('tr.approve')}</Button>
                        <Button size="xs" variant="outline" disabled={busy} onclick={() => act(() => taskRunApi.approve(r.id, s.idx, false))}><X />{t('tr.deny')}</Button>
                      </div>
                    {/if}
                    {#if s.output}<pre class="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded bg-muted/50 p-2">{s.output}</pre>{/if}
                    {#if s.fallback_reason}
                      <div class="mt-1 text-warning">
                        <p>{t('tr.fallback_used')}</p>
                        <ul class="list-disc pl-4">{#each reasons(s.fallback_reason) as line}<li>{line}</li>{/each}</ul>
                      </div>
                    {/if}
                    {#if s.error}<p class="mt-1 text-destructive">{errorText(s.error)}</p>{/if}
                  </li>
                {/each}
              </ol>
            </div>
            {#if detail.files.length}
              <div>
                <h4 class="mb-1 font-semibold">{t('tr.sec_files')}</h4>
                <ul>{#each detail.files as f (f.path)}<li class="flex items-center gap-2"><code>{f.path}</code><Badge variant={f.state === 'present' ? 'outline' : 'destructive'}>{t(`tr.file_${f.state}` as const)}</Badge></li>{/each}</ul>
              </div>
            {/if}
            <details>
              <summary class="cursor-pointer font-semibold">{t('tr.sec_events')}</summary>
              <ul class="mt-1 max-h-40 overflow-auto font-mono">
                {#each detail.events as e}<li>{new Date(e.at).toLocaleTimeString()} {e.step_idx !== null ? `#${e.step_idx + 1} ` : ''}{eventText(e)}</li>{/each}
              </ul>
            </details>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
</section>

<Dialog.Root open={!!launch} onOpenChange={(v) => { if (!v && !launchBusy) launch = null; }}>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{launch ? launch.row.title : ''}</Dialog.Title>
      <Dialog.Description>{t('tr.sec_checks')}</Dialog.Description>
    </Dialog.Header>
    {#if launch}
      <label class="flex flex-col gap-1 text-sm">{t('tr.resume_on')}
        <select class="h-8 rounded-md border border-input bg-background px-2 text-sm" bind:value={chosenModel} onchange={refreshLaunch} disabled={launchBusy}>
          {#each [...new Set([...launch.row.policy.models, ...gatewayModels])] as m (m)}<option value={m}>{m}</option>{/each}
        </select>
      </label>
      <ul class="flex flex-col gap-1 text-xs">
        {#each checks as c (c.model)}
          <li class={c.ok ? '' : 'text-warning'}>
            {#if c.ok}{t('tr.check_ok', { model: c.model })}{c.info ? ` (${t('tr.check_info', { context: c.info.context ?? t('tr.unknown'), tools: tools(c.info.tools) })})` : ''}{:else}{errorText(c.reason ?? '')}{/if}
          </li>
        {/each}
      </ul>
      <h4 class="text-sm font-semibold">{t('tr.sec_compat')}</h4>
      <ul class="flex flex-col gap-1 text-xs">
        {#each compat as n}<li class={n.level === 'warn' ? 'text-warning' : 'text-muted-foreground'}>{t(n.level === 'warn' ? 'tr.compat_warn' : 'tr.compat_info')}: {errorText(n.message)}</li>
        {:else}<li class="text-muted-foreground">{t('tr.compat_none')}</li>{/each}
      </ul>
      {#if launchError}<p class="text-sm text-destructive" role="alert">{launchError}</p>{/if}
      <Dialog.Footer>
        <Button variant="outline" disabled={launchBusy} onclick={() => (launch = null)}>{t('tr.f_cancel')}</Button>
        <Button disabled={launchBusy || !app.gatewayRunning} onclick={confirmLaunch}>{t(launch.step ? 'tr.step_forward' : launch.row.status === 'draft' ? 'tr.start' : 'tr.resume')}</Button>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={editOpen}>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{t(draft.kind === 'analysis' ? 'tr.new_analysis' : 'tr.new_agent')}</Dialog.Title>
      <Dialog.Description>{t('tr.hint')}</Dialog.Description>
    </Dialog.Header>
    <form class="flex flex-col gap-3" onsubmit={(e) => { e.preventDefault(); void save(); }}>
      <label class="flex flex-col gap-1 text-sm">{t('tr.f_title')}<Input bind:value={draft.title} required maxlength={120} disabled={busy} /></label>
      <label class="flex flex-col gap-1 text-sm">{t('tr.f_goal')}<Textarea bind:value={draft.goal} rows={2} disabled={busy} /></label>
      <div class="flex flex-col gap-1 text-sm">
        <label for="tr-models">{t('tr.f_models')}</label>
        <Textarea id="tr-models" bind:value={modelsText} rows={3} disabled={busy} />
        {#if gatewayModels.length}<p class="text-xs text-muted-foreground">{gatewayModels.slice(0, 12).join(', ')}</p>{/if}
        <p class="text-xs text-muted-foreground">{t('tr.f_models_hint')}</p>
      </div>
      <div class="grid grid-cols-2 gap-3 text-sm">
        <label class="flex flex-col gap-1">{t('tr.f_min_context')}<Input bind:value={minContext} inputmode="numeric" disabled={busy} /></label>
        <label class="flex flex-col gap-1">{t('tr.f_retries')}<Input type="number" min="0" max="8" bind:value={draft.policy.max_retries} disabled={busy} /></label>
        <label class="flex flex-col gap-1">{t('tr.f_timeout')}<Input type="number" min="5" max="3600" bind:value={draft.policy.timeout_secs} disabled={busy} /></label>
        <label class="flex flex-col gap-1">{t('tr.f_max_tokens')}<Input type="number" min="16" max="32768" bind:value={draft.policy.max_tokens} disabled={busy} /></label>
      </div>
      <label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={draft.policy.require_tools} />{t('tr.f_require_tools')}</label>
      <label class="flex items-center gap-2 text-sm"><input type="checkbox" checked={draft.mode === 'step'} onchange={(e) => (draft.mode = e.currentTarget.checked ? 'step' : 'auto')} />{t('tr.f_step_mode')}</label>
      <div class="flex flex-col gap-1 text-sm">
        <label for="tr-files">{t('tr.f_files')}</label>
        <Textarea id="tr-files" bind:value={filesText} rows={2} disabled={busy} />
        <p class="text-xs text-muted-foreground">{t('tr.f_files_hint')}</p>
      </div>
      <div class="flex flex-col gap-1 text-sm">
        <label for="tr-steps">{t('tr.f_steps')}</label>
        <Textarea id="tr-steps" bind:value={stepsTextValue} rows={5} disabled={busy} />
        <p class="text-xs text-muted-foreground">{t('tr.f_steps_hint')}</p>
      </div>
      {#if formError}<p class="text-sm text-destructive" role="alert">{formError}</p>{/if}
      <Dialog.Footer>
        <Button type="button" variant="outline" disabled={busy} onclick={() => (editOpen = false)}>{t('tr.f_cancel')}</Button>
        <Button type="submit" disabled={busy || !draft.title.trim() || !stepsTextValue.trim()}>{t('tr.f_save')}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
