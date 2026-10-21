<script lang="ts">
  import { videoQuestion } from './video';
  import { onDestroy } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { fileToProject, blobToBase64, outputName } from './projectOutputs';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  let { model, task }: { model: string; task?: string } = $props();
  let understanding = $derived(task === 'video-text-to-text');
  let prompt = $state('');
  let media = $state('');
  let format = $state('mp4');
  let frames = $state(9);
  let steps = $state(20);
  let size = $state('256x256');
  let busy = $state(false);
  let progress = $state(0);
  let error = $state('');
  let answer = $state('');
  let output = $state('');
  let controller: AbortController | undefined;
  let job = '';
  async function api(path: string, init: RequestInit = {}) {
    const response = await fetch(`${app.inferenceUrl}${path}`, { ...init, headers: { 'Content-Type': 'application/json', ...app.inferenceHeaders() } });
    if (!response.ok) throw new Error((await response.json()).error?.message ?? response.statusText);
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
  onDestroy(() => { cancel(); if (output) URL.revokeObjectURL(output); });
  async function run() {
    const c = new AbortController(); controller = c;
    busy = true; error = ''; answer = ''; progress = 0;
    try {
      if (understanding) {
        const r = await api('/chat/completions', { method: 'POST', signal: c.signal, body: JSON.stringify(videoQuestion(model, prompt, media)) });
        answer = (await r.json()).choices[0].message.content;
      } else {
        const r = await api('/videos', { method: 'POST', body: JSON.stringify({ model, prompt, frames, steps, size, output_format: format, ...(media ? { input_reference: media } : {}) }) });
        const id = (await r.json()).id as string; job = id;
        if (c.signal.aborted) { cancel(); return; }
        for (;;) {
          c.signal.throwIfAborted();
          const r = await api(`/videos/${id}`, { signal: c.signal });
          const status = await r.json(); progress = status.progress;
          if (status.status === 'failed') throw new Error(status.error?.message);
          if (status.status === 'completed') break;
          await new Promise(resolve => setTimeout(resolve, 1000));
        }
        const blob = await (await api(`/videos/${id}/content`, { signal: c.signal })).blob();
        if (output) URL.revokeObjectURL(output);
        output = URL.createObjectURL(blob);
        await fileToProject(outputName(prompt, format), await blobToBase64(blob), { source: 'manual', model });
        job = '';
      }
    } catch (e) { if (!c.signal.aborted) error = e instanceof Error ? e.message : String(e); }
    finally { busy = false; }
  }
</script>

<div class="space-y-4 p-4">
  <h2 class="text-lg font-semibold">{t(understanding ? 'vid.questions' : 'vid.generate')}</h2>
  <p class="text-sm text-muted-foreground">{t('vid.memory')}</p>
  <Label for="video-prompt">{t(understanding ? 'vid.question' : 'vid.prompt')}</Label>
  <Textarea id="video-prompt" bind:value={prompt} disabled={busy} />
  <Label for="video-media">{t(understanding ? 'vid.input' : 'vid.start_image')}</Label>
  <input id="video-media" type="file" accept={understanding ? 'video/mp4,video/webm' : 'image/png,image/jpeg,image/webp'} onchange={pick} disabled={busy} />
  {#if !understanding}
    <div class="flex flex-wrap gap-3">
      <Label>{t('vid.format')} <select bind:value={format} disabled={busy}><option>mp4</option><option>webm</option></select></Label>
      <Label for="video-frames">{t('vid.frames')}</Label><Input id="video-frames" type="number" min={1} max={129} bind:value={frames} disabled={busy} />
      <Label for="video-steps">{t('vid.steps')}</Label><Input id="video-steps" type="number" min={1} max={500} bind:value={steps} disabled={busy} />
      <Label for="video-size">{t('vid.size')}</Label><Input id="video-size" bind:value={size} disabled={busy} />
    </div>
  {/if}
  <Button onclick={run} disabled={busy || !app.gatewayRunning || !model || !prompt.trim() || ((understanding || task === 'image-to-video') && !media)}>{t('vid.run')}</Button>
  {#if busy}<Button variant="outline" onclick={cancel}>{t('vid.cancel')}</Button><progress max="100" value={progress}></progress>{/if}
  {#if error}<p role="alert" class="text-destructive">{error}</p>{/if}
  {#if answer}<p class="whitespace-pre-wrap">{answer}</p>{/if}
  {#if output}<video controls src={output}><track kind="captions" /></video>{/if}
</div>
