<script lang="ts">
  import TrustPermissions from "#lib/components/tools/TrustPermissions.svelte";
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { defaultLoadFit } from '#lib/fit.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';

  let { enabled = true }: { enabled?: boolean } = $props();

  type AgentKind = 'hermes' | 'openclaw';
  interface Detection { installed: boolean; version: string | null; executable: string | null; config_path: string; missing_dependencies: string[]; connected: boolean; model: string | null; installation: Task; managed: boolean }
  interface Model { alias: string; chat: boolean | null; tools: string; model_path: string | null; file_bytes: number | null; context_size: number | null; remote?: boolean }
  interface Task { status: string; output: string; error: string | null; id?: string; agent?: AgentKind; model?: string; prompt?: string; started_at?: number; finished_at?: number | null; tokens?: number | null; grants?: { read_folders: string[] }; audit?: string[] }
  let agent = $state<AgentKind>('hermes');
  let detection = $state<Detection | null>(null);
  let models = $state<Model[]>([]);
  let selected = $state('');
  let recommended = $state('');
  let task = $state<Task>({ status: 'idle', output: '', error: null });
  let prompt = $state('');
  let folders = $state('');
  let granted = $state(false);
  let history = $state<Task[]>([]);
  let opened = $state('');
  let handoff = $state<{ token_id: string; endpoint: string; config: Record<string, unknown> } | null>(null);
  let paperclipModel = $state('');
  let instance = $state('http://127.0.0.1:3100');
  let paperclipAgent = $state('');
  let credential = $state('');
  let notice = $state('');
  const viewing = $derived(history.find(entry => entry.id === opened));
  let error = $state('');
  let pending = $state(false);
  let disposed = false;
  let reading = false;
  async function refresh() {
    if (reading) return;
    reading = true;
    try {
      const asked = agent;
      const [d, s, h] = await Promise.all([invoke<Detection>('detect_agent', { agent: asked }), invoke<Task>('agent_task_status'), invoke<Task[]>('agent_task_history')]);
      if (!disposed && asked === agent) { detection = d; task = s; history = h; if (d.model) selected = d.model; }
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { reading = false; }
  }
  async function loadModels() {
    try {
      const available = (await invoke<Model[]>('list_agent_choices')).filter(m => m.chat !== false);
      const fits = await Promise.all(available.map(async m => {
        if (!m.model_path || !['verified', 'template_only'].includes(m.tools)) return null;
        try { return { model: m, fit: await defaultLoadFit({ model_path: m.model_path, model_size_bytes: m.file_bytes ?? 0, context_size: m.context_size ?? 8192, gpu_layers: 999, origin: 'gateway' }) }; }
        catch { return null; }
      }));
      const rank = ['full_gpu', 'tight', 'hybrid', 'cpu_heavy'];
      const candidates = fits.filter(x => x && rank.includes(x.fit.category));
      candidates.sort((a, b) => rank.indexOf(a!.fit.category) - rank.indexOf(b!.fit.category) || Number(b!.model.tools === 'verified') - Number(a!.model.tools === 'verified') || (b!.model.file_bytes ?? 0) - (a!.model.file_bytes ?? 0));
      if (!disposed) { models = available; recommended = candidates[0]?.model.alias ?? ''; if (!selected) selected = recommended || available[0]?.alias || ''; }
    } catch (e) { if (!disposed) error = errorText(e); }
  }
  onMount(() => {
    disposed = false;
    void refresh(); void loadModels();
    const timer = setInterval(() => void refresh(), 2000);
    return () => { disposed = true; clearInterval(timer); };
  });
  async function action(command: string, args?: Record<string, unknown>) {
    pending = true; error = '';
    try { await invoke(command, args); await refresh(); }
    catch (e) { error = errorText(e); }
    finally { pending = false; }
  }
  async function run() {
    if (!granted) return;
    const read_folders = folders.split('\n').map(s => s.trim()).filter(Boolean);
    granted = false;
    // The confirmation above is the user's grant for these folders: record it on the
    // Permissions page (revocable there) before the run, which only reads granted folders.
    if (read_folders.length) {
      try { await invoke('permissions_grant', { kind: 'agent', id: agent, capability: 'file', paths: read_folders }); }
      catch (e) { error = errorText(e); return; }
    }
    await action('start_agent_task', { prompt, grants: { read_folders }, agent });
  }
  function pick(kind: AgentKind) {
    if (kind === agent) return;
    agent = kind; detection = null; selected = ''; granted = false; error = '';
    void refresh().then(() => { if (!selected) selected = recommended || models[0]?.alias || ''; });
  }
  const label = (kind: AgentKind | undefined) => t(kind === 'openclaw' ? 'agents.openclaw' : 'agents.hermes');
  const modelLabel = (m: Model) => `${m.alias}${m.remote ? ` · ${t('agents.remote')}` : ''}${m.alias === recommended ? ` · ${t('agents.recommended')}` : ''}`;
  const installing = $derived(detection?.installation.status === 'running');
  async function paperclip(command: 'generate' | 'copy' | 'apply' | 'revoke') {
    pending = true; error = ''; notice = '';
    try {
      if (command === 'generate') handoff = await invoke('paperclip_handoff', { model: paperclipModel });
      if (command === 'copy' && handoff) { await navigator.clipboard.writeText(JSON.stringify(handoff.config, null, 2)); notice = t('agents.copied'); }
      if (command === 'apply' && handoff) { await invoke('apply_paperclip_config', { instance, agent: paperclipAgent, credential, config: handoff.config }); credential = ''; notice = t('agents.paperclip_applied'); }
      if (command === 'revoke' && handoff) { await invoke('revoke_paperclip_token', { id: handoff.token_id }); handoff = null; }
    } catch (e) { error = errorText(e); }
    finally { pending = false; }
  }
</script>

<section class="mt-6 space-y-4 border-t pt-5" aria-label={t('modules.agents')}>
  <h3 class="font-semibold">{t('agents.title')}</h3>
  <p class="text-sm text-muted-foreground">{t('agents.function')}</p>
  <TrustPermissions kind="agent" />
  <div class="flex flex-wrap items-center gap-2" role="radiogroup" aria-label={t('agents.agent')}>
    {#each ['hermes', 'openclaw'] as const as kind (kind)}
      <Button variant={agent === kind ? 'default' : 'outline'} role="radio" aria-checked={agent === kind} disabled={pending || task.status === 'running'} onclick={() => pick(kind)}>{label(kind)}</Button>
    {/each}
  </div>
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
  {#if pending}<p role="status" aria-live="polite" class="text-sm text-muted-foreground">{t('modules.working')}</p>{/if}
  {#if !enabled && task.status === 'running'}<Button variant="outline" onclick={() => action('stop_agent_task')}>{t('agents.stop')}</Button>{/if}
  {#if !enabled}<p role="status" class="text-sm text-muted-foreground">{t('modules.agent_unavailable')}</p>{/if}
  {#if detection}
    <p class="text-sm">{label(agent)} · {t('modules.version')}: {detection.version ?? t('modules.not_installed')}</p>
    {#if agent === 'openclaw' && !detection.installed && enabled}
      <p class="text-sm text-muted-foreground">{t('agents.openclaw_install_permission')}</p>
      <div class="flex flex-wrap gap-2">
        <Button disabled={pending || installing || detection.missing_dependencies.length > 0} onclick={() => { void invoke('install_openclaw').then(refresh).catch(e => { error = errorText(e); }); }}>{t('agents.install_openclaw')}</Button>
        {#if installing}<Button variant="outline" onclick={() => action('cancel_openclaw_install')}>{t('agents.cancel_install')}</Button>{/if}
      </div>
    {/if}
    {#if detection.installation.output && (uiLevel.advanced || detection.installation.status === 'failed')}
      <pre class="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-muted p-3 text-xs" aria-label={t('agents.install_output')}>{detection.installation.output}</pre>
    {/if}
    {#if detection.missing_dependencies.length}<p role="alert">{t('agents.missing_dependencies')}: {detection.missing_dependencies.map(d => d.startsWith('agents.') ? t(d as TKey) : d).join(', ')}</p>{/if}
    {#if uiLevel.advanced}
      <dl class="space-y-1 break-all text-xs text-muted-foreground">
        <dt>{t('agents.executable')}</dt><dd>{detection.executable ?? '—'}</dd>
        <dt>{t('agents.config')}</dt><dd>{detection.config_path}</dd>
        <dt>{t('agents.backup')}</dt><dd>{detection.config_path}.harness.bak</dd>
      </dl>
    {/if}
    {#if detection.installed && enabled}
      <p class="text-sm">{t(agent === 'openclaw' ? 'agents.openclaw_connect_permission' : 'agents.connect_permission')}</p>
      {#if app.gateway?.status.running && !app.gateway.status.require_token}<p class="text-xs text-muted-foreground">{t('agents.open_gateway')}</p>{/if}
      <label class="block text-sm" for="agent-model">{t('agents.model')}</label>
      <select id="agent-model" class="w-full rounded border bg-background p-2" bind:value={selected} disabled={pending || detection.connected}>
        {#each models as model (model.alias)}<option value={model.alias}>{modelLabel(model)}</option>{/each}
      </select>
      {#if models.find(m => m.alias === selected)?.remote}<p class="text-xs text-muted-foreground">{t('agents.remote_help')}</p>{/if}
      <p class="text-xs text-muted-foreground">{recommended ? t('agents.recommendation') : t('agents.no_recommendation')}</p>
      <p class="break-all text-xs text-muted-foreground">{t('agents.preview')}: {app.gateway?.status.endpoint?.base_url ?? t('agents.gateway_required')} · {selected}</p>
      <div class="flex flex-wrap gap-2">
        {#if detection.connected}
          <Button variant="outline" disabled={pending || task.status === 'running'} onclick={() => action('disconnect_agent', { agent })}>{t('agents.disconnect')}</Button>
        {:else}
          <Button disabled={pending || !selected} onclick={() => action('connect_agent', { model: selected, agent })}>{t('agents.connect')}</Button>
        {/if}
        <Button variant="outline" disabled={pending} onclick={() => { void refresh(); void loadModels(); }}>{t('modules.refresh')}</Button>
        {#if agent === 'openclaw' && detection.managed}
          <Button variant="outline" disabled={pending || installing || task.status === 'running'} onclick={() => action('remove_openclaw')}>{t('agents.remove_openclaw')}</Button>
        {/if}
      </div>
      <p class="text-sm text-muted-foreground">{t(agent === 'openclaw' ? 'agents.openclaw_task_permission' : 'agents.task_permission')}</p>
      <label class="block text-sm" for="agent-folders">{t('agents.read_folders')}</label>
      <textarea id="agent-folders" class="w-full rounded border bg-background p-2" bind:value={folders} oninput={() => granted = false} disabled={task.status === 'running'}></textarea>
      <p class="text-xs text-muted-foreground">{t(agent === 'openclaw' ? 'agents.openclaw_scope_limits' : 'agents.scope_limits')}</p>
      <label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={granted} disabled={task.status === 'running'} />{t('agents.grant_run')}</label>
      <label class="block text-sm" for="agent-prompt">{t('agents.task')}</label>
      <textarea id="agent-prompt" class="min-h-24 w-full rounded border bg-background p-2" bind:value={prompt} placeholder={t('agents.example')} maxlength="16384" disabled={task.status === 'running'}></textarea>
      <div class="flex gap-2">
        <Button disabled={pending || !granted || !detection.connected || !prompt.trim() || task.status === 'running' || detection.missing_dependencies.length > 0} onclick={run}>{t('agents.run')}</Button>
        <Button variant="outline" disabled={task.status !== 'running'} onclick={() => action('stop_agent_task')}>{t('agents.stop')}</Button>
      </div>
      <p role="status" aria-live="polite">{t(('agents.' + task.status) as TKey)}</p>
      {#if task.error}<p role="alert" class="text-destructive">{errorText(task.error)}</p>{/if}
      {#if task.output}<pre class="max-h-96 overflow-auto whitespace-pre-wrap break-words rounded bg-muted p-3 text-xs" aria-label={t('agents.output')}>{task.output}</pre>{/if}
    {/if}
  {/if}
  <h4 class="font-semibold">{t('agents.history')}</h4>
  <p class="text-xs text-muted-foreground">{t('agents.history_help')}</p>
  {#each history as entry (entry.id)}
    <div class="space-y-2 rounded border p-3 text-sm">
      <p class="break-words">{entry.prompt}</p>
      <p>{label(entry.agent)} · {entry.model} · {t(('agents.' + entry.status) as TKey)} · {entry.started_at ? new Date(entry.started_at).toLocaleString() : '—'}</p>
      <p>{t('agents.duration')}: {Math.max(0, Math.round(((entry.finished_at ?? Date.now()) - (entry.started_at ?? Date.now())) / 1000))} s · {t('agents.tokens')}: {entry.tokens ?? t('agents.unavailable')}</p>
      {#if entry.error}<p class="text-destructive">{errorText(entry.error)}</p>{/if}
      <Button variant="outline" onclick={() => opened = opened === entry.id ? '' : entry.id ?? ''}>{t('agents.reopen')}</Button>
      <Button variant="outline" disabled={pending || entry.status === 'running'} onclick={async () => { await action('delete_agent_task', { id: entry.id }); if (opened === entry.id) opened = ''; }}>{t('agents.delete_history')}</Button>
    </div>
  {/each}
  {#if viewing}
    <pre class="max-h-96 overflow-auto whitespace-pre-wrap break-words rounded bg-muted p-3 text-xs" aria-label={t('agents.output')}>{viewing.output}</pre>
    <h4 class="font-semibold">{t('agents.audit')}</h4>
    <pre class="max-h-64 overflow-auto whitespace-pre-wrap break-words text-xs">{JSON.stringify({ grants: viewing.grants, network: 'inference_only', started_at: viewing.started_at, finished_at: viewing.finished_at, events: viewing.audit }, null, 2)}</pre>
  {/if}
  <h4 class="font-semibold">Paperclip</h4>
  <p class="text-sm text-muted-foreground">{t('agents.paperclip_help')}</p>
  <label class="block text-sm" for="paperclip-model">{t('agents.model')}</label>
  <select id="paperclip-model" class="w-full rounded border bg-background p-2" bind:value={paperclipModel} disabled={pending || !!handoff}>
    {#each models as model (model.alias)}<option value={model.alias}>{modelLabel(model)}</option>{/each}
  </select>
  {#if models.find(m => m.alias === paperclipModel)?.remote}<p class="text-xs text-muted-foreground">{t('agents.remote_help')}</p>{/if}
  <Button disabled={pending || !paperclipModel || !!handoff} onclick={() => paperclip('generate')}>{t('agents.generate_config')}</Button>
  {#if handoff}
    <p class="break-all text-sm">{handoff.endpoint}</p>
    <p class="text-xs">{t('agents.secret_help')}</p>
    <pre class="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-muted p-3 text-xs">{JSON.stringify(handoff.config, null, 2)}</pre>
    <Button variant="outline" disabled={pending} onclick={() => paperclip('copy')}>{t('agents.copy_config')}</Button>
    <Button variant="outline" disabled={pending} onclick={() => paperclip('revoke')}>{t('agents.revoke_token')}</Button>
    <label class="block text-sm" for="paperclip-instance">{t('agents.paperclip_instance')}</label>
    <input id="paperclip-instance" class="w-full rounded border bg-background p-2" bind:value={instance} />
    <label class="block text-sm" for="paperclip-agent">{t('agents.paperclip_agent')}</label>
    <input id="paperclip-agent" class="w-full rounded border bg-background p-2" bind:value={paperclipAgent} />
    <label class="block text-sm" for="paperclip-credential">{t('agents.paperclip_credential')}</label>
    <input id="paperclip-credential" type="password" autocomplete="off" class="w-full rounded border bg-background p-2" bind:value={credential} />
    <Button disabled={pending || !paperclipAgent || !instance} onclick={() => paperclip('apply')}>{t('agents.apply_config')}</Button>
  {/if}
  {#if notice}<p role="status">{notice}</p>{/if}
</section>
