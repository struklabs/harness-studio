<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { localizeError } from '#lib/network.js';
  import { untrack } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { toast } from 'svelte-sonner';
  import { formatBytes } from '#lib/format.js';
  import { defaultLoadFit, toHardwareFit } from '#lib/fit.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { onnxUnsupportedText } from '#lib/engines.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { downloads } from '#lib/stores/downloads.svelte.js';
  import { isRepoFolder, library, normPath, repoRelative } from '#lib/stores/library.svelte.js';
  import type { FitResult, HardwareFitResult, HfAccess, HfModelSearchResult, HfModelVariant, ImageBundlePlan, LocalModel, ModelArtifactUpdate, SystemHardwareInfo } from '#lib/types.js';
  import * as Sheet from '#lib/components/ui/sheet/index.js';
  import * as Table from '#lib/components/ui/table/index.js';
  import * as Tabs from '#lib/components/ui/tabs/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Skeleton } from '#lib/components/ui/skeleton/index.js';
  import { cn } from '#lib/utils.js';
  import { isPending, progressPercent } from './downloadQueue.svelte';
  import { clearPageCache } from './hubData';
  import { formatName, isPreciseRefusal, notRunnableKey } from './inference';
  import { canConvert, speechNeedsConversion } from './convertible';
  import { needsPrismFork } from './forkQuants';
  import ConvertPanel from './ConvertPanel.svelte';
  import EngineNeeded from '../common/EngineNeeded.svelte';
  import ModelCard from './ModelCard.svelte';
  import PublisherAvatar from './PublisherAvatar.svelte';
  import { ggmlTwinFor, isTwinFile } from './ct2Twin';
  import { isCompanionVariant, isCtcVariant, isWhisperVariant, pickProjectorVariant, recommendCrispasrVariant, recommendCtcVariant, recommendFloatVariant, recommendVariant, recommendWhisperVariant, whisperFitsMemory, whisperOnCpu } from './recommend';
  import { security } from '#lib/stores/security.svelte.js';
  import { AlertCircle, Ban, CircleCheck, Download, ExternalLink, Info, LoaderCircle, LogIn, Play, RefreshCw, ShieldAlert } from '@lucide/svelte';

  interface Props {
    /** Selected repo; null closes the sheet. */
    repo: HfModelSearchResult | null;
    hardware: SystemHardwareInfo | null;
    quantFilter: string;
    sizeFilter: string;
    /** File of this repo to preselect and highlight (the ggml twin the user was sent to). */
    focusFile?: string | null;
    /** Opens another repo of the Hub, optionally with a file preselected. */
    onOpenRepo?: (id: string, file?: string) => void;
    onClose: () => void;
    onClearFilters: () => void;
  }

  let { repo, hardware, quantFilter, sizeFilter, focusFile = null, onOpenRepo, onClose, onClearFilters }: Props = $props();

  let tab = $state<'files' | 'card'>('files');
  let isLoadingVariants = $state(false);
  let variants = $state<HfModelVariant[]>([]);
  let variantsError = $state<string | null>(null);
  let variantsToken = 0;

  // Filled by <ModelCard> for the selected repo.
  let repoAccess = $state<HfAccess | null>(null);
  let repoGated = $state<string | null>(null);
  let repoLicense = $state<string | null>(null);
  let checkingAccess = $state(false);
  let modelCard = $state<ReturnType<typeof ModelCard> | null>(null);
  /** "Retry" ran and access is still missing. */
  let recheckedNoAccess = $state(false);
  let gateLicense = $derived(repoAccess?.license ?? repoLicense ?? repo?.license ?? null);

  async function recheckAccess() {
    recheckedNoAccess = false;
    await modelCard?.recheckAccess();
    recheckedNoAccess = repoAccess?.has_access === false;
  }

  function goSignIn() {
    onClose();
    security.openSettings('network');
  }

  /** Variant names whose enqueue call is in flight. */
  let enqueueing = $state<string[]>([]);
  /** Variant awaiting the "gated, no access" confirmation. */
  let gateConfirmFor = $state<string | null>(null);
  let checkingUpdateFor = $state<string | null>(null);
  let artifactUpdateMsg = $state<string | null>(null);

  let repoId = $derived(repo?.id ?? null);
  /** A CTranslate2 export whose ggml build the whisper.cpp engine runs. */
  let ggmlTwin = $derived(repo?.compatibility.status === 'unsupported' || repo?.format === 'ctranslate2' ? ggmlTwinFor(repo?.id) : null);

  let unsupportedFormat = $derived(repo?.compatibility.status === 'unsupported' ? repo.compatibility.format : null);
  /** Its runtime has no build for this OS / architecture (ONNX on an Intel Mac): no download either. */
  let platformBlocked = $derived(repo?.compatibility.status === 'unsupported_platform' ? repo.compatibility : null);
  /** A safetensors chat model: instead of the unsupported notice the drawer offers the conversion. */
  let convertible = $derived(canConvert(repo));
  /** The repo needs an engine family (whisper.cpp, stable-diffusion.cpp, sherpa-onnx…) that was not installed from this drawer yet. */
  let installedHere = $state<string[]>([]);
  let neededFamily = $derived(
    repo?.compatibility.status === 'needs_runtime' && repo.compatibility.runtime !== 'mistral.rs' && !installedHere.includes(repo.compatibility.runtime)
      ? repo.compatibility.runtime
      : null
  );
  function engineInstalled(family: string) {
    installedHere = [...installedHere, family];
    clearPageCache();
  }

  /** A safetensors chat model whose mistral.rs runtime is not registered (no managed install). */
  let needsMistralrs = $derived(repo?.compatibility.status === 'needs_runtime' && repo.compatibility.runtime === 'mistral.rs');
  /** A variant of this repo uses a PrismML-fork quant (PQ2_0 / PTQ1_0): the official llama.cpp can't load it. */
  let needsPrism = $derived(variants.some((v) => needsPrismFork(v.quantization, ...v.shards.map((s) => s.filename))));
  function openRuntimeSettings() {
    onClose();
    security.openSettings('runtime-registry');
  }

  /**
   * An image repo of stable-diffusion.cpp: its files are checkpoints and companions, not GGUF
   * quantizations. A diffusers pipeline (Python engine) is one download of its folders instead.
   */
  let isImageRepo = $derived(
    !!repo?.capabilities?.includes('image_generation') &&
      !(repo.format === 'diffusers' && 'runtime' in repo.compatibility && repo.compatibility.runtime === 'python-hf')
  );
  /** What the first main model file of an image repo needs besides itself (the curated bundle). */
  let bundlePlan = $state<ImageBundlePlan | null>(null);
  let bundleToken = 0;
  $effect(() => {
    const file = isImageRepo ? variants.find((v) => v.quantization !== 'component')?.shards[0]?.filename : undefined;
    const my = ++bundleToken;
    if (!file) {
      bundlePlan = null;
      return;
    }
    invoke<ImageBundlePlan | null>('get_image_bundle_plan', { file })
      .then((plan) => {
        if (my === bundleToken) bundlePlan = plan;
      })
      .catch(() => {
        if (my === bundleToken) bundlePlan = null;
      });
  });
  let bundleMissing = $derived((bundlePlan?.components ?? []).filter((c) => !c.path));
  let bundleBytes = $derived(bundleMissing.reduce((sum, c) => sum + c.size, 0));
  /** The VAE / text encoder file of a bundle component, as the repos name them. */
  const COMPONENT_LABEL: Record<string, TKey> = {
    vae: 'imgb.role_vae',
    clip_l: 'imgb.role_clip_l',
    clip_g: 'imgb.role_clip_g',
    t5xxl: 'imgb.role_t5xxl',
    llm: 'imgb.role_llm',
  };

  /** Fit per variant, keyed by variant name (filled below). */
  let variantFits = $state(new Map<string, HardwareFitResult>());

  /** Best-fitting variant for this machine (same estimate as the Fit column). */
  /** whisper.cpp ggml repo: fit is a memory check, not a GGUF VRAM estimate. */
  let isWhisperRepo = $derived(variants.length > 0 && variants.every(isWhisperVariant));
  /** ONNX CTC speech repo: precision matters more than size (see `recommendCtcVariant`). */
  let isCtcRepo = $derived(variants.length > 0 && variants.every(isCtcVariant));
  let mixedAudioFamilies = $derived(repo?.compatibility && 'runtime' in repo.compatibility && repo.compatibility.runtime === 'audio.cpp' && new Set(variants.filter(v => v.name.includes('/')).map(v => v.name.split('/')[0])).size > 1);
  let recommended = $derived(
    mixedAudioFamilies ? null : repo?.compatibility && 'runtime' in repo.compatibility && repo.compatibility.runtime === 'crispasr'
      ? recommendCrispasrVariant(variants, hardware?.available_ram_bytes, hardware?.gpus.reduce((sum, gpu) => sum + (gpu.vram_bytes ?? 0), 0))
      : isWhisperRepo
      ? recommendWhisperVariant(variants, hardware?.available_ram_bytes, whisperOnCpu(hardware?.os))
      : isCtcRepo
        ? recommendCtcVariant(variants, hardware?.available_ram_bytes)
        : (() => {
            const runnable = variants.filter(v => !v.not_runnable);
            const fits = new Map([...variantFits].map(([name, fit]) => [name, fit.category]));
            return recommendVariant(runnable, fits) ?? recommendFloatVariant(runnable, fits);
          })()
  );
  $effect(() => {
    const id = repoId;
    const q = recommended?.quantization;
    if (!id) return;
    if (!q) {
      untrack(() => {
        if (id in library.recommended) {
          const { [id]: _previous, ...rest } = library.recommended;
          library.recommended = rest;
        }
      });
      return;
    }
    untrack(() => {
      if (library.recommended[id] !== q) library.recommended = { ...library.recommended, [id]: q };
    });
  });

  /** Local files of this repo keyed by their path inside the repo (lower case). */
  let localByFile = $derived.by(() => {
    const map = new Map<string, LocalModel>();
    if (!repoId) return map;
    for (const m of library.forRepo(repoId)) {
      const rel = repoRelative(m, repoId);
      if (rel) map.set(rel, m);
    }
    return map;
  });
  /** The repo downloaded as a whole folder (MLX, Python engine): its one variant is that folder. */
  let localFolder = $derived(repoId ? library.forRepo(repoId).find((m) => isRepoFolder(m, repoId)) : undefined);
  function localFor(v: HfModelVariant): LocalModel | undefined {
    const file = v.shards[0]?.filename;
    return (file ? localByFile.get(normPath(file).toLowerCase()) : undefined) ?? (variants.length === 1 ? localFolder : undefined);
  }
  function quantLabel(v: HfModelVariant): string {
    const local = localFor(v);
    for (const q of [local?.gguf?.quantization, local?.quantization, v.quantization]) {
      if (q && q.toLowerCase() !== 'unknown') return q;
    }
    return '–';
  }
  /** Downloaded variants with a newer upstream version (checked lazily, once per opened repo). */
  let updateFlags = $state<Record<string, boolean>>({});
  let updateToken = 0;
  $effect(() => {
    const id = repoId;
    const list = variants;
    void localByFile;
    if (!id || isLoadingVariants || list.length === 0) return;
    untrack(() => void checkDownloadedUpdates(id, list));
  });
  async function checkDownloadedUpdates(id: string, list: HfModelVariant[]) {
    const targets = list.filter((v) => localFor(v) && updateFlags[`${id}|${v.name}`] === undefined).slice(0, 8);
    if (targets.length === 0) return;
    const my = ++updateToken;
    for (const v of targets) {
      const file = v.shards[0]?.filename;
      if (!file) continue;
      try {
        const res: ModelArtifactUpdate = await invoke('check_model_artifact_update', { repoId: id, filename: file });
        if (my !== updateToken) return;
        updateFlags = { ...updateFlags, [`${id}|${v.name}`]: res.has_update };
        const any = Object.entries(updateFlags).some(([k, val]) => val && k.startsWith(`${id}|`));
        if (library.updates[id] !== any) library.updates = { ...library.updates, [id]: any };
      } catch {
        // Offline / rate limited: the manual check button still works.
      }
    }
  }

  $effect(() => {
    const id = repoId;
    untrack(() => {
      if (id) void loadVariants(id);
      else {
        variantsToken++;
        variants = [];
      }
    });
  });

  async function loadVariants(id: string, refresh = false) {
    if (!refresh) tab = 'files';
    isLoadingVariants = true;
    variantsError = null;
    artifactUpdateMsg = null;
    gateConfirmFor = null;
    recheckedNoAccess = false;
    const token = ++variantsToken;

    // No Harness runtime runs this format (here): there is nothing to list or download.
    if (repo?.compatibility.status === 'unsupported' || repo?.compatibility.status === 'unsupported_platform') {
      variants = [];
      isLoadingVariants = false;
      return;
    }

    // Fast-prefill from repo quants so the user instantly sees quants without waiting.
    const quants = repo?.quants ?? [];
    if (quants.length > 0) {
      const baseName = id.split('/')[1] || id;
      variants = quants.map((q) => ({
        name: `${baseName}-${q}`,
        quantization: q,
        total_size_bytes: 0,
        is_split: false,
        shards: [],
      }));
    } else {
      variants = [];
    }

    try {
      const res: HfModelVariant[] = await invoke('get_hf_model_variants', { repoId: id });
      if (token === variantsToken) variants = res;
    } catch (e) {
      if (token === variantsToken && variants.length === 0) {
        variantsError = t('hub.variants_load_failed', { error: String(e) });
      }
    } finally {
      if (token === variantsToken) isLoadingVariants = false;
    }
  }

  /** Gated repo without confirmed access: warn before queuing. */
  let needsGateConfirm = $derived(!!repoGated && repoAccess?.has_access !== true);

  function requestDownload(variant: HfModelVariant) {
    if (isLoadingVariants || variant.not_runnable) return;
    if (needsGateConfirm) {
      gateConfirmFor = variant.name;
      return;
    }
    void enqueueVariant(variant);
  }

  async function enqueueVariant(variant: HfModelVariant) {
    gateConfirmFor = null;
    if (variant.not_runnable) return;
    const id = repoId;
    if (!id) return;
    enqueueing = [...enqueueing, variant.name];
    try {
      await downloads.enqueueVariant(id, variant);
      toast.success(t('hub.announce_queued', { name: variant.name }));
      // An image model of the curated list brings its VAE and text encoders with it.
      const file = variant.shards[0]?.filename;
      if (isImageRepo && file && variant.quantization !== 'component') {
        const jobs = await invoke<number[]>('download_image_companions', { file });
        if (jobs.length > 0) {
          await downloads.refresh();
          toast.success(t('imgb.companions_queued', { count: jobs.length }));
        }
      }
      // A vision repo's main model brings its projector (mmproj), unless it is already local.
      const projector = pickProjectorVariant(variants, variant);
      const projectorFile = projector?.shards[0]?.filename;
      if (projector && projectorFile && !localByFile.has(normPath(projectorFile).toLowerCase())) {
        try {
          await downloads.enqueueVariant(id, projector);
          toast.success(t('hub.projector_queued', { name: projector.name, size: formatBytes(projector.total_size_bytes) }));
        } catch (e) {
          console.warn('projector download', e);
        }
      }
    } catch (e) {
      toast.error(t('hub.download_start_failed', { error: String(e) }));
    } finally {
      enqueueing = enqueueing.filter((n) => n !== variant.name);
    }
  }

  async function checkVariantUpdate(variant: HfModelVariant) {
    const id = repoId;
    if (!id) return;
    const fileToCheck = variant.shards[0]?.filename || `${variant.name}.gguf`;
    checkingUpdateFor = variant.name;
    artifactUpdateMsg = null;
    try {
      const res: ModelArtifactUpdate = await invoke('check_model_artifact_update', { repoId: id, filename: fileToCheck });
      artifactUpdateMsg = t('hub.update_check_result', { name: variant.name, message: localizeError(res.message) });
      if (localFor(variant)) updateFlags = { ...updateFlags, [`${id}|${variant.name}`]: res.has_update };
    } catch (e) {
      artifactUpdateMsg = t('hub.update_check_failed', { error: String(e) });
    } finally {
      checkingUpdateFor = null;
    }
  }

  async function refreshAfterAccess(id: string) {
    if (repoId === id) await loadVariants(id, true);
  }

  function openHf(path: string) {
    modelCard?.markLicensePageOpened();
    openUrl(`https://huggingface.co/${path}`).catch((e) => toast.error(t('hub.open_link_failed', { error: String(e) })));
  }

  // Fit per variant (Rust estimate, one batched call), once per list/hardware change.
  $effect(() => {
    const list = variants;
    const ctx = app.settings?.server.default_ctx || 32768;
    void hardware;
    const sized = list.filter((v) => !v.not_runnable && v.total_size_bytes > 0);
    if (isImageRepo) {
      // stable-diffusion.cpp's estimate (the llama.cpp one read these files as 32k-context chat
      // models); a main file loads with every component of its bundle, downloaded or not.
      const companions = (bundlePlan?.components ?? []).reduce((sum, c) => sum + c.size, 0);
      const size = (v: HfModelVariant) => v.total_size_bytes + (isCompanionVariant(v) ? 0 : companions);
      invoke<FitResult[]>('estimate_image_fit', { sizes: sized.map(size) })
        .then((results) => {
          if (list === untrack(() => variants)) variantFits = new Map(sized.map((v, i) => [v.name, toHardwareFit(results[i])]));
        })
        .catch(() => {});
      return;
    }
    Promise.all(
      sized.map((v) =>
        // As a default load starts it (automatic KV, fewer GPU layers when the full offload
        // does not fit). An `.onnx` name routes the estimate to the ONNX encoder engine (the
        // llama.cpp estimate reads a small encoder as a 32k-context LLM).
        defaultLoadFit({
          model_path: onnxFile(v),
          model_size_bytes: v.total_size_bytes,
          context_size: ctx,
          gpu_layers: 999,
        }).then(
          (r) => [v.name, toHardwareFit(r)] as const
        )
      )
    )
      .then((pairs) => {
        if (list === untrack(() => variants)) variantFits = new Map(pairs);
      })
      .catch(() => {});
  });

  function onnxFile(v: HfModelVariant): string | null {
    const file = v.shards[0]?.filename;
    return file && file.toLowerCase().endsWith('.onnx') ? file : null;
  }

  const FIT_TONE: Record<string, string> = {
    full_gpu: 'text-success',
    tight: 'text-warning',
    hybrid: 'text-warning',
    cpu_heavy: 'text-muted-foreground',
    oom_risk: 'text-destructive',
  };

  let filteredVariants = $derived.by(() =>
    variants.filter((v) => {
      if (quantFilter !== 'ALL') {
        const q = v.quantization.toUpperCase();
        if (quantFilter === 'IQ3') {
          if (!q.startsWith('IQ3')) return false;
        } else if (quantFilter === 'BF16/FP16') {
          if (!q.includes('16') && !q.includes('BF16') && !q.includes('FP16')) return false;
        } else if (q !== quantFilter) {
          return false;
        }
      }
      if (sizeFilter !== 'ALL') {
        const gb = v.total_size_bytes / (1024 * 1024 * 1024);
        if (sizeFilter === '<4B' && gb >= 3.5) return false;
        if (sizeFilter === '4B-9B' && (gb < 3.5 || gb > 7.5)) return false;
        if (sizeFilter === '10B-20B' && (gb <= 7.5 || gb > 16.0)) return false;
        if (sizeFilter === '30B+' && gb <= 16.0) return false;
      }
      return true;
    })
  );
  let showAllFiles = $state(false);
  $effect(() => {
    void repoId;
    showAllFiles = false;
  });
  let visibleVariants = $derived.by(() => {
    if (uiLevel.advanced || showAllFiles) return filteredVariants;
    const focused = focusFile ? filteredVariants.find((v) => isTwinFile(v.shards[0]?.filename ?? v.name, focusFile)) : undefined;
    if (focused) return [focused];
    const choice = filteredVariants.find((v) => v.name === recommended?.name) ?? filteredVariants.find((v) => !isCompanionVariant(v)) ?? filteredVariants[0];
    return choice ? [choice] : [];
  });
  /** Scrolls the preselected row into view once it is rendered. */
  function focusRow(focused: boolean) {
    return (el: HTMLElement) => {
      if (focused) el.scrollIntoView({ block: 'center' });
    };
  }
  let filtersActive = $derived(quantFilter !== 'ALL' || sizeFilter !== 'ALL');
</script>

<Sheet.Root open={!!repo} onOpenChange={(o) => !o && onClose()}>
  <Sheet.Content side="right" class="w-[min(680px,94vw)] gap-0 p-0 data-[side=right]:sm:max-w-none">
    {#if repo}
      <Sheet.Header class="gap-1 border-b border-border pr-12">
        <div class="flex min-w-0 items-center gap-3">
          <PublisherAvatar name={repo.id.includes('/') ? repo.id.split('/')[0] : (repo.author ?? '')} class="size-10" />
          <Sheet.Title class="truncate text-base font-semibold" title={repo.id}>{repo.id}</Sheet.Title>
        </div>
        <Sheet.Description class="flex items-center gap-2 text-xs">
          <span>{t('hub.sheet_desc')}</span>
          <AdvancedOnly><button
            type="button"
            class="inline-flex items-center gap-1 text-xs text-muted-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
            onclick={() => openHf(repo.id)}
          >
            {t('hub.card_open_hf')}
            <ExternalLink class="size-3" />
          </button></AdvancedOnly>
        </Sheet.Description>
      </Sheet.Header>

      <Tabs.Root bind:value={tab} class="min-h-0 flex-1 gap-0">
        <div class="border-b border-border px-4 py-2">
          <Tabs.List variant="line">
            <Tabs.Trigger value="files">
              {t('hub.tab_files')}
              <span class="font-num text-xs text-muted-foreground">{variants.length}</span>
            </Tabs.Trigger>
            <Tabs.Trigger value="card">{t('hub.tab_card')}</Tabs.Trigger>
          </Tabs.List>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto p-4">
          <!-- Gated repo status (both tabs) -->
          {#if repoGated}
            <div class="mb-4" aria-live="polite">
              {#if checkingAccess && !repoAccess}
                <div class="flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
                  <LoaderCircle class="size-3.5 animate-spin" />{t('hub.gate_checking')}
                </div>
              {:else if repoAccess?.has_access === true}
                <Badge variant="outline" class="text-success"><CircleCheck />{t('hfauth.gate_ok')}</Badge>
              {:else}
                <div class="flex flex-col gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2.5 text-xs" role="alert">
                  <div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
                    <span class="inline-flex items-center gap-1.5 font-medium text-warning">
                      <ShieldAlert class="size-3.5 shrink-0" />{t('hfauth.gate_title')}
                    </span>
                    {#if gateLicense}
                      <span class="font-mono text-muted-foreground">{t('hfauth.gate_license', { license: gateLicense })}</span>
                    {/if}
                  </div>
                  <p class="text-foreground">
                    {#if repoAccess && !repoAccess.token_present}
                      {t('hfauth.gate_sign_in_needed')}
                    {:else if repoAccess?.has_access === false}
                      {t('hfauth.gate_accept_needed')}
                    {:else if repoAccess?.reason}
                      {localizeError(repoAccess.reason)}
                    {:else}
                      {t('hub.gate_unknown')}
                    {/if}
                  </p>
                  {#if repoGated === 'manual'}
                    <p class="text-muted-foreground">{t('hfauth.gate_manual')}</p>
                  {/if}
                  {#if recheckedNoAccess}
                    <p class="text-muted-foreground">{t('hfauth.gate_still_no_access')}</p>
                  {/if}
                  <div class="flex flex-wrap items-center gap-2">
                    <Button variant="outline" size="xs" onclick={() => openHf(repo.id)}>
                      <ExternalLink />{t('hfauth.gate_accept')}
                    </Button>
                    {#if repoAccess && !repoAccess.token_present}
                      <Button variant="outline" size="xs" onclick={goSignIn}><LogIn />{t('hfauth.gate_go_sign_in')}</Button>
                    {/if}
                    <Button variant="ghost" size="xs" disabled={checkingAccess} onclick={recheckAccess}>
                      <RefreshCw class={cn(checkingAccess && 'animate-spin')} />{t('hfauth.gate_retry')}
                    </Button>
                  </div>
                </div>
              {/if}
            </div>
          {/if}

          <div class={cn(tab !== 'card' && 'hidden')}>
            <ModelCard
              bind:this={modelCard}
              onAccessGranted={refreshAfterAccess}
              repoId={repo.id}
              gatedHint={repo.gated ?? null}
              bind:access={repoAccess}
              bind:gated={repoGated}
              bind:license={repoLicense}
              bind:checkingAccess
            />
          </div>

          {#if tab === 'files'}
            <div class="flex flex-col gap-3">
              {#if needsMistralrs}
                <div class="flex items-center gap-2 rounded-lg border border-info/30 bg-info/10 px-3 py-2 text-xs text-info" role="status">
                  <Info class="size-3.5 shrink-0" />
                  <span class="min-w-0 flex-1">{t('hub.compat_needs_mistralrs_drawer')}</span>
                  <Button variant="outline" size="xs" class="shrink-0" onclick={openRuntimeSettings}>
                    {t('hub.compat_open_runtime_settings')}
                  </Button>
                </div>
              {/if}

              {#if needsPrism}
                <div class="flex items-center gap-2 rounded-lg border border-info/30 bg-info/10 px-3 py-2 text-xs text-info" role="status">
                  <Info class="size-3.5 shrink-0" />
                  <span class="min-w-0 flex-1">{t('hub.needs_prism_fork')}</span>
                  <Button variant="outline" size="xs" class="shrink-0" onclick={openRuntimeSettings}>
                    {t('hub.compat_open_runtime_settings')}
                  </Button>
                </div>
              {/if}

              {#if neededFamily}
                <div class="flex items-center gap-2 rounded-lg border border-info/30 bg-info/10 px-3 py-2 text-xs text-info" role="status">
                  <Info class="size-3.5 shrink-0" />
                  <span class="min-w-0 flex-1">{t('eng.drawer_note', { runtime: neededFamily })}</span>
                  <EngineNeeded family={neededFamily} hint={false} onInstalled={() => engineInstalled(neededFamily)} />
                </div>
              {/if}
              {#if isImageRepo && bundlePlan && bundlePlan.components.length > 0}
                <div class="flex flex-col gap-1 rounded-lg border border-border bg-muted/40 px-3 py-2 text-xs" role="note">
                  <span class="font-medium text-foreground">{t('imgb.title', { name: bundlePlan.label })}</span>
                  <span class="text-muted-foreground">
                    {bundleMissing.length > 0
                      ? t('imgb.missing', { size: formatBytes(bundleBytes) })
                      : t('imgb.complete')}
                  </span>
                  <ul class="flex flex-col gap-0.5">
                    {#each bundlePlan.components as c (c.role)}
                      <li class="flex items-center justify-between gap-2">
                        <span class="min-w-0 truncate">{t(COMPONENT_LABEL[c.role] ?? 'imgb.role_other')} <span class="font-mono text-muted-foreground" title={`${c.repo}/${c.file}`}>{c.file.split('/').pop()}</span></span>
                        <span class="shrink-0 font-num {c.path ? 'text-success' : 'text-muted-foreground'}">
                          {c.path ? t('imgb.present') : formatBytes(c.size)}
                        </span>
                      </li>
                    {/each}
                  </ul>
                </div>
              {:else if isImageRepo && variants.length > 0}
                <div class="rounded-lg border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground" role="note">
                  {t('imgb.unknown_bundle')}
                </div>
              {/if}

              {#if filtersActive}
                <div class="flex items-center justify-between gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2 text-xs">
                  <span class="text-muted-foreground">
                    {t('hub.filter_active_notice', { count: filteredVariants.length, total: variants.length })}
                  </span>
                  <Button variant="ghost" size="xs" onclick={onClearFilters}>{t('hub.show_all_quants', { count: variants.length })}</Button>
                </div>
              {/if}

              {#if artifactUpdateMsg}
                <div class="rounded-lg border border-border bg-muted/40 px-3 py-2 text-xs" role="status">{artifactUpdateMsg}</div>
              {/if}

              {#if gateConfirmFor}
                <div class="flex flex-col gap-2 rounded-lg border border-warning/30 bg-warning/10 p-3 text-xs" role="alert">
                  <span>{t('hub.gate_confirm_text')}</span>
                  <div class="flex gap-2">
                    <Button
                      variant="outline"
                      size="xs"
                      onclick={() => {
                        const v = variants.find((x) => x.name === gateConfirmFor);
                        if (v) void enqueueVariant(v);
                      }}>{t('hub.gate_confirm_yes')}</Button
                    >
                    <Button variant="ghost" size="xs" onclick={() => (gateConfirmFor = null)}>{t('hub.cancel')}</Button>
                  </div>
                </div>
              {/if}

              {#if convertible && repoId}
                <ConvertPanel {repoId} />
              {:else if unsupportedFormat}
                <div class="flex items-start gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
                  <Ban class="mt-px size-3.5 shrink-0 text-warning" />
                  <span class="min-w-0 flex-1">{ggmlTwin ? t('hub.ct2_use_ggml') : repo.not_runnable === 'audio_cpp_unsupported' ? t('hub.not_runnable_audio_cpp') : isPreciseRefusal(repo.not_runnable) ? t(notRunnableKey(repo.not_runnable)) : speechNeedsConversion(repo) ? t('hub.asr_needs_conversion_drawer') : unsupportedFormat === 'gguf' ? t('hub.compat_gguf_other_engine_drawer') : t('hub.compat_unsupported_drawer', { format: formatName(unsupportedFormat) })}</span>
                </div>
                {#if ggmlTwin}
                  <div class="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2 text-xs">
                    <span class="flex min-w-0 items-center gap-1.5 text-muted-foreground"><Play class="size-3.5 shrink-0 text-success" />{t('hub.ct2_runs_with', { file: ggmlTwin.file })}</span>
                    <Button variant="outline" size="xs" onclick={() => onOpenRepo?.(ggmlTwin.repo, ggmlTwin.file)}>{t('hub.ct2_open_ggml')}</Button>
                  </div>
                {/if}
              {:else if platformBlocked}
                {@const msg = onnxUnsupportedText(platformBlocked)}
                <div class="flex items-start gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
                  <Ban class="mt-px size-3.5 shrink-0 text-warning" />
                  <span class="min-w-0 flex-1">{t(msg.key, msg.params)}</span>
                </div>
              {:else if isLoadingVariants && variants.length === 0}
                <div class="flex flex-col gap-2" role="status" aria-label={t('hub.scanning_files')}>
                  <Skeleton class="h-9 w-full" />
                  <Skeleton class="h-9 w-full" />
                  <Skeleton class="h-9 w-full" />
                </div>
              {:else if variantsError}
                <div class="flex items-center justify-between gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive" role="alert">
                  <span class="flex min-w-0 gap-2"><AlertCircle class="mt-px size-3.5 shrink-0" /><span class="break-words">{variantsError}</span></span>
                  <Button variant="outline" size="xs" onclick={() => repoId && loadVariants(repoId)}>{t('hub.retry_button')}</Button>
                </div>
              {:else if variants.length === 0}
                <p class="py-6 text-center text-xs text-muted-foreground">{t('hub.no_gguf_found')}</p>
              {:else if filteredVariants.length === 0}
                <div class="flex flex-col items-center gap-2 py-6 text-center text-xs text-muted-foreground">
                  <span>{t('hub.no_variant_filter_results')}</span>
                  <Button variant="outline" size="xs" onclick={onClearFilters}>{t('hub.show_all_quants', { count: variants.length })}</Button>
                </div>
              {:else}
                {#if !uiLevel.advanced && !showAllFiles && filteredVariants.length > 1}
                  <Button variant="outline" size="sm" class="self-start" onclick={() => (showAllFiles = true)}>{t('hub.all_files')}</Button>
                {/if}
                <div class="min-w-0 overflow-hidden rounded-lg border border-border">
                  <Table.Root>
                    <Table.Header>
                      <Table.Row class="hover:bg-transparent">
                        <Table.Head class="h-8 text-xs">{t(uiLevel.advanced || showAllFiles ? 'hub.col_quant' : 'hub.download_option')}</Table.Head>
                        <Table.Head class="h-8 text-right text-xs">{t('hub.col_size')}</Table.Head>
                        <Table.Head class="h-8 text-xs">{t('hub.col_fit')}</Table.Head>
                        <Table.Head class="h-8 w-px"><span class="sr-only">{t('hub.col_actions')}</span></Table.Head>
                      </Table.Row>
                    </Table.Header>
                    <Table.Body>
                      {#each visibleVariants as variant, idx (variant.name + '-' + (variant.is_split ? 'split' : 'single') + '-' + idx)}
                        {@const fit = variantFits.get(variant.name)}
                        {@const job = downloads.jobFor(repo.id, variant.name)}
                        {@const pending = !!job && isPending(job.status)}
                        {@const adding = enqueueing.includes(variant.name)}
                        {@const local = localFor(variant)}
                        {@const isRunningNow = !!local && library.isRunning(local)}
                        {@const hasUpdate = updateFlags[`${repo.id}|${variant.name}`] === true}
                        {@const isRecommended = recommended?.name === variant.name}
                        {@const isFocused = !!focusFile && isTwinFile(variant.shards[0]?.filename ?? variant.name, focusFile)}
                        <Table.Row class={isFocused ? 'bg-primary/10' : ''} data-focus={isFocused ? 'true' : undefined} {@attach focusRow(isFocused)}>
                          <Table.Cell class="max-w-[220px]">
                            <div class="flex min-w-0 flex-col">
                              <span class="flex items-center gap-1.5">
                                {#if uiLevel.advanced || showAllFiles}<span class="font-mono text-sm font-medium" title={t('hub.quant_badge_title')}>{quantLabel(variant)}</span>{:else}<span class="text-sm font-medium">{t('hub.download_option')}</span>{/if}
                                {#if variant.is_split && (uiLevel.advanced || showAllFiles)}
                                  <Badge variant="outline" class="text-xs" title={t('hub.split_desc', { count: variant.shards.length })}>
                                    {t('hub.split_set', { count: variant.shards.length })}
                                  </Badge>
                                {/if}
                              </span>
                              {#if uiLevel.advanced || showAllFiles}<span class="truncate font-mono text-xs text-muted-foreground" title={variant.name}>{variant.name}</span>{/if}
                              {#if isRecommended || local || hasUpdate}
                                <span class="mt-1 flex flex-wrap items-center gap-1">
                                  {#if isRecommended}
                                    <Badge variant="outline" class="text-xs text-muted-foreground" title={t('hub.recommended_title')}>{t('hub.recommended_badge')}</Badge>
                                  {/if}
                                  {#if isRunningNow}
                                    <Badge variant="outline" class="text-xs text-success"><Play />{t('hub.badge_running')}</Badge>
                                  {:else if local}
                                    <Badge variant="outline" class="text-xs text-success"><CircleCheck />{t('hub.badge_downloaded')}</Badge>
                                  {/if}
                                  {#if hasUpdate}
                                    <Badge variant="outline" class="text-xs text-warning"><RefreshCw />{t('hub.badge_update')}</Badge>
                                  {/if}
                                </span>
                              {/if}
                            </div>
                          </Table.Cell>
                          <Table.Cell class="text-right font-num text-xs">
                            {variant.total_size_bytes > 0 ? formatBytes(variant.total_size_bytes) : '–'}
                          </Table.Cell>
                          <Table.Cell>
                            {#if variant.not_runnable}
                              <span class="text-xs text-warning">{t(notRunnableKey(variant.not_runnable))}</span>
                            {:else if 'runtime' in repo.compatibility && repo.compatibility.runtime === 'audio.cpp'}
                              <span class="text-xs text-warning">{!app.settings?.audio_cpp_enabled ? t('audio_cpp.needs_engine') : engineInstall.installed('audio.cpp') ? t('process.experimental') : t('speak.engine_missing', { family: 'audio.cpp' })}</span>
                            {:else if isWhisperRepo}
                              {@const wfit = whisperFitsMemory(variant.total_size_bytes, hardware?.available_ram_bytes)}
                              {#if wfit === null}
                                <span class="text-xs text-muted-foreground">{variant.total_size_bytes > 0 ? '–' : t('hub.fit_calculating')}</span>
                              {:else}
                                <Tooltip.Root>
                                  <Tooltip.Trigger>
                                    {#snippet child({ props })}
                                      <span {...props} class={cn('inline-flex items-center gap-1.5 text-xs', wfit ? 'text-success' : 'text-warning')}>
                                        <span class="size-1.5 rounded-full bg-current"></span>{wfit ? t('hub.whisper_fit_ok') : t('hub.whisper_fit_tight')}
                                      </span>
                                    {/snippet}
                                  </Tooltip.Trigger>
                                  <Tooltip.Content class="max-w-64">{t('hub.whisper_fit_desc')}</Tooltip.Content>
                                </Tooltip.Root>
                              {/if}
                            {:else if fit && variant.total_size_bytes > 0}
                              <Tooltip.Root>
                                <Tooltip.Trigger>
                                  {#snippet child({ props })}
                                    <span {...props} class={cn('inline-flex items-center gap-1.5 text-xs', FIT_TONE[fit.category])}>
                                      <span class="size-1.5 rounded-full bg-current"></span>{fit.label}
                                    </span>
                                  {/snippet}
                                </Tooltip.Trigger>
                                <Tooltip.Content class="max-w-64">{fit.description}</Tooltip.Content>
                              </Tooltip.Root>
                            {:else}
                              <span class="text-xs text-muted-foreground">{t('hub.fit_calculating')}</span>
                            {/if}
                          </Table.Cell>
                          <Table.Cell>
                            {#if !variant.not_runnable}
                              <div class="flex items-center justify-end gap-1">
                                <AdvancedOnly>
                                  <Tooltip.Root>
                                    <Tooltip.Trigger>
                                      {#snippet child({ props })}
                                        <Button
                                          {...props}
                                          variant="ghost"
                                          size="icon-sm"
                                          onclick={() => checkVariantUpdate(variant)}
                                          disabled={checkingUpdateFor === variant.name}
                                          aria-label={t('hub.check_update_button')}
                                        >
                                          <RefreshCw class={cn(checkingUpdateFor === variant.name && 'animate-spin')} />
                                        </Button>
                                      {/snippet}
                                    </Tooltip.Trigger>
                                    <Tooltip.Content class="max-w-64">{t('hub.check_update_title')}</Tooltip.Content>
                                  </Tooltip.Root>
                                </AdvancedOnly>
                                <Button
                                  variant="outline"
                                  size="sm"
                                  class="min-w-24 font-num"
                                  onclick={() => requestDownload(variant)}
                                  disabled={pending || adding || isLoadingVariants}
                                >
                                  {#if adding}
                                    <LoaderCircle class="animate-spin" />
                                  {:else if job && job.status === 'Queued'}
                                    {t('hub.variant_queued')}
                                  {:else if job && pending}
                                    {progressPercent(job).toFixed(0)}%
                                  {:else}
                                    <Download />{t('hub.download_short')}
                                  {/if}
                                </Button>
                              </div>
                            {/if}
                          </Table.Cell>
                        </Table.Row>
                      {/each}
                    </Table.Body>
                  </Table.Root>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </Tabs.Root>
    {/if}
  </Sheet.Content>
</Sheet.Root>
