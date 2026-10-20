<script lang="ts">
  // Projects: chats, outputs and agent tasks grouped under a name and a folder, with the folders
  // tools and agents may use. The Playground files new chats and outputs under the active one.
  import './common/polished-pages.css';
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { projects } from '#lib/stores/projects.svelte.js';
  import { convApi, type ConversationSummary } from './playground/conversations';
  import { listCoreTools, getMcpStatus } from '#lib/tools-client.js';
  import {
    projectApi, emptyInput, inputOf, folderLines, fileName, formatSize,
    type AgentTask, type Project, type ProjectInput, type ProjectOutput,
  } from '#lib/projects.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { FolderKanban, FileText, Image, AudioLines, File, FolderOpen, ExternalLink, Plus, Archive, ArchiveRestore, Trash2, Download, Pencil, MessageSquare, Bot } from '@lucide/svelte';

  let all = $state<Project[]>([]);
  let showArchived = $state(false);
  let selectedId = $state<string | null>(null);
  let chats = $state<ConversationSummary[]>([]);
  let outputs = $state<ProjectOutput[]>([]);
  let tasks = $state<AgentTask[]>([]);
  let error = $state('');
  let notice = $state('');
  let busy = $state(false);

  let editOpen = $state(false);
  let editing = $state<Project | null>(null);
  let form = $state<ProjectInput>(emptyInput());
  let extraFolders = $state('');
  let formError = $state('');
  let toolNames = $state<string[]>([]);
  let skillNames = $state<string[]>([]);
  let mcpNames = $state<string[]>([]);

  let removing = $state<Project | null>(null);
  let deleteFiles = $state(false);
  let agentPrompt = $state('');

  const selected = $derived(all.find((p) => p.id === selectedId) ?? null);
  const visible = $derived(all.filter((p) => showArchived || !p.archived));

  async function load() {
    try {
      all = await projectApi.list(true);
      if (selectedId && !all.some((p) => p.id === selectedId)) selectedId = null;
      selectedId ??= visible[0]?.id ?? null;
      error = '';
      void projects.load();
    } catch (e) { error = errorText(e); }
  }

  async function loadDetail(id: string) {
    try {
      const [c, o, a] = await Promise.all([
        convApi.list({ project_id: id, limit: 200 }),
        projectApi.outputs(id),
        projectApi.agentTasks(id).catch(() => [] as AgentTask[]),
      ]);
      if (selectedId !== id) return;
      chats = c; outputs = o; tasks = a;
    } catch (e) { error = errorText(e); }
  }

  $effect(() => {
    const id = selectedId;
    chats = []; outputs = []; tasks = [];
    if (id) void loadDetail(id);
  });

  onMount(() => {
    void load();
    // A running agent task finishes on its own; poll while the page is open.
    const timer = setInterval(() => { if (selectedId && !busy) void loadDetail(selectedId); }, 5000);
    return () => clearInterval(timer);
  });

  async function act(fn: () => Promise<unknown>) {
    busy = true; error = ''; notice = '';
    try { await fn(); } catch (e) { error = errorText(e); } finally { busy = false; }
  }

  async function openEditor(project: Project | null) {
    editing = project;
    form = project ? inputOf(project) : emptyInput();
    extraFolders = form.allowed_folders.join('\n');
    formError = '';
    editOpen = true;
    try {
      const [listing, skills, mcp] = await Promise.all([
        listCoreTools(),
        invoke<{ name: string }[]>('skills_list'),
        getMcpStatus().catch(() => []),
      ]);
      toolNames = listing.tools.filter((tl) => tl.source !== 'mcp').map((tl) => tl.name).sort();
      skillNames = skills.map((s) => s.name).sort();
      mcpNames = mcp.map((m) => m.name).sort();
    } catch { /* attachments stay as saved */ }
  }

  async function pickFolder(apply: (path: string) => void) {
    const path = await invoke<string | null>('pick_directory_dialog').catch(() => null);
    if (path) apply(path);
  }

  function toggle(list: string[], name: string): string[] {
    return list.includes(name) ? list.filter((n) => n !== name) : [...list, name];
  }

  async function save() {
    busy = true; formError = '';
    try {
      const input = { ...$state.snapshot(form), allowed_folders: folderLines(extraFolders) };
      const saved = editing ? await projectApi.update(editing.id, input) : await projectApi.create(input);
      editOpen = false;
      selectedId = saved.id;
      await load();
    } catch (e) { formError = errorText(e); }
    finally { busy = false; }
  }

  async function usein(project: Project, conversationId?: string) {
    await projects.setActive(project.id);
    if (conversationId) projects.pendingConversation = conversationId;
    app.requestedTab = 'playground';
  }

  async function remove() {
    const project = removing;
    if (!project) return;
    await act(async () => {
      const report = await projectApi.remove(project.id, deleteFiles);
      if (projects.activeId === project.id) await projects.setActive(null);
      removing = null;
      notice = t('proj.deleted', { chats: report.unassigned_chats, deleted: report.deleted_files, kept: report.kept_files });
      await load();
    });
  }

  async function exportZip(project: Project) {
    await act(async () => {
      const path = await projectApi.exportZip(project.id);
      if (path) notice = t('proj.exported', { path });
    });
  }

  function kindIcon(kind: string) {
    return kind === 'image' ? Image : kind === 'audio' ? AudioLines : kind === 'text' ? FileText : File;
  }

  function sourceLabel(o: ProjectOutput): string {
    const key = ({ chat: 'proj.source_chat', tool: 'proj.source_tool', image: 'proj.source_image', speech: 'proj.source_speech', agent: 'proj.source_agent', manual: 'proj.source_manual' } as const)[o.source] ?? 'proj.source_manual';
    return o.tool ? `${t(key)} · ${o.tool}` : t(key);
  }

  function chatTitle(id: string | null): string | null {
    if (!id) return null;
    return chats.find((c) => c.id === id)?.title || t('proj.untitled_chat');
  }
</script>

<div class="flex h-full min-h-0">
  <aside class="flex w-64 shrink-0 flex-col gap-2 border-r border-border p-3">
    <div class="flex items-center justify-between gap-2">
      <h1 class="text-sm font-semibold">{t('proj.title')}</h1>
      <Button size="xs" onclick={() => openEditor(null)} disabled={busy}><Plus />{t('proj.new')}</Button>
    </div>
    <label class="flex items-center gap-2 text-xs text-muted-foreground"><input type="checkbox" bind:checked={showArchived} />{t('proj.show_archived')}</label>
    <nav class="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto" aria-label={t('proj.title')}>
      {#each visible as p (p.id)}
        <button type="button" class="flex flex-col rounded-md px-2 py-1.5 text-left hover:bg-muted/60 {selectedId === p.id ? 'bg-muted/60' : ''}" aria-current={selectedId === p.id ? 'page' : undefined} onclick={() => (selectedId = p.id)}>
          <span class="flex items-center gap-1.5 truncate text-sm">{p.name}{#if projects.activeId === p.id}<Badge variant="secondary" class="text-[10px]">{t('proj.active')}</Badge>{/if}</span>
          <span class="text-xs text-muted-foreground">{t('proj.counts', { chats: p.chat_count, outputs: p.output_count })}{#if p.archived} · {t('proj.archived')}{/if}</span>
        </button>
      {:else}
        <p class="px-2 text-xs text-muted-foreground">{t('proj.empty')}</p>
      {/each}
    </nav>
  </aside>

  <main class="min-w-0 flex-1 overflow-y-auto p-6">
    {#if error}<p class="mb-3 text-sm text-destructive" role="alert">{error}</p>{/if}
    {#if notice}<p class="mb-3 text-sm text-muted-foreground" role="status">{notice}</p>{/if}
    {#if !selected}
      <div class="flex flex-col items-start gap-3">
        <FolderKanban class="size-8 text-muted-foreground" />
        <h2 class="text-lg font-semibold">{t('proj.title')}</h2>
        <p class="max-w-prose text-sm text-muted-foreground">{t('proj.intro')}</p>
        <Button onclick={() => openEditor(null)}><Plus />{t('proj.new')}</Button>
      </div>
    {:else}
      {@const p = selected}
      <header class="flex flex-col gap-2">
        <div class="flex flex-wrap items-center gap-2">
          <h2 class="text-lg font-semibold">{p.name}</h2>
          {#if p.archived}<Badge variant="outline">{t('proj.archived')}</Badge>{/if}
          {#if projects.activeId === p.id}<Badge variant="secondary">{t('proj.active')}</Badge>{/if}
        </div>
        {#if p.description}<p class="max-w-prose whitespace-pre-wrap text-sm text-muted-foreground">{p.description}</p>{/if}
        <div class="flex flex-wrap gap-2">
          {#if !p.archived}<Button size="sm" onclick={() => usein(p)}><MessageSquare />{t('proj.use_in_playground')}</Button>{/if}
          <Button size="sm" variant="outline" disabled={busy} onclick={() => openEditor(p)}><Pencil />{t('proj.edit')}</Button>
          <Button size="sm" variant="outline" disabled={busy} onclick={() => exportZip(p)}><Download />{t('proj.export')}</Button>
          <Button size="sm" variant="outline" disabled={busy} onclick={() => act(async () => { await projectApi.archive(p.id, !p.archived); if (!p.archived && projects.activeId === p.id) await projects.setActive(null); await load(); })}>
            {#if p.archived}<ArchiveRestore />{t('proj.unarchive')}{:else}<Archive />{t('proj.archive')}{/if}
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onclick={() => { removing = p; deleteFiles = false; }}><Trash2 />{t('proj.delete')}</Button>
        </div>
      </header>

      <section class="mt-6 flex flex-col gap-2" aria-labelledby="proj-folders">
        <h3 id="proj-folders" class="text-sm font-semibold">{t('proj.folders')}</h3>
        <p class="text-xs text-muted-foreground">{t('proj.folders_hint')}</p>
        {#if p.folder}
          <div class="flex flex-wrap items-center gap-2">
            <code class="break-all text-xs">{p.folder}</code>
            <Button size="xs" variant="ghost" onclick={() => act(() => projectApi.revealFolder(p.id))}><FolderOpen />{t('proj.reveal_folder')}</Button>
          </div>
        {:else}
          <p class="text-xs text-warning">{t('proj.no_folder')}</p>
        {/if}
        {#each p.allowed_folders as folder (folder)}<code class="break-all text-xs text-muted-foreground">{folder}</code>{/each}
        {#if p.tools.length || p.skills.length || p.mcp_servers.length}
          <div class="flex flex-wrap gap-1">
            {#each p.tools as name (name)}<Badge variant="outline">{t('proj.tool_badge', { name })}</Badge>{/each}
            {#each p.skills as name (name)}<Badge variant="outline">{t('proj.skill_badge', { name })}</Badge>{/each}
            {#each p.mcp_servers as name (name)}<Badge variant="outline">{t('proj.mcp_badge', { name })}</Badge>{/each}
          </div>
        {/if}
      </section>

      <section class="mt-6 flex flex-col gap-2" aria-labelledby="proj-outputs">
        <h3 id="proj-outputs" class="text-sm font-semibold">{t('proj.outputs')}</h3>
        {#if outputs.length === 0}<p class="text-xs text-muted-foreground">{t('proj.outputs_empty')}</p>{/if}
        <ul class="flex flex-col divide-y divide-border rounded-lg border border-border">
          {#each outputs as o (o.id)}
            {@const Icon = kindIcon(o.kind)}
            <li class="flex flex-wrap items-center gap-3 p-2 text-sm">
              <Icon class="size-4 shrink-0 text-muted-foreground" />
              <div class="flex min-w-0 flex-1 flex-col">
                <span class="truncate font-mono text-xs" title={o.path}>{fileName(o.path)}</span>
                <span class="text-xs text-muted-foreground">
                  {sourceLabel(o)}{#if o.model} · {o.model}{/if}{#if chatTitle(o.conversation_id)} · {chatTitle(o.conversation_id)}{/if} · {new Date(o.created_at).toLocaleString()} · {formatSize(o.size)}
                </span>
              </div>
              {#if o.exists === false}<Badge variant="outline">{t('proj.missing')}</Badge>{/if}
              <div class="flex gap-1">
                <Button size="xs" variant="ghost" disabled={o.exists === false} onclick={() => act(() => projectApi.openOutput(o.id))}><ExternalLink />{t('proj.open')}</Button>
                <Button size="xs" variant="ghost" disabled={o.exists === false} onclick={() => act(() => projectApi.revealOutput(o.id))}><FolderOpen />{t('proj.reveal')}</Button>
                <Button size="xs" variant="ghost" aria-label={t('proj.forget_output')} title={t('proj.forget_output')} onclick={() => act(async () => { await projectApi.forgetOutput(o.id); await loadDetail(p.id); await load(); })}><Trash2 /></Button>
              </div>
            </li>
          {/each}
        </ul>
      </section>

      <section class="mt-6 flex flex-col gap-2" aria-labelledby="proj-chats">
        <h3 id="proj-chats" class="text-sm font-semibold">{t('proj.chats')}</h3>
        {#if chats.length === 0}<p class="text-xs text-muted-foreground">{t('proj.chats_empty')}</p>{/if}
        <ul class="flex flex-col gap-1">
          {#each chats as c (c.id)}
            <li><button type="button" class="w-full truncate rounded-md px-2 py-1 text-left text-sm hover:bg-muted/60" disabled={p.archived} onclick={() => usein(p, c.id)}>{c.title || t('proj.untitled_chat')}</button></li>
          {/each}
        </ul>
      </section>

      <section class="mt-6 flex flex-col gap-2" aria-labelledby="proj-agent">
        <h3 id="proj-agent" class="text-sm font-semibold">{t('proj.agent_tasks')}</h3>
        <p class="text-xs text-muted-foreground">{t('proj.agent_hint')}</p>
        {#if !p.archived}
          <Textarea bind:value={agentPrompt} rows={3} aria-label={t('proj.agent_prompt')} placeholder={t('proj.agent_prompt')} />
          <div><Button size="sm" variant="outline" disabled={busy || !agentPrompt.trim()} onclick={() => act(async () => { await projectApi.startAgent(p.id, agentPrompt); agentPrompt = ''; await loadDetail(p.id); })}><Bot />{t('proj.agent_run')}</Button></div>
        {/if}
        <ul class="flex flex-col gap-2">
          {#each tasks as task (task.id)}
            <li class="rounded-lg border border-border p-2 text-xs">
              <div class="flex flex-wrap items-center gap-2">
                <Badge variant="outline">{task.status}</Badge>
                <span class="text-muted-foreground">{task.model} · {new Date(task.started_at * 1000).toLocaleString()}</span>
                {#if task.output.trim() && task.status !== 'running'}
                  <Button size="xs" variant="ghost" disabled={busy} onclick={() => act(async () => { await projectApi.saveAgentOutput(p.id, task.id); await loadDetail(p.id); await load(); })}>{t('proj.agent_save')}</Button>
                {/if}
              </div>
              <p class="mt-1 line-clamp-2">{task.prompt}</p>
              {#if task.error}<p class="mt-1 text-destructive">{errorText(task.error)}</p>{/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  </main>
</div>

<Dialog.Root bind:open={editOpen}>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{t(editing ? 'proj.edit' : 'proj.new')}</Dialog.Title>
      <Dialog.Description>{t('proj.form_description')}</Dialog.Description>
    </Dialog.Header>
    <form class="flex flex-col gap-4" onsubmit={(e) => { e.preventDefault(); void save(); }}>
      <label class="flex flex-col gap-1 text-sm">{t('proj.name')}<Input bind:value={form.name} required maxlength={120} disabled={busy} /></label>
      <label class="flex flex-col gap-1 text-sm">{t('proj.description')}<Textarea bind:value={form.description} rows={3} disabled={busy} /></label>
      <div class="flex flex-col gap-1 text-sm">
        <label for="proj-folder">{t('proj.folder')}</label>
        <div class="flex gap-2">
          <Input id="proj-folder" value={form.folder ?? ''} oninput={(e) => (form.folder = (e.currentTarget as HTMLInputElement).value || null)} disabled={busy} />
          <Button type="button" variant="outline" disabled={busy} onclick={() => pickFolder((path) => (form.folder = path))}>{t('proj.choose')}</Button>
        </div>
        <p class="text-xs text-muted-foreground">{t('proj.folder_hint')}</p>
      </div>
      <div class="flex flex-col gap-1 text-sm">
        <label for="proj-extra">{t('proj.extra_folders')}</label>
        <Textarea id="proj-extra" bind:value={extraFolders} rows={2} disabled={busy} />
        <div><Button type="button" size="xs" variant="outline" disabled={busy} onclick={() => pickFolder((path) => (extraFolders = [...folderLines(extraFolders), path].join('\n')))}>{t('proj.add_folder')}</Button></div>
        <p class="text-xs text-muted-foreground">{t('proj.extra_folders_hint')}</p>
      </div>
      {#each [
        { label: 'proj.attach_tools' as const, names: toolNames, key: 'tools' as const },
        { label: 'proj.attach_skills' as const, names: skillNames, key: 'skills' as const },
        { label: 'proj.attach_mcp' as const, names: mcpNames, key: 'mcp_servers' as const },
      ] as group (group.key)}
        {#if group.names.length || form[group.key].length}
          <fieldset class="flex flex-col gap-1 text-sm">
            <legend>{t(group.label)}</legend>
            <div class="flex max-h-32 flex-wrap gap-x-3 gap-y-1 overflow-y-auto">
              {#each [...new Set([...group.names, ...form[group.key]])] as name (name)}
                <label class="flex items-center gap-1 text-xs"><input type="checkbox" checked={form[group.key].includes(name)} onchange={() => (form[group.key] = toggle(form[group.key], name))} />{name}</label>
              {/each}
            </div>
          </fieldset>
        {/if}
      {/each}
      <p class="text-xs text-muted-foreground">{t('proj.attach_hint')}</p>
      {#if formError}<p class="text-sm text-destructive" role="alert">{formError}</p>{/if}
      <Dialog.Footer>
        <Button type="button" variant="outline" disabled={busy} onclick={() => (editOpen = false)}>{t('proj.cancel')}</Button>
        <Button type="submit" disabled={busy || !form.name.trim()}>{t('proj.save')}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={!!removing} onOpenChange={(v) => { if (!v && !busy) removing = null; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('proj.delete')}</Dialog.Title>
      <Dialog.Description>{t('proj.delete_confirm', { name: removing?.name ?? '' })}</Dialog.Description>
    </Dialog.Header>
    <label class="flex items-start gap-2 text-sm"><input type="checkbox" class="mt-1" bind:checked={deleteFiles} />{t('proj.delete_files')}</label>
    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={() => (removing = null)}>{t('proj.cancel')}</Button>
      <Button variant="destructive" disabled={busy} onclick={remove}>{t('proj.delete')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
