<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { localizeError } from '#lib/network.js';
  import type { GatewayOverview } from '#lib/types.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { cn } from '#lib/utils.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { FileAudio, ImageUp, LoaderCircle, Play, Power, TriangleAlert, Workflow } from '@lucide/svelte';
  import { encodeWav, resample, WAV_RATE } from './wav';
  import { buildAudioRequest, buildImageRequest, buildRequest, highlight, isAudioTask, isFormTask, isImageTask, parseResponse, runPipeline, type PipelineForm, type PipelineResult } from './pipeline';

  /** Largest image read into the request (the gateway takes 47 MiB; a photo is far smaller). */
  const MAX_IMAGE_BYTES = 20 * 1024 * 1024;
  /** Longest clip sent (the sidecar's limit); a 16 kHz 16-bit WAV of it is ~19 MB. */
  const MAX_AUDIO_SECONDS = 600;
  /** Mask overlay colours, one per label in order. */
  const MASK_COLORS = ['#ef4444', '#3b82f6', '#22c55e', '#eab308', '#a855f7', '#06b6d4', '#f97316', '#ec4899'];

  interface Props {
    /** Definition alias sent to `/v1/hf/{model}`. */
    model: string;
    /** Hugging Face pipeline task of the model's file. */
    task: string | undefined;
  }

  let { model, task }: Props = $props();

  let form = $state<PipelineForm>({ text: '', labels: t('pl.sample_labels'), multiLabel: false, question: t('pl.sample_question'), context: t('pl.sample_context') });
  let raw = $state('{\n  "inputs": ""\n}');
  let busy = $state(false);
  let error = $state('');
  let result = $state<{ res: PipelineResult; ms: number; text: string; image: string } | null>(null);
  /** The image of an image task, as a data URL. */
  let image = $state('');
  let imageName = $state('');
  /** Natural size of the result image (detection boxes are in its pixels). */
  let natural = $state({ w: 0, h: 0 });
  /** Masks shown over the image, by label. */
  let shownMasks = $state<Record<string, boolean>>({});

  function pickImage(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    if (file.size > MAX_IMAGE_BYTES) {
      error = t('pl.image_too_big', { mb: Math.round(MAX_IMAGE_BYTES / 1048576) });
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      image = typeof reader.result === 'string' ? reader.result : '';
      imageName = file.name;
      error = '';
    };
    reader.onerror = () => (error = t('pl.image_unreadable'));
    reader.readAsDataURL(file);
  }

  /** The clip of an audio task, as a 16 kHz mono WAV data URL. */
  let audio = $state('');
  let audioName = $state('');
  let decoding = $state(false);

  /** Any format the webview decodes (WAV, MP3, FLAC, OGG...) becomes a 16 kHz mono WAV. */
  async function pickAudio(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    decoding = true;
    error = '';
    const ctx = new AudioContext();
    try {
      const decoded = await ctx.decodeAudioData(await file.arrayBuffer());
      if (decoded.duration > MAX_AUDIO_SECONDS) {
        error = t('pl.audio_too_long', { min: MAX_AUDIO_SECONDS / 60 });
        return;
      }
      const mono = new Float32Array(decoded.length);
      for (let c = 0; c < decoded.numberOfChannels; c++) {
        const data = decoded.getChannelData(c);
        for (let i = 0; i < data.length; i++) mono[i] += data[i] / decoded.numberOfChannels;
      }
      const wav = encodeWav(resample(mono, decoded.sampleRate, WAV_RATE), WAV_RATE);
      audio = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '');
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(new Blob([wav], { type: 'audio/wav' }));
      });
      audioName = file.name;
    } catch {
      error = t('pl.audio_unreadable');
    } finally {
      void ctx.close();
      decoding = false;
    }
  }
  let controller: AbortController | null = null;

  // A sample that fits the task whenever the task changes (and the text is still a sample or empty).
  let lastTask: string | undefined;
  $effect(() => {
    const next = task;
    if (next === lastTask) return;
    lastTask = next;
    result = null;
    error = '';
    form.labels = next === 'zero-shot-image-classification' || next === 'zero-shot-object-detection' ? t('pl.sample_image_labels') : t('pl.sample_labels');
    form.text =
      next === 'zero-shot-classification' ? t('pl.sample_zero_shot')
      : next === 'token-classification' ? t('pl.sample_ner')
      : next === 'fill-mask' ? t('pl.sample_mask')
      : next === 'summarization' ? t('pl.sample_summary')
      : next === 'translation' ? t('pl.sample_translation')
      : next === 'text2text-generation' ? t('pl.sample_t2t')
      : t('pl.sample_text');
  });

  let body = $derived.by<Record<string, unknown> | null>(() => {
    if (task && isImageTask(task)) return buildImageRequest(task, image, form.labels);
    if (isAudioTask(task)) return buildAudioRequest(audio);
    if (isFormTask(task)) return buildRequest(task, form);
    try {
      const v = JSON.parse(raw) as unknown;
      return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
    } catch {
      return null;
    }
  });
  let canRun = $derived(!busy && !decoding && app.gatewayRunning && !!model && body !== null);

  async function run() {
    if (!canRun || !body) return;
    controller?.abort();
    const c = new AbortController();
    controller = c;
    busy = true;
    error = '';
    const text = form.text;
    const shot = image;
    try {
      const { json, ms } = await runPipeline(app.inferenceUrl, model, body, app.inferenceHeaders(), c.signal);
      const res = parseResponse(task, json);
      // Every label gets a value: binding a checkbox to `undefined` throws in Svelte 5.
      shownMasks = res.kind === 'masks' ? Object.fromEntries(res.items.map((m, i) => [m.label, i < 3])) : {};
      result = { res, ms, text, image: shot };
    } catch (e) {
      if (!c.signal.aborted) {
        error = e instanceof Error ? e.message : String(e);
        result = null;
      }
    } finally {
      if (controller === c) {
        controller = null;
        busy = false;
      }
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void run();
    }
  }

  let enablingGateway = $state(false);
  /** Same call as the Settings > Gateway switch: the stored settings with `enabled` on. */
  async function enableGateway() {
    enablingGateway = true;
    try {
      const current = app.gateway ?? (await invoke<GatewayOverview>('get_gateway_status'));
      const next = await invoke<GatewayOverview>('set_gateway_settings', {
        settings: { ...current.settings, enabled: true },
      });
      app.setGateway(next);
      toast.success(t('asr.gateway_on'));
    } catch (e) {
      toast.error(t('asr.gateway_on_failed', { error: localizeError(e) }));
    } finally {
      enablingGateway = false;
    }
  }

  const pct = (score: number) => `${Math.max(2, Math.min(100, score * 100)).toFixed(1)}%`;
</script>

<div class="mx-auto flex w-full max-w-[860px] flex-col gap-5 px-4 py-6">
  {#if !app.gatewayRunning}
    <div class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('pl.need_gateway')}</span>
      <Button variant="outline" size="sm" class="shrink-0" disabled={enablingGateway} onclick={enableGateway}>
        {#if enablingGateway}<LoaderCircle class="animate-spin" />{:else}<Power />{/if}
        {t('asr.enable_gateway')}
      </Button>
    </div>
  {/if}
  {#if !model}
    <div class="flex items-start gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('pl.no_model')}</span>
    </div>
  {/if}

  <section class="flex flex-col gap-3" aria-labelledby="pl-input">
    <div class="flex flex-wrap items-center gap-2">
      <h3 id="pl-input" class="text-sm font-semibold text-foreground">{t('pl.task')}</h3>
      <Badge variant="outline" class="font-mono">{task ?? t('pl.task_unknown')}</Badge>
    </div>

    {#if task && isImageTask(task)}
      <div class="flex flex-wrap items-center gap-3">
        <Button variant="outline" size="sm" onclick={() => document.getElementById('pl-image')?.click()} disabled={busy}>
          <ImageUp />
          {t('pl.image_pick')}
        </Button>
        <input id="pl-image" type="file" accept="image/png,image/jpeg,image/webp,image/bmp,image/gif" class="sr-only" onchange={pickImage} />
        <span class="min-w-0 truncate text-xs text-muted-foreground">{imageName || t('pl.image_none')}</span>
      </div>
      {#if image}
        <img src={image} alt={imageName} class="max-h-48 w-fit rounded-md border border-border object-contain" />
      {/if}
      {#if task === 'zero-shot-image-classification' || task === 'zero-shot-object-detection'}
        <Field label={t('pl.labels_label')} id="pl-image-labels" hint={t('pl.image_labels_hint')}>
          <Input id="pl-image-labels" bind:value={form.labels} disabled={busy} onkeydown={(e) => e.key === 'Enter' && run()} />
        </Field>
      {/if}
    {:else if isAudioTask(task)}
      <div class="flex flex-wrap items-center gap-3">
        <Button variant="outline" size="sm" onclick={() => document.getElementById('pl-audio')?.click()} disabled={busy || decoding}>
          {#if decoding}<LoaderCircle class="animate-spin" />{:else}<FileAudio />{/if}
          {t('pl.audio_pick')}
        </Button>
        <input id="pl-audio" type="file" accept="audio/*,.wav,.mp3,.flac,.ogg,.m4a" class="sr-only" onchange={pickAudio} />
        <span class="min-w-0 truncate text-xs text-muted-foreground">{audioName || t('pl.audio_none')}</span>
      </div>
      {#if audio}
        <audio src={audio} controls class="w-full max-w-md"></audio>
      {/if}
    {:else if task === 'question-answering'}
      <div class="flex flex-col gap-1.5">
        <Label for="pl-question" class="text-sm font-medium text-foreground">{t('pl.question_label')}</Label>
        <Input id="pl-question" bind:value={form.question} disabled={busy} onkeydown={(e) => e.key === 'Enter' && run()} />
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="pl-context" class="text-sm font-medium text-foreground">{t('pl.context_label')}</Label>
        <Textarea id="pl-context" rows={5} bind:value={form.context} disabled={busy} onkeydown={onKey} />
      </div>
    {:else if isFormTask(task)}
      <Field label={t('pl.text_label')} id="pl-text" hint={task === 'fill-mask' ? t('pl.mask_hint') : undefined}>
        <Textarea id="pl-text" rows={4} bind:value={form.text} placeholder={t('pl.text_placeholder')} disabled={busy} onkeydown={onKey} />
      </Field>
      {#if task === 'zero-shot-classification'}
        <Field label={t('pl.labels_label')} id="pl-labels" hint={t('pl.labels_hint')}>
          <Input id="pl-labels" bind:value={form.labels} disabled={busy} />
        </Field>
        <div class="flex items-center gap-2">
          <Checkbox id="pl-multi" bind:checked={form.multiLabel} disabled={busy} />
          <Label for="pl-multi" class="text-sm font-normal">{t('pl.multi_label')}</Label>
        </div>
      {/if}
    {:else}
      <Field label={t('pl.raw_label')} id="pl-raw">
        <Textarea id="pl-raw" rows={8} class="font-mono text-xs" bind:value={raw} disabled={busy} onkeydown={onKey} />
      </Field>
      {#if body === null && raw.trim()}
        <span class="text-xs text-destructive">{t('pl.raw_invalid')}</span>
      {/if}
    {/if}

    <div>
      <Button size="sm" onclick={run} disabled={!canRun}>
        {#if busy}<LoaderCircle class="animate-spin" />{:else}<Play />{/if}
        {busy ? t('pl.running') : t('pl.run')}
      </Button>
    </div>
  </section>

  {#if error}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      {error}
    </div>
  {/if}

  {#if result}
    {@const res = result.res}
    <section class="flex flex-col gap-3" aria-labelledby="pl-result">
      <div class="flex flex-wrap items-center gap-2">
        <h3 id="pl-result" class="flex items-center gap-1 text-sm font-semibold text-foreground"><Workflow class="size-4" aria-hidden="true" />{t('pl.result')}</h3>
        <Badge variant="outline" class="font-num">{t('pl.took', { ms: Math.round(result.ms) })}</Badge>
      </div>

      {#if res.kind === 'labels'}
        <ul class="flex flex-col gap-2">
          {#each res.items as item (item.label)}
            <li class="flex flex-col gap-1">
              <div class="flex items-center justify-between gap-3 text-xs">
                <span class="min-w-0 truncate text-foreground">{item.label}</span>
                <span class="font-num text-muted-foreground">{item.score.toFixed(3)}</span>
              </div>
              <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted" aria-hidden="true">
                <div class="h-full rounded-full bg-primary" style:width={pct(item.score)}></div>
              </div>
            </li>
          {/each}
        </ul>
      {:else if res.kind === 'entities'}
        {#if res.items.length === 0}
          <p class="text-xs text-muted-foreground">{t('pl.no_entities')}</p>
        {:else}
          <p class="rounded-md border border-border px-3 py-2 text-sm leading-7 text-foreground [overflow-wrap:anywhere]">
            {#each highlight(result.text.trim(), res.items) as piece, i (i)}
              {#if piece.entity}
                <mark class="rounded bg-primary/15 px-1 text-foreground">{piece.text}<span class="ml-1 font-mono text-[10px] font-semibold text-primary">{piece.entity}</span></mark>
              {:else}{piece.text}{/if}
            {/each}
          </p>
          <table class="w-full border-collapse overflow-hidden rounded-lg border border-border text-xs">
            <tbody>
              {#each res.items as e, i (i)}
                <tr class="border-t border-border first:border-t-0">
                  <td class="w-24 px-3 py-1.5 font-mono font-semibold text-foreground">{e.entity}</td>
                  <td class="px-3 py-1.5 text-foreground [overflow-wrap:anywhere]">{e.word}</td>
                  <td class="w-20 px-3 py-1.5 text-right font-num text-muted-foreground">{e.score.toFixed(3)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {:else if res.kind === 'fills'}
        <ul class="flex flex-col gap-2">
          {#each res.items as f, i (i)}
            <li class="flex flex-col gap-1 rounded-md border border-border px-3 py-2">
              <div class="flex items-center justify-between gap-3 text-xs">
                <span class="font-mono font-semibold text-foreground">{f.token}</span>
                <span class="font-num text-muted-foreground">{f.score.toFixed(3)}</span>
              </div>
              <span class="text-xs text-muted-foreground [overflow-wrap:anywhere]">{f.sequence}</span>
            </li>
          {/each}
        </ul>
      {:else if res.kind === 'answers'}
        <ul class="flex flex-col gap-2">
          {#each res.items as a, i (i)}
            <li class="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2 text-sm">
              <span class="min-w-0 text-foreground [overflow-wrap:anywhere]"><span class="mr-2 text-xs text-muted-foreground">{t('pl.answer')}</span>{a.answer}</span>
              <span class="shrink-0 font-num text-xs text-muted-foreground">{t('pl.score')} {a.score.toFixed(3)}</span>
            </li>
          {/each}
        </ul>
      {:else if res.kind === 'detections'}
        <div class="relative w-fit max-w-full">
          <img src={result.image} alt={imageName} class="block max-h-[480px] max-w-full rounded-md" onload={(e) => { const img = e.currentTarget as HTMLImageElement; natural = { w: img.naturalWidth, h: img.naturalHeight }; }} />
          {#if natural.w > 0}
            {#each res.items as d, i (i)}
              <div
                class="pointer-events-none absolute rounded-sm border-2"
                style:border-color={MASK_COLORS[i % MASK_COLORS.length]}
                style:left={`${(d.box.xmin / natural.w) * 100}%`}
                style:top={`${(d.box.ymin / natural.h) * 100}%`}
                style:width={`${((d.box.xmax - d.box.xmin) / natural.w) * 100}%`}
                style:height={`${((d.box.ymax - d.box.ymin) / natural.h) * 100}%`}
              >
                <!-- A box at the top edge keeps its label inside, over the image. -->
                <span class={cn('absolute left-0 rounded-sm', d.box.ymin < natural.h * 0.05 ? 'top-0' : '-top-5', 'px-1 text-[10px] font-semibold text-white')} style:background-color={MASK_COLORS[i % MASK_COLORS.length]}>{d.label} {d.score.toFixed(2)}</span>
              </div>
            {/each}
          {/if}
        </div>
        {#if res.items.length === 0}<p class="text-xs text-muted-foreground">{t('pl.no_objects')}</p>{/if}
      {:else if res.kind === 'masks'}
        <div class="relative w-fit max-w-full">
          <img src={result.image} alt={imageName} class="block max-h-[480px] max-w-full rounded-md" />
          {#each res.items as m, i (m.label)}
            {#if shownMasks[m.label]}
              <div
                class="pointer-events-none absolute inset-0 rounded-md opacity-50"
                style:background-color={MASK_COLORS[i % MASK_COLORS.length]}
                style:mask-image={`url(${m.mask})`}
                style:mask-size="100% 100%"
                style:mask-mode="luminance"
              ></div>
            {/if}
          {/each}
        </div>
        <div class="flex flex-wrap gap-2">
          {#each res.items as m, i (m.label)}
            <label class="flex items-center gap-1.5 rounded-md border border-border px-2 py-1 text-xs">
              <Checkbox bind:checked={shownMasks[m.label]} />
              <span class="size-2.5 rounded-sm" style:background-color={MASK_COLORS[i % MASK_COLORS.length]}></span>
              {m.label}
            </label>
          {/each}
        </div>
      {:else if res.kind === 'features'}
        <p class="text-xs text-muted-foreground">{t('pl.features_shape', { shape: res.value.shape.join(' × ') })}</p>
        <pre class="overflow-auto rounded-md border border-border bg-muted/30 p-3 font-mono text-xs text-foreground">[{res.value.preview.map((v) => v.toFixed(4)).join(', ')}, …]</pre>
      {:else if res.kind === 'text'}
        <div class={cn('grid gap-3', result.image && 'sm:grid-cols-[minmax(0,12rem)_1fr] sm:items-start')}>
          {#if result.image}<img src={result.image} alt={imageName} class="w-full rounded-md border border-border" />{/if}
          <p class="whitespace-pre-wrap rounded-md border border-border px-3 py-2 text-sm text-foreground [overflow-wrap:anywhere]">{res.value}</p>
        </div>
      {:else if res.kind === 'depth'}
        <div class="grid gap-3 sm:grid-cols-2">
          <img src={result.image} alt={imageName} class="w-full rounded-md border border-border" />
          <img src={res.value.image} alt={t('pl.depth_alt')} class="w-full rounded-md border border-border" />
        </div>
        <p class="text-xs text-muted-foreground">{t('pl.depth_hint')}</p>
      {:else}
        <pre class="max-h-96 overflow-auto rounded-md border border-border bg-muted/30 p-3 font-mono text-xs text-foreground">{JSON.stringify(res.value, null, 2)}</pre>
      {/if}
    </section>
  {/if}
</div>
