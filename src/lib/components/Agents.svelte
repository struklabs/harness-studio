<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { defaultLoadFit } from '#lib/fit.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';

  interface Detection { installed: boolean; version: string | null; executable: string | null; config_path: string; missing_dependencies: string[]; connected: boolean; model: string | null; installation: Task }
  interface Model { alias: string; chat: boolean | null; tools: string; model_path: string | null; file_bytes: number | null; context_size: number | null }
  interface Task { status: string; output: string; error: string | null }
  let detection = $state<Detection | null>(null);
  let models = $state<Model[]>([]);
  let selected = $state('');
  let recommended = $state('');
  let task = $state<Task>({ status: 'idle', output: '', error: null });
  let prompt = $state('');
  let error = $state('');
  let pending = $state(false);
  let disposed = false;
  let reading = false;
  async function refresh() {
    if (reading) return;
    reading = true;
    try {
      const [d, s] = await Promise.all([invoke<Detection>('detect_agent'), invoke<Task>('agent_task_status')]);
      if (!disposed) { detection = d; task = s; if (d.model) selected = d.model; }
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { reading = false; }
  }
  async function loadModels() {
    try {
      const available = (await invoke<Model[]>('list_agent_models')).filter(m => m.chat !== false);
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
</script>

<section class="mt-6 space-y-4 border-t pt-5" aria-label={t('modules.agents')}>
  <h3 class="font-semibold">Hermes Agent</h3>
  <p class="text-sm text-muted-foreground">{t('agents.function')}</p>
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
  {#if detection}
    <p class="text-sm">{t('modules.version')}: {detection.version ?? t('modules.not_installed')}</p>
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
    {#if detection.installed}
      <p class="text-sm">{t('agents.connect_permission')}</p>
      {#if app.gateway?.status.running && !app.gateway.status.require_token}<p class="text-xs text-muted-foreground">{t('agents.open_gateway')}</p>{/if}
      <label class="block text-sm" for="agent-model">{t('agents.model')}</label>
      <select id="agent-model" class="w-full rounded border bg-background p-2" bind:value={selected} disabled={pending || detection.connected}>
        {#each models as model (model.alias)}<option value={model.alias}>{model.alias}{model.alias === recommended ? ` · ${t('agents.recommended')}` : ''}</option>{/each}
      </select>
      <p class="text-xs text-muted-foreground">{recommended ? t('agents.recommendation') : t('agents.no_recommendation')}</p>
      <p class="break-all text-xs text-muted-foreground">{t('agents.preview')}: {app.gateway?.status.endpoint?.base_url ?? t('agents.gateway_required')} · {selected}</p>
      <div class="flex flex-wrap gap-2">
        {#if detection.connected}
          <Button variant="outline" disabled={pending || task.status === 'running'} onclick={() => action('disconnect_agent')}>{t('agents.disconnect')}</Button>
        {:else}
          <Button disabled={pending || !selected} onclick={() => action('connect_agent', { model: selected })}>{t('agents.connect')}</Button>
        {/if}
        <Button variant="outline" disabled={pending} onclick={() => { void refresh(); void loadModels(); }}>{t('modules.refresh')}</Button>
      </div>
      <p class="text-sm text-muted-foreground">{t('agents.task_permission')}</p>
      <label class="block text-sm" for="agent-prompt">{t('agents.task')}</label>
      <textarea id="agent-prompt" class="min-h-24 w-full rounded border bg-background p-2" bind:value={prompt} placeholder={t('agents.example')} maxlength="16384" disabled={task.status === 'running'}></textarea>
      <div class="flex gap-2">
        <Button disabled={pending || !detection.connected || !prompt.trim() || task.status === 'running' || detection.missing_dependencies.length > 0} onclick={() => action('start_agent_task', { prompt })}>{t('agents.run')}</Button>
        <Button variant="outline" disabled={task.status !== 'running'} onclick={() => action('stop_agent_task')}>{t('agents.stop')}</Button>
      </div>
      <p role="status" aria-live="polite">{t(('agents.' + task.status) as TKey)}</p>
      {#if task.error}<p role="alert" class="text-destructive">{errorText(task.error)}</p>{/if}
      {#if task.output}<pre class="max-h-96 overflow-auto whitespace-pre-wrap break-words rounded bg-muted p-3 text-xs" aria-label={t('agents.output')}>{task.output}</pre>{/if}
    {/if}
  {/if}
</section>
