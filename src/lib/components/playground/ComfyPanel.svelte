<script lang="ts">
  // Image tab, ComfyUI source: runs text-to-image workflows on the user's own ComfyUI. The
  // prompt and the size/steps/seed/CFG fields are the same ones the built-in engine uses.
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { toast } from 'svelte-sonner';
  import Field from '../dashboard/parts/Field.svelte';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { errorCode } from '#lib/errors.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { holdReleaseWhile } from '#lib/windowActivity.js';
  import { CircleCheck, ExternalLink, FolderOpen, Image as ImageIcon, LoaderCircle, Radar, RefreshCw, Square, Trash2 } from '@lucide/svelte';
  import { dataUrl, SIZE_PRESETS } from './image';
  import { addComfyResults, imageSession as form } from './image.svelte';
  import {
    COMFYUI_DEFAULT_URL,
    comfyApi,
    groupTemplates,
    paramsFromForm,
    parseComfySize,
    parseSourceKey,
    progressPercent,
    type ComfyCatalog,
    type ComfyStatus,
    type RunEvent,
  } from './comfyui';

  let url = $state('');
  let status = $state<ComfyStatus | null>(null);
  let detecting = $state(false);
  let connectError = $state('');
  let catalog = $state<ComfyCatalog | null>(null);
  let loadingCatalog = $state(false);
  let busy = $state(false);
  let error = $state('');
  let progress = $state<RunEvent | null>(null);
  /** Prompt id of the run in flight (known once ComfyUI queued it): what Cancel removes. */
  let promptId = $state<string | null>(null);
  let cancelling = $state(false);
  $effect(() => holdReleaseWhile('comfyui-run', () => busy));

  let source = $derived(parseSourceKey(form.comfyWorkflow));
  let percent = $derived(progressPercent(progress));
  let sizeError = $derived(form.size.trim() !== '' && !parseComfySize(form.size));
  let canRun = $derived(!busy && !!status && form.prompt.trim() !== '' && !sizeError);
  let deviceText = $derived(status?.devices.map((d) => d.kind || d.name).filter(Boolean).join(', ') ?? '');

  async function detect(quiet = false) {
    detecting = true;
    connectError = '';
    try {
      const found = await comfyApi.status(url.trim() || null);
      // Saved once it answers: the run and the catalog use the saved address.
      await comfyApi.save(found.url);
      url = found.url;
      status = found;
      await loadCatalog();
    } catch (e) {
      status = null;
      catalog = null;
      if (!quiet) connectError = localizeError(e);
    } finally {
      detecting = false;
    }
  }

  async function loadCatalog() {
    loadingCatalog = true;
    try {
      catalog = await comfyApi.catalog();
      const known =
        source.kind === 'builtin' ||
        (source.kind === 'saved' && catalog.workflows.some((w) => w.path === (source as { path: string }).path)) ||
        (source.kind === 'template' && catalog.templates.some((tp) => source.kind === 'template' && tp.source === source.source && tp.name === source.name));
      if (!known) form.comfyWorkflow = 'builtin';
      if (form.comfyCheckpoint && !catalog.checkpoints.includes(form.comfyCheckpoint)) form.comfyCheckpoint = '';
    } catch (e) {
      connectError = localizeError(e);
    } finally {
      loadingCatalog = false;
    }
  }

  async function run() {
    if (!canRun) return;
    busy = true;
    error = '';
    progress = null;
    promptId = null;
    cancelling = false;
    let off: UnlistenFn | undefined;
    try {
      off = await listen<RunEvent>('comfyui-progress', (e) => {
        progress = e.payload;
        promptId = e.payload.prompt_id;
      });
      const checkpoint = form.comfyCheckpoint || (source.kind === 'builtin' ? catalog?.checkpoints[0] ?? null : null);
      const result = await comfyApi.run(source, paramsFromForm(form, checkpoint));
      const workflow = workflowLabel();
      addComfyResults(
        result.images.map((img) => ({
          path: img.path,
          image: { data: img.data, format: img.format },
          prompt: form.prompt.trim(),
          workflow,
          seed: result.seed,
        })),
      );
      toast.success(t('comfy.saved', { count: result.images.length }));
    } catch (e) {
      if (errorCode(e) === 'comfyui.cancelled') toast.info(t('comfy.cancelled'));
      else error = localizeError(e);
    } finally {
      off?.();
      busy = false;
      progress = null;
      promptId = null;
      cancelling = false;
    }
  }

  async function cancel() {
    if (!promptId) return;
    cancelling = true;
    try {
      await comfyApi.cancel(promptId);
    } catch (e) {
      cancelling = false;
      toast.error(localizeError(e));
    }
  }

  function workflowLabel(): string {
    if (source.kind === 'saved') return source.path;
    if (source.kind === 'template') {
      const s = source;
      return catalog?.templates.find((tp) => tp.source === s.source && tp.name === s.name)?.title ?? s.name;
    }
    return t('comfy.builtin');
  }

  async function open(path: string, reveal: boolean) {
    try {
      await (reveal ? comfyApi.reveal(path) : comfyApi.open(path));
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  function stageText(e: RunEvent | null): string {
    if (cancelling) return t('comfy.cancelling');
    if (!e) return t('comfy.sending');
    if (e.stage === 'queued') return t('comfy.queued');
    if (e.stage === 'progress' && e.max > 0) return t('comfy.step', { value: e.value, max: e.max });
    return t('comfy.running');
  }

  onMount(() => {
    void (async () => {
      try {
        url = (await comfyApi.settings()).url;
      } catch {
        /* Not saved yet. */
      }
      if (url) await detect(true);
    })();
  });
</script>

<div class="mx-auto flex w-full max-w-[960px] flex-col gap-5 px-4 py-6">
  <section class="flex flex-col gap-3 rounded-lg border border-border p-3" aria-labelledby="comfy-connection">
    <div class="flex flex-wrap items-center gap-2">
      <h3 id="comfy-connection" class="text-sm font-semibold text-foreground">{t('comfy.connection_title')}</h3>
      {#if status}
        <span class="flex items-center gap-1 text-xs text-success" role="status">
          <CircleCheck class="size-3.5" aria-hidden="true" />
          {t('comfy.detected', { version: status.version ?? '?' })}{deviceText ? ` · ${deviceText}` : ''}
        </span>
      {/if}
    </div>
    <p class="text-xs text-muted-foreground">{t('comfy.connection_desc')}</p>
    <div class="flex flex-wrap items-end gap-2">
      <div class="flex min-w-60 flex-1 flex-col gap-1.5">
        <Label for="comfy-url" class="text-xs">{t('comfy.url')}</Label>
        <Input
          id="comfy-url"
          bind:value={url}
          placeholder={COMFYUI_DEFAULT_URL}
          disabled={busy || detecting}
          onkeydown={(e) => {
            if (e.key === 'Enter') void detect();
          }}
        />
      </div>
      <Button variant="outline" size="sm" disabled={busy || detecting} onclick={() => detect()}>
        {#if detecting}<LoaderCircle class="animate-spin" />{:else}<Radar />{/if}
        {t('comfy.detect')}
      </Button>
    </div>
    {#if connectError}
      <p class="text-xs text-destructive [overflow-wrap:anywhere]" role="alert">{connectError}</p>
    {/if}
  </section>

  <section class="flex flex-col gap-3" aria-labelledby="comfy-input">
    <h3 id="comfy-input" class="sr-only">{t('comfy.title')}</h3>
    <div class="grid gap-3 sm:grid-cols-2">
      <div class="flex flex-col gap-1.5">
        <div class="flex items-center gap-1">
          <Label for="comfy-workflow" class="text-sm font-semibold text-foreground">{t('comfy.workflow')}</Label>
          <Button
            variant="ghost"
            size="icon-sm"
            class="ml-auto"
            disabled={!status || loadingCatalog || busy}
            onclick={loadCatalog}
            aria-label={t('comfy.refresh')}
            title={t('comfy.refresh')}
          >
            <RefreshCw class={loadingCatalog ? 'animate-spin' : ''} />
          </Button>
        </div>
        <select
          id="comfy-workflow"
          bind:value={form.comfyWorkflow}
          disabled={busy || !catalog}
          class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
        >
          <option value="builtin">{t('comfy.builtin')}</option>
          {#if catalog?.workflows.length}
            <optgroup label={t('comfy.saved_workflows')}>
              {#each catalog.workflows as w (w.path)}
                <option value={`saved:${w.path}`}>{w.path.replace(/\.json$/, '')}</option>
              {/each}
            </optgroup>
          {/if}
          {#each groupTemplates(catalog?.templates ?? []) as [group, items] (group)}
            <optgroup label={t('comfy.templates', { group })}>
              {#each items as tp (`${tp.source}/${tp.name}`)}
                <option value={`template:${tp.source}/${tp.name}`} title={tp.description}>{tp.title}</option>
              {/each}
            </optgroup>
          {/each}
        </select>
        <span class="text-xs text-muted-foreground">{t(source.kind === 'builtin' ? 'comfy.builtin_hint' : 'comfy.workflow_hint')}</span>
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="comfy-checkpoint" class="text-sm font-semibold text-foreground">{t('comfy.checkpoint')}</Label>
        <select
          id="comfy-checkpoint"
          bind:value={form.comfyCheckpoint}
          disabled={busy || !catalog || catalog.checkpoints.length === 0}
          class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
        >
          <option value="">{t(source.kind === 'builtin' ? 'comfy.checkpoint_first' : 'comfy.checkpoint_workflow')}</option>
          {#each catalog?.checkpoints ?? [] as c (c)}
            <option value={c}>{c}</option>
          {/each}
        </select>
        {#if catalog && catalog.checkpoints.length === 0}
          <span class="text-xs text-muted-foreground">{t('comfy.no_checkpoints')}</span>
        {/if}
      </div>
    </div>
    <div class="flex flex-col gap-1.5">
      <Label for="comfy-prompt" class="text-sm font-semibold text-foreground">{t('img.prompt_label')}</Label>
      <Textarea
        id="comfy-prompt"
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
    <AdvancedOnly collapsible invalid={sizeError}>
      <div class="flex flex-col gap-1.5">
        <Label for="comfy-negative" class="text-sm font-semibold text-foreground">{t('img.negative_label')}</Label>
        <Input id="comfy-negative" bind:value={form.negative} placeholder={t('comfy.negative_placeholder')} disabled={busy} />
      </div>
      <div class="grid gap-3 sm:grid-cols-4">
        <Field label={t('img.size_label')} id="comfy-size" note={sizeError ? t('comfy.size_invalid') : t('comfy.workflow_default')}>
          <Input id="comfy-size" bind:value={form.size} list="comfy-sizes" placeholder={t('comfy.workflow_value')} disabled={busy} aria-invalid={sizeError} />
          <datalist id="comfy-sizes">
            {#each SIZE_PRESETS as s (s)}<option value={s}></option>{/each}
          </datalist>
        </Field>
        <Field label={t('img.steps_label')} id="comfy-steps">
          <Input id="comfy-steps" bind:value={form.steps} inputmode="numeric" placeholder={t('comfy.workflow_value')} disabled={busy} />
        </Field>
        <Field label={t('img.seed_label')} id="comfy-seed" note={t('img.seed_note')}>
          <Input id="comfy-seed" bind:value={form.seed} inputmode="numeric" placeholder={t('img.seed_random')} disabled={busy} />
        </Field>
        <Field label={t('img.cfg_label')} id="comfy-cfg">
          <Input id="comfy-cfg" bind:value={form.cfgScale} inputmode="decimal" placeholder={t('comfy.workflow_value')} disabled={busy} />
        </Field>
      </div>
    </AdvancedOnly>
    <div class="flex flex-wrap items-center gap-3">
      {#if busy}
        <Button size="sm" variant="destructive" disabled={!promptId || cancelling} onclick={cancel}>
          <Square />{t('comfy.cancel')}
        </Button>
        <div class="flex min-w-48 flex-1 flex-col gap-1" role="status" aria-live="polite">
          <span class="flex items-center gap-2 text-xs text-muted-foreground">
            <LoaderCircle class="size-3.5 animate-spin" />{stageText(progress)}
          </span>
          {#if percent !== null}
            <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent} aria-label={t('comfy.progress')}>
              <div class="h-full bg-primary transition-[width]" style={`width: ${percent}%`}></div>
            </div>
          {/if}
        </div>
      {:else}
        <Button size="sm" onclick={run} disabled={!canRun}>
          <ImageIcon />{t('comfy.run')}
        </Button>
        {#if !status}
          <span class="text-xs text-muted-foreground">{t('comfy.need_detect')}</span>
        {:else if form.prompt.trim() === ''}
          <span class="text-xs text-muted-foreground">{t('img.need_prompt')}</span>
        {/if}
      {/if}
    </div>
  </section>

  {#if error}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">{error}</div>
  {/if}

  <section class="flex flex-col gap-3" aria-labelledby="comfy-results">
    <h3 id="comfy-results" class="text-sm font-semibold text-foreground">{t('comfy.results_title')}</h3>
    {#if form.comfyResults.length === 0}
      <p class="text-sm text-muted-foreground">{t('comfy.results_empty')}</p>
    {:else}
      <ul class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {#each form.comfyResults as item (item.id)}
          <li class="flex flex-col overflow-hidden rounded-lg border border-border bg-muted/20">
            <img src={dataUrl(item.image)} alt={item.prompt} class="aspect-square w-full bg-muted object-contain" />
            <div class="flex flex-col gap-1.5 p-2.5">
              <p class="line-clamp-2 text-xs text-foreground [overflow-wrap:anywhere]" title={item.prompt}>{item.prompt}</p>
              <p class="truncate font-num text-xs text-muted-foreground" title={item.path}>{item.workflow} · {t('comfy.seed_value', { seed: item.seed })}</p>
              <div class="flex items-center gap-1.5">
                <Button variant="outline" size="sm" onclick={() => open(item.path, false)}><ExternalLink />{t('comfy.open')}</Button>
                <Button variant="ghost" size="sm" onclick={() => open(item.path, true)}><FolderOpen />{t('comfy.reveal')}</Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onclick={() => (form.comfyResults = form.comfyResults.filter((r) => r.id !== item.id))}
                  aria-label={t('comfy.remove')}
                  title={t('comfy.remove')}
                >
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
