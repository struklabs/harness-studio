<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import EngineNeeded from '#lib/components/common/EngineNeeded.svelte';
  import AudioFilePicker from './AudioFilePicker.svelte';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { runtimesStore } from '#lib/stores/runtimes.svelte.js';
  import { describeErrorBody } from '#lib/components/playground/sse.js';
  import { BackendError } from '#lib/errors.js';

  type OptionName = 'audio_chunk_duration_sec' | 'audio_chunk_overlap_sec' | 'edge_pad_duration_sec' | 'num_inference_steps' | 'guidance_scale' | 'ddim_eta' | 'seed' | 'input_sample_rate' | 'sampler_mode';
  interface OptionSpec { name: OptionName; min: number; max: number; default: number; integer: boolean; choices: string[] }
  interface Model { name: string; family: string; engine: 'audio.cpp' | 'mlx-audio'; options: OptionSpec[] }
  interface Output { name: 'enhanced' | 'vocals' | 'instrumental' | 'target' | 'residual'; audio_base64: string; mime: string; url: string }
  let overview = $state<{ models: Model[]; runtime_installed: boolean; mlx_separation?: boolean } | null>(null);
  let model = $state('');
  let file = $state<File | null>(null);
  let prompt = $state('');
  let options = $state<Record<string, number | string>>({});
  let outputs = $state<Output[]>([]);
  let busy = $state(false);
  let recState = $state<'idle' | 'starting' | 'recording'>('idle');
  let loadToken = 0;
  let error = $state('');
  let controller: AbortController | null = null;
  let enabled = $derived(!!app.settings?.audio_cpp_enabled);
  // mlx-audio separators run without audio.cpp; its notices show only when nothing else can run.
  let mlxModels = $derived(overview?.models.filter(m => m.engine === 'mlx-audio') ?? []);
  let models = $derived(overview?.models.filter(m => m.engine === 'mlx-audio' || (enabled && overview?.runtime_installed)) ?? []);
  let chosen = $derived(models.find(m => m.name === model));
  const controlClass = 'h-8 rounded-md border border-input bg-background px-2 text-sm';

  async function load() {
    const token = ++loadToken;
    try {
      const next = await invoke<typeof overview>('get_audio_process_overview');
      if (token !== loadToken) return;
      overview = next;
      const usable = overview?.models.filter(m => m.engine === 'mlx-audio' || (enabled && overview?.runtime_installed)) ?? [];
      if (!usable.some(m => m.name === model)) model = usable[0]?.name ?? '';
    } catch (e) { error = localizeError(e); }
  }
  onMount(() => { void runtimesStore.load(); });
  $effect(() => {
    void enabled;
    void engineInstall.version;
    // A finished Hub download adds a processing model.
    void library.version;
    void runtimesStore.list;
    untrack(() => void load());
  });
  function clear() { for (const output of outputs) URL.revokeObjectURL(output.url); outputs = []; }
  onDestroy(() => { controller?.abort(); clear(); });
  $effect(() => { void model; options = {}; prompt = ''; });
  async function run() {
    if (!file || !chosen || busy || recState !== 'idle') return;
    error = '';
    clear();
    if (file.size > 32 * 1024 * 1024) { error = t('process.limits'); return; }
    busy = true;
    const c = new AbortController(); controller = c;
    try {
      const body = new FormData();
      body.append('model', model); body.append('file', file);
      if (chosen.family === 'sam_audio') body.append('prompt', prompt);
      for (const [key, value] of Object.entries(options)) body.append(key, String(value));
      // UniverSR requires an explicit effective bandwidth for 44.1/48 kHz uploads.
      if (chosen.family === 'universr' && options.input_sample_rate === undefined) body.append('input_sample_rate', '16000');
      const response = await fetch(`${app.inferenceUrl}/audio/process`, { method: 'POST', headers: app.inferenceHeaders(), body, signal: c.signal });
      if (!response.ok) {
        const text = await response.text();
        let key: unknown;
        try { key = JSON.parse(text)?.error?.harness?.error_key; } catch { /* plain text */ }
        if (typeof key === 'string') throw new BackendError(key);
        throw new Error(describeErrorBody(text) || `HTTP ${response.status}`);
      }
      const result: { outputs: Omit<Output, 'url'>[] } = await response.json();
      outputs = result.outputs.map(o => {
        const bytes = Uint8Array.from(atob(o.audio_base64), c => c.charCodeAt(0));
        return { ...o, url: URL.createObjectURL(new Blob([bytes], { type: o.mime })) };
      });
    } catch (e) { if (!c.signal.aborted) error = localizeError(e); }
    finally { if (controller === c) { controller = null; busy = false; } }
  }
  async function save(output: Output) {
    try { await invoke('save_processed_audio', { data: output.audio_base64, fileName: output.name }); }
    catch (e) { error = localizeError(e); }
  }
</script>

<section class="mx-auto flex w-full max-w-[860px] flex-col gap-4 px-4 py-6" aria-labelledby="audio-process-title">
  <div class="flex items-center gap-2"><h2 id="audio-process-title" class="text-base font-semibold">{t('process.title')}</h2><span class="rounded border border-warning/40 px-2 py-0.5 text-xs text-warning">{t('process.experimental')}</span></div>
  <p class="text-sm text-muted-foreground">{t('process.description')}</p>
  {#if !enabled && !mlxModels.length}
    <p class="text-sm text-muted-foreground">{t('audio_cpp.needs_engine')}</p>
    <Button variant="outline" class="self-start" onclick={() => security.openSettings('runtime-installed')}>{t('audio_cpp.enable')}</Button>
  {:else if enabled && overview && !overview.runtime_installed && !mlxModels.length}
    <EngineNeeded family="audio.cpp" onInstalled={() => void load()} />
  {:else}
    {#if overview && !models.length}<p class="text-sm text-muted-foreground">{t(overview.mlx_separation ? 'process.no_models_mac' : 'process.no_models')}</p>{/if}
    <label class="flex flex-col gap-1 text-sm" for="process-model">{t('process.model')}
      <select id="process-model" class={controlClass} bind:value={model} disabled={busy || !models.length}>
        {#each models as m}<option value={m.name}>{m.name} · {m.engine} · {m.family}</option>{/each}
      </select>
    </label>
    <div class="flex flex-col gap-1.5">
      <label class="text-sm" for="process-file">{t('process.file')}</label>
      <AudioFilePicker id="process-file" label={t('process.file')} accept=".wav" wavOnly bind:file bind:recState {busy} contextKey={`process-${model}`} sampleRate={chosen && ['apollo', 'bs_roformer', 'mel_band_roformer'].includes(chosen.family) ? 44100 : 16000} />
    </div>
    <p class="text-xs text-muted-foreground">{t('process.limits')}</p>
    {#if chosen && ['apollo', 'bs_roformer', 'mel_band_roformer'].includes(chosen.family)}<p class="text-xs text-muted-foreground">{t('process.rate')}</p>{/if}
    {#if chosen?.family === 'sam_audio'}
      <label class="flex flex-col gap-1 text-sm" for="process-prompt">{t('process.prompt')}<input id="process-prompt" class={controlClass} bind:value={prompt} maxlength="1024" disabled={busy} /></label>
      <p class="text-xs text-muted-foreground">{t('process.sam_hint')}</p>
    {/if}
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {#each chosen?.options ?? [] as option}
        <label class="flex flex-col gap-1 text-sm" for={`process-${option.name}`}>{t(`process.option.${option.name}`)}
          {#if option.choices.length}<select id={`process-${option.name}`} class={controlClass} disabled={busy} value={options[option.name] ?? 'midpoint'} onchange={e => options[option.name] = e.currentTarget.value}>{#each option.choices as choice}<option value={choice}>{choice}</option>{/each}</select>
          {:else if option.name === 'input_sample_rate'}<select id={`process-${option.name}`} class={controlClass} disabled={busy} value={options[option.name] ?? 16000} onchange={e => options[option.name] = Number(e.currentTarget.value)}>{#each [8000, 12000, 16000, 24000] as rate}<option value={rate}>{rate} Hz</option>{/each}</select>
          {:else}<input id={`process-${option.name}`} class={controlClass} type="number" min={option.min} max={option.max} step={option.integer ? 1 : 'any'} disabled={busy} value={options[option.name] ?? option.default} onchange={e => { if (e.currentTarget.value === '') delete options[option.name]; else options[option.name] = Number(e.currentTarget.value); }} />{/if}
        </label>
      {/each}
    </div>
    {#if !app.gatewayRunning}<p class="text-sm text-muted-foreground">{t('process.need_gateway')}</p>{/if}
    <div class="flex gap-2"><Button disabled={busy || recState !== 'idle' || !file || !chosen || !app.gatewayRunning || (chosen.family === 'sam_audio' && !prompt.trim())} onclick={run}>{t(busy ? 'process.running' : 'process.run')}</Button>{#if busy}<Button variant="outline" onclick={() => controller?.abort()}>{t('process.cancel')}</Button>{/if}<Button variant="ghost" disabled={busy} onclick={() => void load()}>{t('process.refresh')}</Button></div>
    {#each outputs as output}<div class="flex flex-wrap items-center gap-3 rounded-md border p-3"><span class="w-32 shrink-0 text-sm">{t(`process.output.${output.name}`)}</span><audio class="min-w-0 flex-1" controls src={output.url} aria-label={t(`process.output.${output.name}`)}></audio><Button variant="outline" size="sm" onclick={() => void save(output)}>{t('process.save')}</Button></div>{/each}
  {/if}
  {#if error}<p class="text-sm text-destructive" role="alert">{error}</p>{/if}
</section>
