<script lang="ts">
  import type { HardwareFitResult, KvChoice } from '#lib/types.js';
  import { formatBytes } from '#lib/format.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  // Shown before a start whose GPU part exceeds the dedicated VRAM: on Windows the driver then
  // moves the excess into shared system memory (no OOM, but slow, and RAM can fill up).
  interface Props {
    fit: HardwareFitResult;
    recommendedNgl: number | null;
    gpuLayers: number;
    contextSize: number;
    /** The context the model was trained on (GGUF header), if known. */
    trainedContext: number | null;
    /** What automatic KV precision would pick when that fits (the form has a fixed one). */
    autoKv: KvChoice | null;
    onAutoKvAndStart: () => void;
    onApplyAndStart: () => void;
    onLowerContextAndStart: () => void;
    onStartAnyway: () => void;
    onCancel: () => void;
  }

  let {
    fit,
    recommendedNgl,
    gpuLayers,
    contextSize,
    trainedContext,
    autoKv,
    onAutoKvAndStart,
    onApplyAndStart,
    onLowerContextAndStart,
    onStartAnyway,
    onCancel,
  }: Props = $props();
  // A context beyond the trained length is what overflows: fewer GPU layers would only move
  // the same oversized KV cache into system RAM.
  let overTrained = $derived(trainedContext !== null && trainedContext > 0 && contextSize > trainedContext);
  // The fixed KV precision is what overflows and automatic precision fits: that is the fix,
  // not fewer GPU layers.
  let kvFix = $derived(!overTrained && autoKv !== null);
  // The dialog only opens for an `oom_risk` launch, so the category says nothing here: offer
  // the recommendation whenever it moves layers off the GPU.
  let canApply = $derived(!overTrained && !kvFix && recommendedNgl !== null && recommendedNgl < gpuLayers);
</script>

<Dialog.Root open onOpenChange={(next) => !next && onCancel()}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('spill.title')}</Dialog.Title>
      <Dialog.Description>
        {t('spill.body', { need: formatBytes(fit.requiredVramBytes), have: formatBytes(fit.vramBudgetBytes) })}
      </Dialog.Description>
    </Dialog.Header>
    <p class="text-sm text-muted-foreground">{t('spill.risk')}</p>
    {#if overTrained}
      <p class="text-sm">{t('spill.ctx_over', { ctx: contextSize.toLocaleString(), train: (trainedContext ?? 0).toLocaleString() })}</p>
    {:else if kvFix && autoKv}
      <p class="text-sm">{t('spill.kv_auto', { k: autoKv.cache_type_k, v: autoKv.cache_type_v })}</p>
    {:else if canApply}
      <p class="text-xs text-muted-foreground">{t('fit.recommend_ngl', { value: recommendedNgl ?? 0 })}</p>
    {/if}
    <Dialog.Footer class="gap-2">
      <Button variant="outline" onclick={onCancel}>{t('spill.cancel')}</Button>
      <Button variant="secondary" onclick={onStartAnyway}>{t('spill.anyway')}</Button>
      {#if overTrained}
        <Button onclick={onLowerContextAndStart}>{t('spill.lower_ctx_start', { train: (trainedContext ?? 0).toLocaleString() })}</Button>
      {:else if kvFix}
        <Button onclick={onAutoKvAndStart}>{t('spill.kv_auto_start')}</Button>
      {:else if canApply}
        <Button onclick={onApplyAndStart}>{t('spill.apply_start')}</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
