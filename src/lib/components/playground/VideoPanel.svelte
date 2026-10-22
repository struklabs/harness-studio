<script lang="ts">
  import { videoQuestion, videoFormatsFor, videoMime, videoPreview, waitForVideo } from './video';
  import { definitionFile } from './modelModes';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import GenerationField from './GenerationField.svelte';
  import QualityPresets from './QualityPresets.svelte';
  import MediaFilePicker from './MediaFilePicker.svelte';
  import { generationPreset, effectiveFrames, videoDuration, videoFormats, inRange, validSize, matchingEstimate, type GenerationEngine, type Quality } from './generationParameters';
  import { generationError } from './generationError';
  import { BackendError } from '#lib/errors.js';
  import { VIDEO_FORMATS, type VideoFormat } from './video';
  import { onDestroy } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { projects } from '#lib/stores/projects.svelte.js';
  import { fileToProject, blobToBase64, outputName } from './projectOutputs';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import JobWaitNote from '../JobWaitNote.svelte';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { localizeError } from '#lib/network.js';
  let { model, task, frames = $bindable(9), size = $bindable('256x256') }: { model: string; task?: string; frames?: number; size?: string } = $props();
  let understanding = $derived(task === 'video-text-to-text');
  let prompt = $state('');
  /** Frames the engine sampled from the clip for the last answer (from `x-harness-video-frames`). */
  let sampledFrames = $state(0);
  let media = $state('');
  /** Only the containers this model's engine writes (sd.cpp has no mp4). */
  let formats = $derived.by(() => {
    const def = domain.definitions.find((d) => d.alias === model);
    return videoFormatsFor(def ? definitionFile(def, library.models) : undefined);
  });
  let steps = $state(20);
  let format = $state<VideoFormat>('mp4');
  let filename = $state('');
  let fps = $state(16);
  let file = $derived.by(() => { const def = domain.definitions.find(d => d.alias === model); return def ? definitionFile(def, library.models) : undefined; });
  let engine = $derived<GenerationEngine>(file?.kind === 'image' ? 'sdcpp' : file?.kind === 'pipeline' ? 'diffusers' : 'unknown');
  let ltx = $derived(/ltx/i.test(`${file?.gguf?.architecture ?? ''} ${file?.architecture ?? ''} ${file?.name ?? ''}`));
  let encodedFrames = $derived(effectiveFrames(frames, ltx));
  let duration = $derived(videoDuration(encodedFrames, fps));
  let validation = $derived({ frames: !inRange(frames, 1, 257, true), steps: !inRange(steps, 1, 500, true), fps: !inRange(fps, 1, 60, true), size: !validSize(size, engine, false) });
  let invalid = $derived(!understanding && Object.values(validation).some(Boolean));
  let samples = $state<{ model: string; signature: string; ms: number }[]>([]);
  let signature = $derived(JSON.stringify([steps, size, encodedFrames, fps, format, !!media]));
  let estimate = $derived(matchingEstimate(samples, model, signature));
  $effect(() => { if (!formats.includes(format)) format = formats[0]; });
  function preset(quality: Quality) {
    const p = generationPreset('video', engine, quality, file?.name ?? model);
    steps = p.steps; size = p.size; frames = p.frames;
  }
  let busy = $state(false);
  let progress = $state(0);
  let error = $state('');
  let answer = $state('');
  let output = $state('');
  let outputFormat = $state('mp4');
  let outputName_ = $state('');
  let stage = $state('');
  let elapsed = $state(0);
  let saved = $state(false);
  let timer: ReturnType<typeof setInterval> | undefined;
  let controller: AbortController | undefined;
  let job = '';
  async function api(path: string, init: RequestInit = {}) {
    const response = await fetch(`${app.inferenceUrl}${path}`, { ...init, headers: { 'Content-Type': 'application/json', ...app.inferenceHeaders() } });
    if (!response.ok) {
      const body = await response.json().catch(() => null);
      if (body?.error?.harness?.error_key) throw new BackendError(body.error.harness.error_key);
      throw new Error(body?.error?.message ?? `HTTP ${response.status}`);
    }
    return response;
  }
  async function pick(file: File) {
    if (busy) return;
    const types = understanding ? ['video/mp4', 'video/webm'] : ['image/png', 'image/jpeg', 'image/webp'];
    if (!types.includes(file.type)) { error = t('gen.error_run'); return; }
    if (file.size > 32 * 1024 * 1024) { error = t('vid.file_limit'); return; }
    try { media = `data:${file.type};base64,${await blobToBase64(file)}`; filename = file.name; error = ''; }
    catch (e) { error = generationError(e); }
  }
  function cancel() {
    controller?.abort();
    if (job) void api(`/videos/${job}`, { method: 'DELETE' }).catch(() => {});
    job = '';
  }
  onDestroy(() => { clearInterval(timer); cancel(); if (output) URL.revokeObjectURL(output); });
  async function run() {
    if (busy || invalid || !prompt.trim() || !model || !app.gatewayRunning || ((understanding || task === 'image-to-video') && !media)) return;
    const runSignature = signature; const runModel = model;
    const c = new AbortController(); controller = c;
    busy = true; error = ''; answer = ''; sampledFrames = 0; progress = 0; stage = 'queued'; elapsed = 0; saved = false;
    const started = Date.now(); clearInterval(timer); timer = setInterval(() => { elapsed = Math.round((Date.now() - started) / 1000); }, 1000);
    try {
      await engineInstall.runWithEngine(async () => {
        c.signal.throwIfAborted();
        if (understanding) {
          const r = await api('/chat/completions', { method: 'POST', signal: c.signal, body: JSON.stringify(videoQuestion(model, prompt, media)) });
          sampledFrames = Number(r.headers.get('x-harness-video-frames')) || 0;
          answer = (await r.json()).choices[0].message.content;
        } else {
          const r = await api('/videos', { method: 'POST', body: JSON.stringify({ model, prompt, frames, fps, steps, size, output_format: format, ...(media ? { input_reference: media } : {}) }) });
          const id = (await r.json()).id as string; job = id;
          if (c.signal.aborted) { cancel(); return; }
          await waitForVideo(async () => (await api(`/videos/${id}`, { signal: c.signal })).json(), {
            signal: c.signal,
            onProgress: (percent, status) => { progress = percent; stage = status; },
          });
          const raw = await (await api(`/videos/${id}/content`, { signal: c.signal })).blob();
          const blob = new Blob([raw], { type: videoMime(format) });
          if (output) URL.revokeObjectURL(output);
          output = URL.createObjectURL(blob);
          outputFormat = format;
          outputName_ = outputName(prompt, format, 'video');
          saved = !!projects.active?.folder;
          await fileToProject(outputName_, await blobToBase64(blob), { source: 'manual', model });
          samples = [...samples.slice(-19), { model: runModel, signature: runSignature, ms: Date.now() - started }];
          job = '';
        }
      });
    } catch (e) { if (!c.signal.aborted) error = engineInstall.autoFailureText(e) ?? generationError(e); }
    finally { busy = false; clearInterval(timer); }
  }
</script>

<div class="space-y-4 p-4">
  <h2 class="text-lg font-semibold">{t(understanding ? 'vid.questions' : 'vid.generate')}</h2>
  <p class="text-sm text-muted-foreground">{t(understanding ? 'vid.sampling' : 'vid.memory')}</p>
  <h3 class="text-sm font-semibold">{t('gen.basic')}</h3>
  {#if !understanding}<QualityPresets onselect={preset} {busy} seconds={estimate} />{/if}
  <GenerationField label={t(understanding ? 'vid.question' : 'vid.prompt')} id="video-prompt" help="gen.prompt">
    <Textarea id="video-prompt" bind:value={prompt} disabled={busy} />
  </GenerationField>
  <GenerationField label={t(understanding ? 'vid.input' : 'vid.start_image')} id="video-media" help="gen.video_media">
    <MediaFilePicker id="video-media" accept={understanding ? 'video/mp4,video/webm' : 'image/png,image/jpeg,image/webp'} name={filename} {busy} onpick={pick} onremove={() => { media = ''; filename = ''; }} />
  </GenerationField>
  {#if !understanding}
    <div class="grid items-start gap-4 sm:grid-cols-2">
      <GenerationField label={t('vid.frames')} id="video-frames" help="gen.frames">
        <Input id="video-frames" type="number" min={1} max={257} bind:value={frames} disabled={busy} aria-invalid={validation.frames} />
        {#if validation.frames}<p class="text-xs text-destructive" role="alert">{t('gen.invalid_number', { kind: t('gen.integer'), min: 1, max: 257 })}</p>{/if}
        {#if duration !== null}<p class="text-xs text-muted-foreground" role="status">{t('gen.duration', { frames: encodedFrames, fps, seconds: duration.toFixed(2) })}</p>{/if}
      </GenerationField>
      <GenerationField label={t('vid.size')} id="video-size" help="gen.video_size">
        <Input id="video-size" bind:value={size} disabled={busy} aria-invalid={validation.size} />
        {#if validation.size}<p class="text-xs text-destructive" role="alert">{t('gen.invalid_size', { multiple: 8, max: 4096 })}</p>{/if}
      </GenerationField>
    </div>
    <details class="rounded-md border border-border p-3" open={invalid}>
      <summary class="cursor-pointer text-sm font-semibold">{t('gen.advanced')}</summary>
      <div class="mt-4 grid items-start gap-4 sm:grid-cols-2">
        <GenerationField label={t('vid.format')} id="video-format" help="gen.format">
          <select id="video-format" class="h-9 rounded-md border border-input bg-background px-2 text-sm" bind:value={format} disabled={busy}>
            {#each VIDEO_FORMATS as f (f)}<option value={f} disabled={!formats.includes(f)}>{f}{!formats.includes(f) ? ` — ${t(engine === 'sdcpp' ? 'gen.format_sdcpp' : engine === 'diffusers' ? 'gen.format_python' : 'gen.format_unknown')}` : ''}</option>{/each}
          </select>
          <p class="text-xs text-muted-foreground">{t(engine === 'sdcpp' ? 'gen.format_sdcpp' : engine === 'diffusers' ? 'gen.format_python' : 'gen.format_unknown')}</p>
        </GenerationField>
        <GenerationField label={t('vid.steps')} id="video-steps" help="gen.steps">
          <Input id="video-steps" type="number" min={1} max={500} bind:value={steps} disabled={busy} aria-invalid={validation.steps} />
          {#if validation.steps}<p class="text-xs text-destructive" role="alert">{t('gen.invalid_number', { kind: t('gen.integer'), min: 1, max: 500 })}</p>{/if}
        </GenerationField>
        <GenerationField label={t('gen.fps_label')} id="video-fps" help="gen.fps">
          <Input id="video-fps" type="number" min={1} max={60} bind:value={fps} disabled={busy} aria-invalid={validation.fps} />
          {#if validation.fps}<p class="text-xs text-destructive" role="alert">{t('gen.invalid_number', { kind: t('gen.integer'), min: 1, max: 60 })}</p>{/if}
        </GenerationField>
      </div>
    </details>
  {/if}
  <Button onclick={run} disabled={busy || invalid || !app.gatewayRunning || !model || !prompt.trim() || ((understanding || task === 'image-to-video') && !media)}>{t('vid.run')}</Button>
  {#if busy}
    <div class="flex flex-wrap items-center gap-3" role="status">
      <Button variant="outline" onclick={cancel}>{t('vid.cancel')}</Button>
      <progress max="100" value={progress} aria-label={t('vid.progress')}></progress>
      <span class="text-xs text-muted-foreground">{t('vid.progress_line', { percent: progress, stage: t(stage === 'in_progress' ? 'vid.stage_running' : 'vid.stage_queued'), seconds: elapsed })}</span>
      <JobWaitNote {model} />
      {#if engineInstall.autoFamily}
        <span class="text-xs text-muted-foreground">{t('eng.auto_installing', { runtime: engineInstall.autoFamily })}</span>
      {/if}
    </div>
  {/if}
  {#if error}<p role="alert" class="text-destructive">{error}</p>{/if}
  {#if answer}<p class="whitespace-pre-wrap">{answer}</p>
    {#if sampledFrames}<p class="text-xs text-muted-foreground">{t('vid.sampled', { n: sampledFrames })}</p>{/if}
  {/if}
  {#if output}
    {@const kind = videoPreview(outputFormat)}
    {#if kind === 'video'}<video controls src={output}><track kind="captions" /></video>
    {:else if kind === 'image'}<img src={output} alt={t('vid.preview')} class="max-w-full" />
    {:else}<p class="text-sm text-muted-foreground">{t('vid.no_preview')}</p>{/if}
    <div class="flex flex-wrap items-center gap-3 text-sm">
      <a class="underline" href={output} download={outputName_}>{t('vid.save_file')}</a>
      {#if saved}<span class="text-muted-foreground">{t('vid.saved_to_project')}</span>{/if}
    </div>
  {/if}
</div>
