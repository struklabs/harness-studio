<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { localizeError } from '#lib/network.js';
  import { errorCode } from '#lib/errors.js';
  import type { GatewayOverview } from '#lib/types.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { libraryDefinitions } from '#lib/stores/downloads.svelte.js';
  import { filesNeedingDefinition } from '#lib/libraryDefinitions.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import EngineNeeded from '../common/EngineNeeded.svelte';
  import { familyFromError } from '#lib/engines.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { holdReleaseWhile } from '#lib/windowActivity.js';
  import { Compass, FolderOpen, Image as ImageIcon, ImagePlus, LoaderCircle, Maximize2, Paintbrush, Power, Save, Square, Trash2, TriangleAlert, X } from '@lucide/svelte';
  import {
    dataUrl,
    fetchImages,
    ImageOutOfMemory,
    imageModel,
    isImageDefinition,
    isMflux,
    isUpscaleDefinition,
    loraOk,
    loraScaleOk,
    maskMode,
    needsStartImage,
    takesLora,
    takesStrength,
    parseSize,
    pngSize,
    MAX_START_IMAGE_BYTES,
    readStartImage,
    SIZE_PRESETS,
    START_IMAGE_TYPES,
    strengthOk,
    upscaleImage,
    upscaleModel,
  } from './image';
  import type { ImageResult } from './image';
  import { addToGallery, imageSession as form, removeFromGallery, type GalleryItem } from './image.svelte';
  import { fileToProject, outputName } from './projectOutputs';
  import MaskEditor from './MaskEditor.svelte';

  /** Image definitions: the gateway loads the diffusion model on the first request (JIT). */
  let models = $derived(domain.definitions.filter((d) => isImageDefinition(d, library.models)));
  /** Image files of the library that still have no definition after the default ones were made: their companion files are missing. */
  /** Super-resolution definitions (Swin2SR): the Upscale action runs on one of them. */
  let upscalers = $derived(domain.definitions.filter((d) => isUpscaleDefinition(d, library.models)));
  let incomplete = $derived(filesNeedingDefinition(library.models, domain.definitions, ['image']));
  let busy = $state(false);
  let error = $state('');
  let memoryBlocked = $state(false);
  /** Engine family the failed run was missing; shown with its install button. */
  let needFamily = $state<string | null>(null);
  let lastMs = $state<number | null>(null);
  let controller: AbortController | null = null;
  // A picture in the gallery would be lost with the page; a request in flight counts as one.
  $effect(() => holdReleaseWhile('image-gallery', () => busy || form.gallery.length > 0));

  $effect(() => {
    form.model = imageModel(form.model, domain.definitions, library.models);
  });
  $effect(() => {
    form.upscaleModel = upscaleModel(form.upscaleModel, domain.definitions, library.models);
  });
  $effect(() => {
    void domain.refreshDefinitions();
  });

  let sizeError = $derived(form.size.trim() !== '' && !parseSize(form.size));
  let strengthShown = $derived(takesStrength(form.model, domain.definitions, library.models));
  let strengthError = $derived(!!form.init && strengthShown && !strengthOk(form.strength));
  /** InstructPix2Pix, Qwen-Image-Edit...: they edit a start image and draw nothing from text alone. */
  let editsOnly = $derived(needsStartImage(form.model, domain.definitions, library.models));
  /** mflux (FLUX.2 [klein], Z-Image, FLUX.1, Qwen-Image): no negative prompt or CFG; an empty size is the largest that fits. */
  let mflux = $derived(isMflux(form.model, domain.definitions, library.models));
  /** What a mask does here: FLUX.1 Fill and inpainting checkpoints need one, text-to-image diffusers pipelines may take one. */
  let maskKind = $derived(maskMode(form.model, domain.definitions, library.models));
  let inpaints = $derived(maskKind !== 'none');
  let maskRequired = $derived(maskKind === 'required');
  /** A LoRA per request: Python engine diffusers pipelines only. */
  let loraShown = $derived(takesLora(form.model, domain.definitions, library.models));
  let loraError = $derived(loraShown && !loraOk(form.lora));
  let loraScaleError = $derived(loraShown && form.lora.trim() !== '' && !loraScaleOk(form.loraScale));
  let canRun = $derived(
    !busy &&
      app.gatewayRunning &&
      !!form.model &&
      form.prompt.trim() !== '' &&
      !sizeError &&
      !strengthError &&
      !loraError &&
      !loraScaleError &&
      (!editsOnly || !!form.init) &&
      (!maskRequired || !!form.mask),
  );
  let drawingMask = $state(false);
  let upscaling = $state(false);
  let fileInput = $state<HTMLInputElement | null>(null);
  let maskInput = $state<HTMLInputElement | null>(null);

  async function pickLora() {
    try {
      const path = await invoke<string | null>('pick_lora_file', { label: t('img.lora_filter') });
      if (path) form.lora = path;
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  /** Enlarges a picture with the chosen super-resolution model; the result joins the gallery. */
  async function upscale(item: ImageResult, label: string) {
    if (busy || upscaling || !form.upscaleModel || !app.gatewayRunning) return;
    controller?.abort();
    const c = new AbortController();
    controller = c;
    upscaling = true;
    error = '';
    needFamily = null;
    try {
      const out = await upscaleImage(app.inferenceUrl, form.upscaleModel, item, app.inferenceHeaders(), c.signal);
      lastMs = out.ms;
      addToGallery([
        {
          image: out.image,
          prompt: label,
          model: form.upscaleModel,
          size: pngSize(out.image) || (out.width && out.height ? `${out.width}x${out.height}` : t('img.model_size')),
          ms: out.ms,
        },
      ]);
      void fileToProject(outputName(label, 'png', 'image'), out.image.data, { source: 'image', model: form.upscaleModel });
    } catch (e) {
      if (!c.signal.aborted) {
        const msg = e instanceof Error ? e.message : String(e);
        needFamily = familyFromError(msg);
        error = needFamily ? '' : msg;
      }
    } finally {
      if (controller === c) controller = null;
      upscaling = false;
    }
  }

  async function pickMask(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    const image = await readStartImage(file);
    if (image === 'type') toast.error(t('img.init_bad_type'));
    else if (image === 'size') toast.error(t('img.init_too_big', { mb: MAX_START_IMAGE_BYTES / 1024 / 1024 }));
    else {
      form.mask = image;
      drawingMask = false;
    }
  }

  async function pickStartImage(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    const image = await readStartImage(file);
    if (image === 'type') toast.error(t('img.init_bad_type'));
    else if (image === 'size') toast.error(t('img.init_too_big', { mb: MAX_START_IMAGE_BYTES / 1024 / 1024 }));
    else useAsStart(image);
  }

  /** A start image sets the canvas: an older size would silently win over it. */
  function useAsStart(image: ImageResult) {
    form.init = image;
    form.size = '';
    form.mask = null;
    drawingMask = false;
  }

  async function run() {
    if (!canRun) return;
    controller?.abort();
    const c = new AbortController();
    controller = c;
    busy = true;
    error = '';
    memoryBlocked = false;
    needFamily = null;
    try {
      const base = inpaints ? form : { ...form, mask: null };
      const withLora = loraShown ? base : { ...base, lora: '', loraScale: '' };
      const params = mflux ? { ...withLora, negative: '', cfgScale: '' } : withLora;
      const { images, ms } = await fetchImages(app.inferenceUrl, form.model, params, app.inferenceHeaders(), c.signal);
      lastMs = ms;
      addToGallery(
        images.map((image) => ({
          image,
          prompt: form.prompt.trim(),
          model: form.model,
          size: pngSize(image) || form.size.trim() || t('img.model_size'),
          ms,
        })),
      );
      for (const image of images) {
        void fileToProject(outputName(form.prompt, image.format === 'jpeg' ? 'jpg' : image.format || 'png', 'image'), image.data, { source: 'image', model: form.model });
      }
    } catch (e) {
      if (!c.signal.aborted && e instanceof ImageOutOfMemory) {
        error = t('img.err_out_of_memory', { need: e.needMb, available: e.availableMb });
      } else if (!c.signal.aborted) {
        memoryBlocked = errorCode(e)?.startsWith('gateway.gpu_memory_') ?? false;
        const msg = e instanceof Error ? e.message : String(e);
        // The gateway passes the load failure through as raw keys; say what is missing instead.
        needFamily = familyFromError(msg);
        error = needFamily ? '' : msg;
      }
    } finally {
      if (controller === c) {
        controller = null;
        busy = false;
      }
    }
  }

  function stop() {
    // The gateway sees the dropped connection and cancels the sd-server job.
    controller?.abort();
    controller = null;
    busy = false;
    upscaling = false;
  }

  async function save(item: GalleryItem) {
    try {
      const saved = await invoke<string | null>('save_generated_image', {
        data: item.image.data,
        fileName: item.prompt.slice(0, 40).trim().replace(/\s+/g, '-').toLowerCase() || 'image',
      });
      if (saved) toast.success(t('img.saved', { path: saved }));
    } catch (e) {
      toast.error(t('img.save_failed', { error: localizeError(e) }));
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
</script>

<div class="mx-auto flex w-full max-w-[960px] flex-col gap-5 px-4 py-6">
  {#if !app.gatewayRunning}
    <div class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('img.need_gateway')}</span>
      <Button variant="outline" size="sm" class="shrink-0" disabled={enablingGateway} onclick={enableGateway}>
        {#if enablingGateway}<LoaderCircle class="animate-spin" />{:else}<Power />{/if}
        {t('asr.enable_gateway')}
      </Button>
    </div>
  {/if}
  {#if models.length === 0 && !libraryDefinitions.busy}
    <div class="flex items-start gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 flex-1">{incomplete.length > 0 ? t('img.files_incomplete') : t('img.no_model')}</span>
      <Button
        variant="outline"
        size="sm"
        class="shrink-0"
        onclick={() => {
          app.requestedHubSearch = 'sd-turbo';
          app.requestedTab = 'hub';
        }}
      >
        <Compass />{t('img.find_model')}
      </Button>
    </div>
  {/if}

  <section class="flex flex-col gap-3" aria-labelledby="img-input">
    <h3 id="img-input" class="sr-only">{t('img.title')}</h3>
    <div class="flex flex-col gap-1.5">
      <Label for="img-model" class="text-sm font-semibold text-foreground">{t('img.model_label')}</Label>
      <select
        id="img-model"
        bind:value={form.model}
        disabled={busy || models.length === 0}
        class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
      >
        {#each models as d (d.id)}
          <option value={d.alias}>{d.alias}</option>
        {/each}
      </select>
    </div>
    <div class="flex flex-col gap-1.5">
      <Label for="img-prompt" class="text-sm font-semibold text-foreground">{t('img.prompt_label')}</Label>
      <Textarea
        id="img-prompt"
        bind:value={form.prompt}
        rows={3}
        placeholder={t('img.prompt_placeholder')}
        disabled={busy}
        onkeydown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            void run();
          }
        }}
      />
    </div>
    {#if loraShown}
      <div class="flex flex-col gap-1.5">
        <span class="text-sm font-semibold text-foreground">{t('img.lora_label')}</span>
        <div class="flex flex-wrap items-start gap-2">
          <Input
            id="img-lora"
            bind:value={form.lora}
            placeholder={t('img.lora_placeholder')}
            disabled={busy}
            aria-label={t('img.lora_label')}
            aria-invalid={loraError}
            class="min-w-0 flex-1 basis-64"
          />
          <Button variant="outline" size="sm" disabled={busy} onclick={pickLora}><FolderOpen />{t('img.lora_pick')}</Button>
          <Input
            id="img-lora-scale"
            bind:value={form.loraScale}
            inputmode="decimal"
            placeholder="1"
            disabled={busy}
            aria-label={t('img.lora_scale_label')}
            aria-invalid={loraScaleError}
            title={t('img.lora_scale_label')}
            class="w-20"
          />
          {#if form.lora}
            <Button variant="ghost" size="sm" disabled={busy} onclick={() => (form.lora = '')}><X />{t('img.lora_remove')}</Button>
          {/if}
        </div>
        <span class="text-xs {loraError || loraScaleError ? 'text-destructive' : 'text-muted-foreground'}">
          {loraError ? t('img.lora_invalid') : loraScaleError ? t('img.lora_scale_invalid') : t('img.lora_note')}
        </span>
      </div>
    {/if}
    <AdvancedOnly collapsible invalid={sizeError}>
    {#if !mflux}
      <div class="flex flex-col gap-1.5">
        <Label for="img-negative" class="text-sm font-semibold text-foreground">{t('img.negative_label')}</Label>
        <Input id="img-negative" bind:value={form.negative} placeholder={t('img.negative_placeholder')} disabled={busy} />
      </div>
    {/if}
    <div class="grid gap-3 sm:grid-cols-4">
      <Field label={t('img.size_label')} id="img-size" note={sizeError ? t('img.size_invalid') : t('img.size_note')}>
        <Input id="img-size" bind:value={form.size} list="img-sizes" placeholder={form.init ? t('img.init_size') : t(mflux ? 'img.mflux_size' : 'img.model_size')} disabled={busy} aria-invalid={sizeError} />
        <datalist id="img-sizes">
          {#each SIZE_PRESETS as s (s)}<option value={s}></option>{/each}
        </datalist>
      </Field>
      <Field label={t('img.steps_label')} id="img-steps">
        <Input id="img-steps" bind:value={form.steps} inputmode="numeric" placeholder={t('img.model_default')} disabled={busy} />
      </Field>
      <Field label={t('img.seed_label')} id="img-seed" note={t('img.seed_note')}>
        <Input id="img-seed" bind:value={form.seed} inputmode="numeric" placeholder={t('img.seed_random')} disabled={busy} />
      </Field>
      <Field label={t('img.count_label')} id="img-count">
        <select id="img-count" bind:value={form.count} disabled={busy} class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground">
          {#each [1, 2, 3, 4] as n (n)}<option value={n}>{n}</option>{/each}
        </select>
      </Field>
    </div>
    {#if !mflux}
        <div class="grid gap-3 sm:grid-cols-4">
          <Field label={t('img.cfg_label')} id="img-cfg" note={t('img.cfg_note')}>
            <Input id="img-cfg" bind:value={form.cfgScale} inputmode="decimal" placeholder={t('img.model_default')} disabled={busy} />
          </Field>
        </div>
    {/if}
    </AdvancedOnly>
    <div class="flex flex-col gap-1.5">
      <span class="text-sm font-semibold text-foreground">{t('img.init_label')}</span>
      <input bind:this={fileInput} type="file" accept={START_IMAGE_TYPES.join(',')} class="hidden" onchange={pickStartImage} />
      {#if form.init}
        <div class="flex flex-wrap items-end gap-3">
          <img src={dataUrl(form.init)} alt={t('img.init_label')} class="size-24 rounded-md border border-border bg-muted object-contain" />
          {#if strengthShown}
            <Field label={t('img.strength_label')} id="img-strength" note={strengthError ? t('img.strength_invalid') : t('img.strength_note')}>
              <Input id="img-strength" bind:value={form.strength} inputmode="decimal" placeholder="0.75" disabled={busy} aria-invalid={strengthError} class="w-28" />
            </Field>
          {/if}
          <Button variant="ghost" size="sm" disabled={busy} onclick={() => (form.init = null)}><X />{t('img.init_remove')}</Button>
          {#if form.upscaleModel}
            <Button variant="outline" size="sm" disabled={busy || upscaling || !app.gatewayRunning} onclick={() => form.init && upscale(form.init, t('img.upscale_label'))} title={t('img.upscale_hint')}>
              {#if upscaling}<LoaderCircle class="animate-spin" />{:else}<Maximize2 />{/if}{t('img.upscale')}
            </Button>
          {/if}
        </div>
        {#if inpaints}
          <input bind:this={maskInput} type="file" accept={START_IMAGE_TYPES.join(',')} class="hidden" onchange={pickMask} />
          {#if drawingMask}
            <MaskEditor
              image={form.init}
              onapply={(mask) => {
                form.mask = mask;
                drawingMask = false;
              }}
              oncancel={() => (drawingMask = false)}
            />
          {:else}
            <div class="flex flex-wrap items-end gap-3">
              {#if form.mask}
                <img src={dataUrl(form.mask)} alt={t('img.mask_label')} class="size-24 rounded-md border border-border bg-muted object-contain" />
                <Button variant="outline" size="sm" disabled={busy} onclick={() => (drawingMask = true)}><Paintbrush />{t('img.mask_draw')}</Button>
                <Button variant="ghost" size="sm" disabled={busy} onclick={() => (form.mask = null)}><X />{t('img.mask_remove')}</Button>
              {:else}
                <Button variant="outline" size="sm" disabled={busy} onclick={() => (drawingMask = true)}><Paintbrush />{t('img.mask_draw')}</Button>
                <Button variant="outline" size="sm" disabled={busy} onclick={() => maskInput?.click()}><ImagePlus />{t('img.mask_pick')}</Button>
              {/if}
              <span class="text-xs text-muted-foreground">{t(maskRequired ? 'img.mask_note' : 'img.mask_note_optional')}</span>
            </div>
          {/if}
        {/if}
      {:else}
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" disabled={busy} onclick={() => fileInput?.click()}><ImagePlus />{t('img.init_pick')}</Button>
          <span class="text-xs text-muted-foreground">{t(editsOnly ? 'img.init_required' : 'img.init_note')}</span>
        </div>
      {/if}
    </div>
    <div class="flex flex-wrap items-center gap-3">
      {#if busy}
        <Button size="sm" variant="destructive" onclick={stop}>
          <Square />{t('img.stop')}
        </Button>
        <span class="flex items-center gap-2 text-xs text-muted-foreground" role="status">
          <LoaderCircle class="size-3.5 animate-spin" />{t('img.running')}
        </span>
      {:else}
        <Button size="sm" onclick={run} disabled={!canRun}>
          <ImageIcon />{t('img.run')}
        </Button>
        {#if form.prompt.trim() === ''}
          <span class="text-xs text-muted-foreground">{t('img.need_prompt')}</span>
        {/if}
      {/if}
    </div>
  </section>

  {#if needFamily}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      <EngineNeeded family={needFamily} onInstalled={() => (needFamily = null)} />
    </div>
  {:else if error}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      {error}
      {#if memoryBlocked}
        <Button variant="outline" size="sm" class="mt-2 flex" onclick={() => (app.requestedTab = 'servers')}>{t('img.manage_servers')}</Button>
      {/if}
    </div>
  {/if}

  <section class="flex flex-col gap-3" aria-labelledby="img-gallery">
    <div class="flex flex-wrap items-center gap-2">
      <h3 id="img-gallery" class="text-sm font-semibold text-foreground">{t('img.gallery_title')}</h3>
      {#if lastMs !== null}
        <AdvancedOnly><span class="font-num text-xs text-muted-foreground">{t('emb.stat_time', { value: Math.round(lastMs) })}</span></AdvancedOnly>
      {/if}
    </div>
    {#if upscalers.length > 0}
      <div class="flex flex-wrap items-center gap-2">
        <Label for="img-upscaler" class="text-xs text-muted-foreground">{t('img.upscale_model')}</Label>
        <select
          id="img-upscaler"
          bind:value={form.upscaleModel}
          disabled={busy || upscaling}
          class="h-8 rounded-md border border-input bg-background px-2 text-xs text-foreground"
        >
          {#each upscalers as d (d.id)}
            <option value={d.alias}>{d.alias}</option>
          {/each}
        </select>
      </div>
    {:else if form.gallery.length > 0}
      <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <span>{t('img.upscale_none')}</span>
        <Button
          variant="outline"
          size="sm"
          onclick={() => {
            app.requestedHubSearch = 'swin2SR';
            app.requestedTab = 'hub';
          }}
        >
          <Compass />{t('img.find_model')}
        </Button>
      </div>
    {/if}
    {#if form.gallery.length === 0}
      <p class="text-sm text-muted-foreground">{t('img.gallery_empty')}</p>
    {:else}
      <ul class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {#each form.gallery as item (item.id)}
          <li class="flex flex-col overflow-hidden rounded-lg border border-border bg-muted/20">
            <img src={dataUrl(item.image)} alt={item.prompt} class="aspect-square w-full bg-muted object-contain" />
            <div class="flex flex-col gap-1.5 p-2.5">
              <p class="line-clamp-2 text-xs text-foreground [overflow-wrap:anywhere]" title={item.prompt}>{item.prompt}</p>
              <p class="font-num text-xs text-muted-foreground">{item.model} · {item.size}</p>
              <div class="flex items-center gap-1.5">
                <Button variant="outline" size="sm" onclick={() => save(item)}><Save />{t('img.save')}</Button>
                <Button variant="ghost" size="sm" disabled={busy} onclick={() => useAsStart(item.image)} title={t('img.use_as_init_hint')}>
                  <ImagePlus />{t('img.use_as_init')}
                </Button>
                {#if form.upscaleModel}
                  <Button variant="ghost" size="sm" disabled={busy || upscaling || !app.gatewayRunning} onclick={() => upscale(item.image, item.prompt)} title={t('img.upscale_hint')}>
                    {#if upscaling}<LoaderCircle class="animate-spin" />{:else}<Maximize2 />{/if}{t('img.upscale')}
                  </Button>
                {/if}
                <Button variant="ghost" size="icon-sm" onclick={() => removeFromGallery(item.id)} aria-label={t('img.remove')} title={t('img.remove')}>
                  <Trash2 />
                </Button>
              </div>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>
