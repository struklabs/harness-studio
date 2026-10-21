<script lang="ts">
  import { fixed } from '#lib/format.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import type { TrainingMetric } from '#lib/training-types.js';
  import { fmtLoss } from './trainingFormat';

  interface Props {
    metrics: TrainingMetric[];
    totalSteps: number;
  }
  let { metrics, totalSteps }: Props = $props();

  const W = 640;
  const H = 260;
  const PAD = { l: 44, r: 12, t: 12, b: 28 };
  const innerW = W - PAD.l - PAD.r;
  const innerH = H - PAD.t - PAD.b;

  type Pt = { step: number; v: number };
  let train = $derived<Pt[]>(metrics.filter((m) => m.loss !== null).map((m) => ({ step: m.step, v: m.loss as number })));
  let evals = $derived<Pt[]>(metrics.filter((m) => m.eval_loss !== null).map((m) => ({ step: m.step, v: m.eval_loss as number })));

  let maxStep = $derived(Math.max(1, totalSteps, ...metrics.map((m) => m.step)));
  let yRange = $derived.by(() => {
    const all = [...train, ...evals].map((p) => p.v);
    if (!all.length) return { lo: 0, hi: 1 };
    const lo = Math.min(...all);
    const hi = Math.max(...all);
    const span = hi - lo || Math.max(hi, 1) * 0.1;
    return { lo: Math.max(0, lo - span * 0.08), hi: hi + span * 0.08 };
  });

  const x = (step: number) => PAD.l + (step / maxStep) * innerW;
  const y = (v: number) => PAD.t + (1 - (v - yRange.lo) / (yRange.hi - yRange.lo)) * innerH;
  const path = (pts: Pt[]) => pts.map((p, i) => `${i === 0 ? 'M' : 'L'}${x(p.step).toFixed(1)},${y(p.v).toFixed(1)}`).join(' ');

  let yTicks = $derived(Array.from({ length: 5 }, (_, i) => yRange.lo + ((yRange.hi - yRange.lo) * i) / 4));
  let xTicks = $derived(Array.from({ length: 5 }, (_, i) => Math.round((maxStep * i) / 4)));

  let hover = $state<number | null>(null);
  let hovered = $derived(hover === null ? null : (metrics.find((m) => m.step === hover) ?? null));

  function move(e: PointerEvent) {
    const svg = e.currentTarget as SVGSVGElement;
    const rect = svg.getBoundingClientRect();
    const px = ((e.clientX - rect.left) / rect.width) * W;
    const step = ((px - PAD.l) / innerW) * maxStep;
    let best: TrainingMetric | null = null;
    for (const m of metrics) {
      if (m.loss === null && m.eval_loss === null) continue;
      if (!best || Math.abs(m.step - step) < Math.abs(best.step - step)) best = m;
    }
    hover = best ? best.step : null;
  }
</script>

<div class="relative">
  {#if !train.length && !evals.length}
    <div class="flex h-48 items-center justify-center rounded-md border border-dashed border-border text-sm text-muted-foreground">
      {t('training.chart_empty')}
    </div>
  {:else}
    <svg viewBox="0 0 {W} {H}" class="h-auto w-full" role="img" aria-label={t('training.chart_label')} onpointermove={move} onpointerleave={() => (hover = null)}>
      {#each yTicks as tick (tick)}
        <line x1={PAD.l} x2={W - PAD.r} y1={y(tick)} y2={y(tick)} stroke="var(--border)" stroke-width="1" />
        <text x={PAD.l - 6} y={y(tick) + 3} text-anchor="end" font-size="10" fill="var(--muted-foreground)">{fixed(tick, 2)}</text>
      {/each}
      {#each xTicks as tick (tick)}
        <text x={x(tick)} y={H - 10} text-anchor="middle" font-size="10" fill="var(--muted-foreground)">{tick}</text>
      {/each}
      <text x={W - PAD.r} y={H - 0} text-anchor="end" font-size="10" fill="var(--muted-foreground)">{t('training.chart_step')}</text>

      {#if train.length}
        <path d={path(train)} fill="none" stroke="var(--chart-1)" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" />
      {/if}
      {#if evals.length}
        <path d={path(evals)} fill="none" stroke="var(--chart-2)" stroke-width="2" stroke-dasharray="5 3" stroke-linejoin="round" stroke-linecap="round" />
        {#each evals as p (p.step)}<circle cx={x(p.step)} cy={y(p.v)} r="3" fill="var(--chart-2)" />{/each}
      {/if}

      {#if hovered}
        <line x1={x(hovered.step)} x2={x(hovered.step)} y1={PAD.t} y2={PAD.t + innerH} stroke="var(--muted-foreground)" stroke-dasharray="2 3" />
        {#if hovered.loss !== null}<circle cx={x(hovered.step)} cy={y(hovered.loss)} r="4" fill="var(--chart-1)" />{/if}
        {#if hovered.eval_loss !== null}<circle cx={x(hovered.step)} cy={y(hovered.eval_loss)} r="4" fill="var(--chart-2)" />{/if}
      {/if}
    </svg>

    <div class="mt-1 flex flex-wrap items-center gap-x-4 text-xs text-muted-foreground">
      <span class="flex items-center gap-1"><span class="inline-block h-0.5 w-4" style="background: var(--chart-1)"></span>{t('training.loss')}</span>
      <span class="flex items-center gap-1"><span class="inline-block h-0.5 w-4" style="background: var(--chart-2)"></span>{t('training.eval_loss')}</span>
      {#if hovered}
        <span class="ml-auto text-foreground" aria-live="polite">
          {t('training.chart_tooltip', { step: hovered.step, loss: fmtLoss(hovered.loss, locale()), eval: fmtLoss(hovered.eval_loss, locale()) })}
        </span>
      {/if}
    </div>
  {/if}
</div>
