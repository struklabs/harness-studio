<script lang="ts">
  import { videoQuestion, videoFormatsFor, videoMime, videoPreview, waitForVideo } from './video';
  import { definitionFile } from './modelModes';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { onDestroy } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { projects } from '#lib/stores/projects.svelte.js';
  import { fileToProject, blobToBase64, outputName } from './projectOutputs';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import JobWaitNote from '../JobWaitNote.svelte';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { localizeError } from '#lib/network.js';
  let { model, task, frames = $bindable(9), size = $bindable('256x256') }: { model: string; task?: string; frames?: number; size?: string } = $props();
  let understanding = $derived(task === 'video-text-to-text');
  let prompt = $state('');
  let media = $state('');
  let format = $state('mp4');
  /** Only the containers this model's engine writes (sd.cpp has no mp4). */
  let formats = $derived.by(() => {
    const def = domain.definitions.find((d) => d.alias === model);
    return videoFormatsFor(def ? definitionFile(def, library.models) : undefined);
  });
  $effect(() => { if (!formats.includes(format as never)) format = formats[0]; });
  let steps = $state(20);
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
      const error = (await response.json()).error;
      throw new Error(error?.harness?.error_key ?? error?.message ?? response.statusText);
    }
    return response;
  }
  async function pick(event: Event) {
    const file = (event.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    if (file.size > 32 * 1024 * 1024) { error = t('vid.file_limit'); return; }
    media = `data:${file.type};base64,${await blobToBase64(file)}`;
  }
  function cancel() {
    controller?.abort();
    if (job) void api(`/videos/${job}`, { method: 'DELETE' }).catch(() => {});
    job = '';
  }
  onDestroy(() => { clearInterval(timer); cancel(); if (output) URL.revokeObjectURL(output); });
  async function run() {
    const c = new AbortController(); controller = c;
    busy = true; error = ''; answer = ''; progress = 0; stage = 'queued'; elapsed = 0; saved = false;
    const started = Date.now(); clearInterval(timer); timer = setInterval(() => { elapsed = Math.round((Date.now() - started) / 1000); }, 1000);
    try {
      await engineInstall.runWithEngine(async () => {
        c.signal.throwIfAborted();
        if (understanding) {
          const r = await api('/chat/completions', { method: 'POST', signal: c.signal, body: JSON.stringify(videoQuestion(model, prompt, media)) });
          answer = (await r.json()).choices[0].message.content;
        } else {
          const r = await api('/videos', { method: 'POST', body: JSON.stringify({ model, prompt, frames, steps, size, output_format: format, ...(media ? { input_reference: media } : {}) }) });
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
          job = '';
        }
      });
    } catch (e) { if (!c.signal.aborted) error = engineInstall.autoFailureText(e) ?? localizeError(e); }
    finally { busy = false; clearInterval(timer); }
  }
</script>

<div class="space-y-4 p-4">
  <h2 class="text-lg font-semibold">{t(understanding ? 'vid.questions' : 'vid.generate')}</h2>
  <p class="text-sm text-muted-foreground">{t(understanding ? 'vid.sampling' : 'vid.memory')}</p>
  <Label for="video-prompt">{t(understanding ? 'vid.question' : 'vid.prompt')}</Label>
  <Textarea id="video-prompt" bind:value={prompt} disabled={busy} />
  <Label for="video-media">{t(understanding ? 'vid.input' : 'vid.start_image')}</Label>
  <input id="video-media" type="file" accept={understanding ? 'video/mp4,video/webm' : 'image/png,image/jpeg,image/webp'} onchange={pick} disabled={busy} />
  {#if !understanding}
    <div class="flex flex-wrap gap-3">
      <Label>{t('vid.format')} <select bind:value={format} disabled={busy}>{#each formats as f (f)}<option>{f}</option>{/each}</select></Label>
      <Label for="video-frames">{t('vid.frames')}</Label><Input id="video-frames" type="number" min={1} max={129} bind:value={frames} disabled={busy} />
      <Label for="video-steps">{t('vid.steps')}</Label><Input id="video-steps" type="number" min={1} max={500} bind:value={steps} disabled={busy} />
      <Label for="video-size">{t('vid.size')}</Label><Input id="video-size" bind:value={size} disabled={busy} />
    </div>
  {/if}
  <Button onclick={run} disabled={busy || !app.gatewayRunning || !model || !prompt.trim() || ((understanding || task === 'image-to-video') && !media)}>{t('vid.run')}</Button>
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
  {#if answer}<p class="whitespace-pre-wrap">{answer}</p>{/if}
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
