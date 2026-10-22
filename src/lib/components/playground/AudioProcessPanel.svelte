<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import GenerationField from './GenerationField.svelte';
  import QualityPresets from './QualityPresets.svelte';
  import { inRange, audioPresetSteps, type Quality } from './generationParameters';
  import { generationError } from './generationError';
  import { t, type TKey } from '#lib/i18n.svelte.js';
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
  import { toast } from 'svelte-sonner';
  import DiarizationResultView from './DiarizationResult.svelte';
  import type { DiarizationResult } from './diarization.js';

  type OptionName = 'speaker_threshold' | 'speaker_min_frames' | 'speaker_pad_frames' | 'audio_chunk_duration_sec' | 'audio_chunk_overlap_sec' | 'edge_pad_duration_sec' | 'num_inference_steps' | 'guidance_scale' | 'ddim_eta' | 'seed' | 'input_sample_rate' | 'sampler_mode';
  interface OptionSpec { name: OptionName; min: number; max: number; default: number; integer: boolean; choices: string[] }
  interface Model { name: string; family: string; engine: 'audio.cpp' | 'mlx-audio'; options: OptionSpec[] }
  interface Output { name: 'enhanced' | 'vocals' | 'instrumental' | 'target' | 'residual'; audio_base64: string; mime: string; url: string }
  let overview = $state<{ models: Model[]; runtime_installed: boolean; mlx_separation?: boolean } | null>(null);
  let model = $state('');
  let file = $state<File | null>(null);
  let prompt = $state('');
  let options = $state<Record<string, number | string>>({});
  let diarization = $state<DiarizationResult | null>(null);
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

  function optionInvalid(option: OptionSpec) {
    const value = options[option.name] ?? (option.choices.length ? 'midpoint' : option.default);
    return option.choices.length ? !option.choices.includes(String(value)) : !inRange(value, option.min, option.max, option.integer);
  }
  let invalid = $derived(chosen?.options.some(optionInvalid) ?? false);
  let tunable = $derived(chosen?.options.find(o => o.name === 'num_inference_steps'));
  function preset(quality: Quality) {
    if (!tunable) return;
    options[tunable.name] = audioPresetSteps(quality, tunable.min, tunable.max);
  }
  async function load() {
    const token = ++loadToken;
    try {
      const next = await invoke<typeof overview>('get_audio_process_overview');
      if (token !== loadToken) return;
      overview = next;
      const usable = overview?.models.filter(m => m.engine === 'mlx-audio' || (enabled && overview?.runtime_installed)) ?? [];
      if (!usable.some(m => m.name === model)) model = usable[0]?.name ?? '';
    } catch (e) { error = generationError(e); }
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
  function clear() { for (const output of outputs) URL.revokeObjectURL(output.url); outputs = []; diarization = null; }
  onDestroy(() => { controller?.abort(); clear(); });
  $effect(() => { void model; options = {}; prompt = ''; untrack(() => { clear(); error = ''; }); });
  $effect(() => { void file; untrack(() => { clear(); error = ''; }); });
  async function run() {
    if (!file || !chosen || busy || invalid || !app.gatewayRunning || recState !== 'idle') return;
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
      const result: DiarizationResult | { task?: string; outputs: Omit<Output, 'url'>[] } = await response.json();
      if (result.task === 'diarization') {
        diarization = result as DiarizationResult;
        return;
      }
      outputs = result.outputs.map(o => {
        const bytes = Uint8Array.from(atob(o.audio_base64), c => c.charCodeAt(0));
        return { ...o, url: URL.createObjectURL(new Blob([bytes], { type: o.mime })) };
      });
    } catch (e) { if (!c.signal.aborted) error = generationError(e); }
    finally { if (controller === c) { controller = null; busy = false; } }
  }
  async function copyJson() {
    if (!diarization) return;
    try { await navigator.clipboard.writeText(JSON.stringify(diarization, null, 2)); toast.success(t('process.json_copied')); }
    catch (e) { error = generationError(e); }
  }
  async function downloadJson() {
    if (!diarization) return;
    try { await invoke('save_audio_process_json', { data: diarization }); }
    catch (e) { error = generationError(e); }
  }
  async function save(output: Output) {
    try { await invoke('save_processed_audio', { data: output.audio_base64, fileName: output.name }); }
    catch (e) { error = generationError(e); }
  }
</script>

<section class="mx-auto flex w-full max-w-[860px] flex-col gap-4 px-4 py-6" aria-labelledby="audio-process-title">
  <div class="flex items-center gap-2"><h2 id="audio-process-title" class="text-base font-semibold">{t('process.title')}</h2><span class="rounded border border-warning/40 px-2 py-0.5 text-xs text-warning">{t('process.experimental')}</span></div>
  <p class="text-sm text-muted-foreground">{t('process.description')}</p>
  {#if !enabled && !mlxModels.length}
    <p class="text-sm text-muted-foreground">{t('audio_cpp.needs_engine')}</p>
    <Button variant="outline" class="self-start" onclick={() => security.openSettings('runtime-installed', t('audio_cpp.experimental'))}>{t('audio_cpp.enable')}</Button>
  {:else if enabled && overview && !overview.runtime_installed && !mlxModels.length}
    <EngineNeeded family="audio.cpp" onInstalled={() => void load()} />
  {:else}
    {#if overview && !models.length}<p class="text-sm text-muted-foreground">{t(overview.mlx_separation ? 'process.no_models_mac' : 'process.no_models')}</p>{/if}
    <h3 class="text-sm font-semibold">{t('gen.basic')}</h3>
    <QualityPresets onselect={tunable ? preset : undefined} {busy} />
    <GenerationField id="process-model" label={t('process.model')} help="gen.model">
      <select id="process-model" class={controlClass} bind:value={model} disabled={busy || !models.length}>
        {#each models as m}<option value={m.name}>{m.name} · {m.engine} · {m.family}</option>{/each}
      </select>
    </GenerationField>
    <GenerationField id="process-file" label={t('process.file')} help="gen.audio_file">
      <AudioFilePicker id="process-file" label={t('process.file')} accept=".wav" wavOnly bind:file bind:recState {busy} contextKey={`process-${model}`} sampleRate={chosen && ['apollo', 'bs_roformer', 'mel_band_roformer'].includes(chosen.family) ? 44100 : 16000} />
    </GenerationField>
    <p class="text-xs text-muted-foreground">{t('process.limits')}</p>
    {#if chosen && ['apollo', 'bs_roformer', 'mel_band_roformer'].includes(chosen.family)}<p class="text-xs text-muted-foreground">{t('process.rate')}</p>{/if}
    {#if chosen?.family === 'sortformer_diar'}<p class="text-xs text-muted-foreground" role="note">{t('process.sortformer_limit')}</p>{/if}
    {#if chosen?.family === 'sam_audio'}
      <GenerationField id="process-prompt" label={t('process.prompt')} help="gen.prompt"><input id="process-prompt" class={controlClass} bind:value={prompt} maxlength="1024" disabled={busy} /></GenerationField>
      <p class="text-xs text-muted-foreground">{t('process.sam_hint')}</p>
    {/if}
    <details class="rounded-md border border-border p-3" open={invalid}><summary class="mb-3 cursor-pointer text-sm font-semibold">{t('gen.advanced')}</summary>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {#each chosen?.options ?? [] as option}
        <GenerationField id={`process-${option.name}`} label={t(`process.option.${option.name}`)} help={`gen.audio_${option.name}` as TKey}>
          {#if option.choices.length}<select id={`process-${option.name}`} aria-invalid={optionInvalid(option)} class={controlClass} disabled={busy} value={options[option.name] ?? 'midpoint'} onchange={e => options[option.name] = e.currentTarget.value}>{#each option.choices as choice}<option value={choice}>{choice}</option>{/each}</select>
          {:else if option.name === 'input_sample_rate'}<select id={`process-${option.name}`} aria-invalid={optionInvalid(option)} class={controlClass} disabled={busy} value={options[option.name] ?? 16000} onchange={e => options[option.name] = Number(e.currentTarget.value)}>{#each [8000, 12000, 16000, 24000] as rate}<option value={rate}>{rate} Hz</option>{/each}</select>
          {:else}<input id={`process-${option.name}`} aria-invalid={optionInvalid(option)} class={controlClass} type="number" min={option.min} max={option.max} step={option.integer ? 1 : 'any'} disabled={busy} value={options[option.name] ?? option.default} onchange={e => { if (e.currentTarget.value === '') delete options[option.name]; else options[option.name] = Number(e.currentTarget.value); }} />{/if}
          {#if !option.choices.length}<p class="text-xs text-muted-foreground">{t('gen.audio_range', { min: option.min, max: option.max, value: option.default })}</p>{/if}
          {#if optionInvalid(option)}<p class="text-xs text-destructive" role="alert">{t('gen.invalid_number', { kind: t(option.integer ? 'gen.integer' : 'gen.number'), min: option.min, max: option.max })}</p>{/if}
        </GenerationField>
      {/each}
    </div>
    </details>
    {#if !app.gatewayRunning}<p class="text-sm text-muted-foreground">{t('process.need_gateway')}</p>{/if}
    <div class="flex gap-2"><Button disabled={busy || invalid || recState !== 'idle' || !file || !chosen || !app.gatewayRunning || (chosen.family === 'sam_audio' && !prompt.trim())} onclick={run}>{t(busy ? 'process.running' : 'process.run')}</Button>{#if busy}<Button variant="outline" onclick={() => controller?.abort()}>{t('process.cancel')}</Button>{/if}<Button variant="ghost" disabled={busy} onclick={() => void load()}>{t('process.refresh')}</Button></div>
    {#if diarization}<DiarizationResultView result={diarization} onCopy={() => void copyJson()} onDownload={() => void downloadJson()} />{/if}
    {#each outputs as output}<div class="flex flex-wrap items-center gap-3 rounded-md border p-3"><span class="w-32 shrink-0 text-sm">{t(`process.output.${output.name}`)}</span><audio class="min-w-0 flex-1" controls src={output.url} aria-label={t(`process.output.${output.name}`)}></audio><Button variant="outline" size="sm" onclick={() => void save(output)}>{t('process.save')}</Button></div>{/each}
  {/if}
  {#if error}<p class="text-sm text-destructive" role="alert">{error}</p>{/if}
</section>
