<script lang="ts">
  import AdvancedOnly from '$lib/components/common/AdvancedOnly.svelte';
  import type { FitCategory, HardwareFitResult } from '$lib/types';
  import { formatBytes } from '$lib/format';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import { Button } from '$lib/components/ui/button';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { ChevronRight } from '@lucide/svelte';
  import KvNote from './KvNote.svelte';

  interface Props {
    fitResult: HardwareFitResult;
    recommendedNgl: number | null;
    gpuLayers: number;
    onApplyRecommendedNgl: () => void;
  }

  let { fitResult, recommendedNgl, gpuLayers, onApplyRecommendedNgl }: Props = $props();

  const shortKey: Record<FitCategory, TKey> = {
    full_gpu: 'dash.fit_short_gpu',
    tight: 'dash.fit_short_tight',
    hybrid: 'dash.fit_short_partial',
    cpu_heavy: 'dash.fit_short_cpu',
    oom_risk: 'dash.fit_short_oom',
  };
  const tone: Record<FitCategory, { text: string; dot: string }> = {
    full_gpu: { text: 'text-success', dot: 'bg-success' },
    tight: { text: 'text-warning', dot: 'bg-warning' },
    hybrid: { text: 'text-warning', dot: 'bg-warning' },
    cpu_heavy: { text: 'text-warning', dot: 'bg-warning' },
    oom_risk: { text: 'text-destructive', dot: 'bg-destructive' },
  };
  // Integrated GPU: CPU weights are green when they are the recommendation (older Intel iGPU or
  // a benchmark said so); an offload into shared memory is neutral.
  let colors = $derived(
    fitResult.sharedIgpu && fitResult.category === 'cpu_heavy' && fitResult.recommendedGpuLayers === 0
      ? tone.full_gpu
      : fitResult.sharedIgpu && (fitResult.category === 'full_gpu' || fitResult.category === 'tight')
        ? { text: 'text-foreground', dot: 'bg-muted-foreground' }
        : tone[fitResult.category],
  );
  let short = $derived(
    fitResult.sharedIgpu && fitResult.category === 'cpu_heavy'
      ? t('dash.fit_short_igpu_cpu')
      : fitResult.sharedIgpu && (fitResult.category === 'full_gpu' || fitResult.category === 'tight')
        ? t('dash.fit_short_igpu_full')
        : t(shortKey[fitResult.category]),
  );
  let memLabel = $derived(fitResult.sharedIgpu ? t('dash.fit_shared_mem') : 'VRAM');
  let sourceNote = $derived(
    fitResult.source === 'measured'
      ? t('fit.source_measured')
      : fitResult.source === 'corrected'
        ? t('fit.source_corrected')
        : fitResult.exact
          ? t('fit.estimate_note')
          : t('fit.estimate_note_guess'),
  );
  let showRecommend = $derived(recommendedNgl !== null && recommendedNgl !== gpuLayers);
  let hasDraft = $derived(fitResult.draftGpuBytes + fitResult.draftHostBytes > 0);
  let draftParts = $derived(
    [
      fitResult.draftGpuBytes > 0 ? `${memLabel} ${formatBytes(fitResult.draftGpuBytes)}` : null,
      fitResult.draftHostBytes > 0 ? `RAM ${formatBytes(fitResult.draftHostBytes)}` : null,
    ]
      .filter(Boolean)
      .join(' · '),
  );
</script>

<!-- Flat (no box inside the controls card): the verdict first; numbers and where they come from on demand. -->
<div class="flex flex-col gap-1.5" aria-label={t('dash.fit_aria')}>
  <div class="flex items-center justify-between gap-2">
    <Tooltip.Root>
      <Tooltip.Trigger class="flex min-w-0 items-center gap-1.5 rounded text-left focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
        <span class={cn('size-2 shrink-0 rounded-full', colors.dot)} aria-hidden="true"></span>
        <span class={cn('truncate text-sm font-medium', colors.text)}>{short}</span>
      </Tooltip.Trigger>
      <Tooltip.Content class="max-w-64 flex-col items-start">
        <span class="font-medium">{fitResult.label}</span>
        <span>{fitResult.description}</span>
        <AdvancedOnly><span class="text-muted-foreground">{sourceNote}</span></AdvancedOnly>
      </Tooltip.Content>
    </Tooltip.Root>
    <AdvancedOnly>
      <span class="shrink-0 font-num text-xs text-muted-foreground" title={sourceNote}>
        {memLabel} {formatBytes(fitResult.requiredVramBytes)}
      </span>
    </AdvancedOnly>
  </div>

  <AdvancedOnly>
    <details class="group/fit text-xs">
      <summary class="flex w-fit cursor-pointer list-none items-center gap-1 rounded-sm text-muted-foreground outline-none select-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 [&::-webkit-details-marker]:hidden">
        <ChevronRight class="size-3.5 transition-transform group-open/fit:rotate-90 motion-reduce:transition-none" />
        {t('ui.fit_details')}
      </summary>
      <dl class="mt-1 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 pl-4.5 font-mono tabular">
        <dt class="font-sans text-muted-foreground">{t('ui.fit_layers')}</dt><dd>{fitResult.offloadedLayers} / {fitResult.totalLayers}</dd>
        <dt class="font-sans text-muted-foreground">{memLabel}</dt><dd>{formatBytes(fitResult.requiredVramBytes)} · {fitResult.vramPercentage}%</dd>
        <dt class="font-sans text-muted-foreground">RAM</dt><dd>{formatBytes(fitResult.requiredRamBytes)} · {fitResult.ramPercentage}%</dd>
        <dt class="font-sans text-muted-foreground">{t('ui.fit_kv')}</dt><dd>{formatBytes(fitResult.kvCacheBytes)}</dd>
        {#if hasDraft}
          <!-- Part of the totals above: weights + its own cache at the target's context. -->
          <dt class="font-sans text-muted-foreground">{t('ui.fit_draft')}</dt>
          <dd>{draftParts}</dd>
        {/if}
        {#if fitResult.kv}
          <dt class="font-sans text-muted-foreground">{t('kv.detail')}</dt>
          <dd>{fitResult.kv.cache_type_k} / {fitResult.kv.cache_type_v} <span class="font-sans text-muted-foreground">({t('kv.detail_auto')})</span></dd>
        {/if}
        <dt class="font-sans text-muted-foreground">{t('ui.fit_source')}</dt><dd class="font-sans">{sourceNote}</dd>
      </dl>
    </details>

    {#if showRecommend}
      <div class="flex items-center justify-between gap-2">
        <span class="text-xs text-muted-foreground">{t('fit.recommend_ngl', { value: recommendedNgl ?? 0 })}</span>
        <Button variant="outline" size="xs" onclick={onApplyRecommendedNgl}>{t('fit.apply')}</Button>
      </div>
    {/if}
    {#if fitResult.draftGpuBytes > 0}
      <span class="text-xs text-muted-foreground">{t('fit.recommend_ngl_draft', { size: formatBytes(fitResult.draftGpuBytes) })}</span>
    {/if}
  </AdvancedOnly>
  {#if fitResult.kv}<KvNote kv={fitResult.kv} />{/if}
</div>
