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
  import { Compass, Image as ImageIcon, ImagePlus, LoaderCircle, Power, Save, Square, Trash2, TriangleAlert, X } from '@lucide/svelte';
  import {
    dataUrl,
    fetchImages,
    ImageOutOfMemory,
    imageModel,
    isImageDefinition,
    needsStartImage,
    takesStrength,
    parseSize,
    pngSize,
    MAX_START_IMAGE_BYTES,
    readStartImage,
    SIZE_PRESETS,
    START_IMAGE_TYPES,
    strengthOk,
  } from './image';
  import type { ImageResult } from './image';
  import { addToGallery, imageSession as form, removeFromGallery, type GalleryItem } from './image.svelte';

  /** Image definitions: the gateway loads the diffusion model on the first request (JIT). */
  let models = $derived(domain.definitions.filter((d) => isImageDefinition(d, library.models)));
  /** Image files of the library that still have no definition after the default ones were made: their companion files are missing. */
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
    void domain.refreshDefinitions();
  });

  let sizeError = $derived(form.size.trim() !== '' && !parseSize(form.size));
  let strengthShown = $derived(takesStrength(form.model, domain.definitions, library.models));
  let strengthError = $derived(!!form.init && strengthShown && !strengthOk(form.strength));
  /** InstructPix2Pix, Qwen-Image-Edit...: they edit a start image and draw nothing from text alone. */
  let editsOnly = $derived(needsStartImage(form.model, domain.definitions, library.models));
  let canRun = $derived(
    !busy && app.gatewayRunning && !!form.model && form.prompt.trim() !== '' && !sizeError && !strengthError && (!editsOnly || !!form.init),
  );
  let fileInput = $state<HTMLInputElement | null>(null);

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
      const { images, ms } = await fetchImages(app.inferenceUrl, form.model, form, app.inferenceHeaders(), c.signal);
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
    <div class="flex flex-col gap-1.5">
      <Label for="img-negative" class="text-sm font-semibold text-foreground">{t('img.negative_label')}</Label>
      <Input id="img-negative" bind:value={form.negative} placeholder={t('img.negative_placeholder')} disabled={busy} />
    </div>
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
        </div>
      {:else}
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" disabled={busy} onclick={() => fileInput?.click()}><ImagePlus />{t('img.init_pick')}</Button>
          <span class="text-xs text-muted-foreground">{t(editsOnly ? 'img.init_required' : 'img.init_note')}</span>
        </div>
      {/if}
    </div>
    <div class="grid gap-3 sm:grid-cols-4">
      <Field label={t('img.size_label')} id="img-size" note={sizeError ? t('img.size_invalid') : t('img.size_note')}>
        <Input id="img-size" bind:value={form.size} list="img-sizes" placeholder={form.init ? t('img.init_size') : t('img.model_size')} disabled={busy} aria-invalid={sizeError} />
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
    <AdvancedOnly>
      <div class="grid gap-3 sm:grid-cols-4">
        <Field label={t('img.cfg_label')} id="img-cfg" note={t('img.cfg_note')}>
          <Input id="img-cfg" bind:value={form.cfgScale} inputmode="decimal" placeholder={t('img.model_default')} disabled={busy} />
        </Field>
      </div>
    </AdvancedOnly>
    <div class="flex items-center gap-3">
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
