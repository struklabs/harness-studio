<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { formatBytes } from '$lib/format';
  import { app } from '$lib/stores/app.svelte';
  import type { LocalModel } from '$lib/types';
  import {
    CACHE_TYPES,
    gpuLayerSweep,
    parseIntList,
    parseOptionalInt,
    testCount,
    type BenchmarkRequest,
    type FlashAttnMode,
  } from '$lib/benchmark-types';
  import { Card, CardHeader, CardTitle, CardContent, CardFooter } from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import { AlertTriangle, Play, Settings } from '@lucide/svelte';
  import { cn } from '$lib/utils';

  interface Props {
    models: LocalModel[];
    modelsLoaded: boolean;
    modelsError: string | null;
    running: boolean;
    starting: boolean;
    startError: string | null;
    runtimeMissing: boolean;
    onGoToSettings?: () => void;
    onStart: (request: BenchmarkRequest) => void;
  }

  let {
    models,
    modelsLoaded,
    modelsError,
    running,
    starting,
    startError,
    runtimeMissing,
    onGoToSettings,
    onStart,
  }: Props = $props();

  const FA_MODES: FlashAttnMode[] = ['on', 'off', 'auto'];

  let modelId = $state('');

  $effect(() => {
    if (models.length > 0 && !models.some((m) => m.id === modelId)) {
      modelId = models[0]?.id ?? '';
    }
  });

  const model = $derived(models.find((m) => m.id === modelId) ?? null);

  // Form fields
  let promptSizes = $state('512');
  let genSizes = $state('128');
  let depths = $state('');
  let gpuLayers = $state('99');
  let flashAttn = $state<FlashAttnMode[]>([]);
  let cacheK = $state<string[]>([]);
  let cacheV = $state<string[]>([]);
  let threads = $state('');
  let batch = $state('');
  let ubatch = $state('');
  let reps = $state('3');

  const pp = $derived(parseIntList(promptSizes));
  const tg = $derived(parseIntList(genSizes));
  const depth = $derived(parseIntList(depths, true));
  const ngl = $derived(parseIntList(gpuLayers, true));
  const threadsP = $derived(parseOptionalInt(threads));
  const batchP = $derived(parseOptionalInt(batch));
  const ubatchP = $derived(parseOptionalInt(ubatch));
  const repsP = $derived.by(() => {
    const p = parseOptionalInt(reps);
    return { ...p, error: p.error || (p.value !== null && p.value > 50) };
  });

  const needSize = $derived(!pp.error && !tg.error && pp.values.length === 0 && tg.values.length === 0);
  const formValid = $derived(
    !!model &&
      !pp.error &&
      !tg.error &&
      !depth.error &&
      !ngl.error &&
      !threadsP.error &&
      !batchP.error &&
      !ubatchP.error &&
      !repsP.error &&
      !needSize
  );

  const request = $derived<BenchmarkRequest>({
    model_path: model?.path ?? '',
    n_prompt: pp.values,
    n_gen: tg.values,
    n_depth: depth.values,
    gpu_layers: ngl.values,
    flash_attn: flashAttn,
    cache_type_k: cacheK,
    cache_type_v: cacheV,
    threads: threadsP.value,
    batch_size: batchP.value,
    ubatch_size: ubatchP.value,
    repetitions: repsP.value,
  });

  const count = $derived(testCount(request));

  function toggle<T>(list: T[], value: T): T[] {
    return list.includes(value) ? list.filter((v) => v !== value) : [...list, value];
  }

  function applyPreset(kind: 'quick' | 'ngl' | 'fa' | 'kv') {
    if (kind === 'quick') {
      promptSizes = '512';
      genSizes = '128';
      depths = '';
      gpuLayers = '99';
      flashAttn = [];
      cacheK = [];
      cacheV = [];
      reps = '2';
    } else if (kind === 'ngl') {
      gpuLayers = gpuLayerSweep(model?.gguf?.block_count).join(', ');
    } else if (kind === 'fa') {
      flashAttn = ['on', 'off'];
    } else {
      cacheK = ['f16', 'q8_0', 'q4_0'];
      cacheV = ['f16', 'q8_0', 'q4_0'];
    }
  }

  function modelOptionLabel(m: LocalModel): string {
    const parts = [m.name, formatBytes(m.size_bytes, 1)];
    if (m.gguf?.block_count) parts.push(t('bench.layers', { count: m.gguf.block_count }));
    return parts.join(' · ');
  }

  function handleStart() {
    if (!formValid || running || starting) return;
    onStart($state.snapshot(request));
  }
</script>

<Card class="border-border overflow-visible h-auto shrink-0">
  <CardHeader class="pb-3 border-b border-border">
    <CardTitle class="text-base font-semibold">{t('bench.config_title')}</CardTitle>
  </CardHeader>
  <CardContent class="pt-4 flex flex-col gap-5 overflow-visible">
    <!-- Model Selector -->
    <div class="flex flex-col gap-1.5">
      <Label for="bench-model" class="text-xs font-medium text-foreground">{t('bench.model')}</Label>
      {#if modelsError}
        <p class="text-xs text-destructive">{t('bench.models_error', { error: modelsError })}</p>
      {:else if modelsLoaded && models.length === 0}
        <p class="text-xs text-muted-foreground">{t('bench.no_models')}</p>
      {/if}
      <select
        id="bench-model"
        bind:value={modelId}
        disabled={running || models.length === 0}
        onchange={() => (gpuLayers = '99')}
        class="h-8 rounded-lg border border-input bg-transparent px-2.5 py-1 text-xs text-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 outline-none disabled:opacity-50"
      >
        {#if models.length === 0}
          <option value="">{t('bench.model_placeholder')}</option>
        {/if}
        {#each models as m (m.id)}
          <option value={m.id}>{modelOptionLabel(m)}</option>
        {/each}
      </select>
    </div>

    <!-- Presets -->
    <div class="flex flex-wrap items-center gap-2">
      <span class="mr-1 text-xs font-medium text-muted-foreground">
        {t('bench.presets')}:
      </span>
      <Button
        type="button"
        variant="secondary"
        size="xs"
        disabled={running}
        onclick={() => applyPreset('quick')}
      >
        {t('bench.preset_quick')}
      </Button>
      <Button
        type="button"
        variant="secondary"
        size="xs"
        disabled={running}
        onclick={() => applyPreset('ngl')}
      >
        {t('bench.preset_ngl')}
      </Button>
      <Button
        type="button"
        variant="secondary"
        size="xs"
        disabled={running}
        onclick={() => applyPreset('fa')}
      >
        {t('bench.preset_fa')}
      </Button>
      <Button
        type="button"
        variant="secondary"
        size="xs"
        disabled={running}
        onclick={() => applyPreset('kv')}
      >
        {t('bench.preset_kv')}
      </Button>
    </div>

    <!-- Carbon-style Form Grid: Primary Parameters -->
    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
      <div class="flex flex-col gap-1">
        <Label for="bench-pp" class="text-xs font-medium text-foreground">{t('bench.prompt_sizes')}</Label>
        <Input
          id="bench-pp"
          type="text"
          inputmode="numeric"
          bind:value={promptSizes}
          aria-invalid={pp.error}
          placeholder="512"
          disabled={running}
          class="h-8 font-num text-xs"
        />
        <span class="text-xs text-muted-foreground">{t('bench.list_hint')}</span>
        {#if pp.error}
          <span class="text-xs text-destructive">{t('bench.invalid_list')}</span>
        {/if}
      </div>

      <div class="flex flex-col gap-1">
        <Label for="bench-tg" class="text-xs font-medium text-foreground">{t('bench.gen_sizes')}</Label>
        <Input
          id="bench-tg"
          type="text"
          inputmode="numeric"
          bind:value={genSizes}
          aria-invalid={tg.error}
          placeholder="128"
          disabled={running}
          class="h-8 font-num text-xs"
        />
        <span class="text-xs text-muted-foreground">{t('bench.list_hint')}</span>
        {#if tg.error}
          <span class="text-xs text-destructive">{t('bench.invalid_list')}</span>
        {/if}
      </div>

      <div class="flex flex-col gap-1">
        <Label for="bench-depth" class="text-xs font-medium text-foreground">{t('bench.depths')}</Label>
        <Input
          id="bench-depth"
          type="text"
          inputmode="numeric"
          bind:value={depths}
          aria-invalid={depth.error}
          placeholder="0"
          disabled={running}
          class="h-8 font-num text-xs"
        />
        <span class="text-xs text-muted-foreground">{t('bench.depth_hint')}</span>
        {#if depth.error}
          <span class="text-xs text-destructive">{t('bench.invalid_list_zero')}</span>
        {/if}
      </div>

      <div class="flex flex-col gap-1">
        <Label for="bench-ngl" class="text-xs font-medium text-foreground">{t('bench.gpu_layers')}</Label>
        <Input
          id="bench-ngl"
          type="text"
          inputmode="numeric"
          bind:value={gpuLayers}
          aria-invalid={ngl.error}
          placeholder="99"
          disabled={running}
          class="h-8 font-num text-xs"
        />
        <span class="text-xs text-muted-foreground">{t('bench.ngl_hint')}</span>
        {#if ngl.error}
          <span class="text-xs text-destructive">{t('bench.invalid_list_zero')}</span>
        {/if}
      </div>
    </div>

    {#if needSize}
      <div class="text-xs text-destructive font-medium px-1" role="alert">
        {t('bench.need_size')}
      </div>
    {/if}

    <!-- Flash Attention Chips -->
    <div class="flex flex-col gap-1.5">
      <div class="flex items-center justify-between">
        <Label class="text-xs font-medium text-foreground">{t('bench.flash_attn')}</Label>
        <span class="text-xs text-muted-foreground">{t('bench.fa_default_hint')}</span>
      </div>
      <div class="flex flex-wrap gap-1.5">
        {#each FA_MODES as mode (mode)}
          {@const active = flashAttn.includes(mode)}
          <button
            type="button"
            disabled={running}
            class={cn(
              'px-2.5 py-1 rounded-md text-xs font-medium border transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed',
              active
                ? 'bg-primary text-primary-foreground border-primary'
                : 'bg-muted/40 hover:bg-muted text-muted-foreground hover:text-foreground border-border'
            )}
            onclick={() => (flashAttn = toggle(flashAttn, mode))}
          >
            {mode}
          </button>
        {/each}
      </div>
    </div>

    <!-- Cache K & Cache V Chips -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
      <div class="flex flex-col gap-1.5">
        <div class="flex items-center justify-between">
          <Label class="text-xs font-medium text-foreground">{t('bench.cache_k')}</Label>
          <span class="text-xs text-muted-foreground">{t('bench.cache_hint')}</span>
        </div>
        <div class="flex flex-wrap gap-1.5">
          {#each CACHE_TYPES as ct (ct)}
            {@const active = cacheK.includes(ct)}
            <button
              type="button"
              disabled={running}
              class={cn(
                'px-2 py-0.5 rounded-md text-xs font-mono border transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed',
                active
                  ? 'bg-primary text-primary-foreground border-primary font-semibold'
                  : 'bg-muted/40 hover:bg-muted text-muted-foreground hover:text-foreground border-border'
              )}
              onclick={() => (cacheK = toggle(cacheK, ct))}
            >
              {ct}
            </button>
          {/each}
        </div>
      </div>

      <div class="flex flex-col gap-1.5">
        <div class="flex items-center justify-between">
          <Label class="text-xs font-medium text-foreground">{t('bench.cache_v')}</Label>
          <span class="text-xs text-muted-foreground">{t('bench.cache_hint')}</span>
        </div>
        <div class="flex flex-wrap gap-1.5">
          {#each CACHE_TYPES as ct (ct)}
            {@const active = cacheV.includes(ct)}
            <button
              type="button"
              disabled={running}
              class={cn(
                'px-2 py-0.5 rounded-md text-xs font-mono border transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed',
                active
                  ? 'bg-primary text-primary-foreground border-primary font-semibold'
                  : 'bg-muted/40 hover:bg-muted text-muted-foreground hover:text-foreground border-border'
              )}
              onclick={() => (cacheV = toggle(cacheV, ct))}
            >
              {ct}
            </button>
          {/each}
        </div>
      </div>
    </div>

    <!-- Secondary Execution Parameters -->
    <div class="grid grid-cols-2 sm:grid-cols-4 gap-3">
      <div class="flex flex-col gap-1">
        <Label for="bench-t" class="text-xs font-medium text-foreground">{t('bench.threads')}</Label>
        <Input
          id="bench-t"
          type="text"
          inputmode="numeric"
          bind:value={threads}
          aria-invalid={threadsP.error}
          placeholder={t('bench.optional')}
          disabled={running}
          class="h-8 font-num text-xs"
        />
        {#if threadsP.error}
          <span class="text-xs text-destructive">{t('bench.invalid_int')}</span>
        {/if}
      </div>

      <div class="flex flex-col gap-1">
        <Label for="bench-b" class="text-xs font-medium text-foreground">{t('bench.batch')}</Label>
        <Input
          id="bench-b"
          type="text"
          inputmode="numeric"
          bind:value={batch}
          aria-invalid={batchP.error}
          placeholder={t('bench.optional')}
          disabled={running}
          class="h-8 font-num text-xs"
        />
        {#if batchP.error}
          <span class="text-xs text-destructive">{t('bench.invalid_int')}</span>
        {/if}
      </div>

      <div class="flex flex-col gap-1">
        <Label for="bench-ub" class="text-xs font-medium text-foreground">{t('bench.ubatch')}</Label>
        <Input
          id="bench-ub"
          type="text"
          inputmode="numeric"
          bind:value={ubatch}
          aria-invalid={ubatchP.error}
          placeholder={t('bench.optional')}
          disabled={running}
          class="h-8 font-num text-xs"
        />
        {#if ubatchP.error}
          <span class="text-xs text-destructive">{t('bench.invalid_int')}</span>
        {/if}
      </div>

      <div class="flex flex-col gap-1">
        <Label for="bench-r" class="text-xs font-medium text-foreground">{t('bench.repetitions')}</Label>
        <Input
          id="bench-r"
          type="text"
          inputmode="numeric"
          bind:value={reps}
          aria-invalid={repsP.error}
          placeholder="3"
          disabled={running}
          class="h-8 font-num text-xs"
        />
        {#if repsP.error}
          <span class="text-xs text-destructive">{t('bench.invalid_reps')}</span>
        {/if}
      </div>
    </div>

    <!-- Active Server Warning -->
    {#if app.isRunning}
      <div class="flex items-center gap-2 p-3 rounded-lg bg-warning/10 border border-warning/30 text-xs text-warning" role="status">
        <AlertTriangle size={15} class="shrink-0" />
        <span>{t('bench.server_warning')}</span>
      </div>
    {/if}

    <!-- Start Error Block -->
    {#if startError}
      <div class="flex flex-col gap-2 p-3 rounded-lg bg-destructive/10 border border-destructive/20 text-xs text-destructive" role="alert">
        <pre class="font-mono text-xs whitespace-pre-wrap">{startError}</pre>
        {#if runtimeMissing && onGoToSettings}
          <Button variant="secondary" size="sm" class="w-fit" onclick={() => onGoToSettings?.()}>
            <Settings size={13} class="mr-1" />
            {t('bench.go_settings')}
          </Button>
        {/if}
      </div>
    {/if}
  </CardContent>

  <!-- Submit Action Footer (Always clearly visible at the bottom) -->
  <CardFooter class="flex items-center justify-between pt-3 pb-4 border-t border-border bg-card shrink-0 rounded-b-xl">
    <span class="text-xs font-num text-muted-foreground" title={t('bench.test_count_hint', { reps: repsP.value ?? 3 })}>
      {t('bench.test_count', { count })}
    </span>
    <Button
      variant="default"
      size="sm"
      disabled={!formValid || running || starting}
      onclick={handleStart}
    >
      <Play size={13} class="mr-1.5" />
      {starting ? t('bench.starting') : t('bench.start')}
    </Button>
  </CardFooter>
</Card>
