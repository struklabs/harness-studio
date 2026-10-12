<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { NA } from './perfFormat';
  import type { Series } from './perfSeries';

  interface Props {
    title: string;
    series: Series[];
    /** Fixed upper bound of the Y axis (e.g. 100 for percentages); auto when omitted. */
    max?: number;
    /** Formats the headline value and the axis maximum. */
    format: (v: number) => string;
    /** Extra line under the chart, e.g. token counts. */
    footnote?: string;
  }

  let { title, series, max, format, footnote }: Props = $props();

  const W = 300;
  const H = 64;

  let axisMax = $derived.by(() => {
    if (max != null) return max;
    let top = 0;
    for (const s of series) for (const v of s.values) if (v != null && v > top) top = v;
    return Math.max(top, 1);
  });

  /** Path with a gap wherever a value is missing. */
  function pathOf(values: (number | null)[]): string {
    if (values.length < 2) return '';
    const step = W / (values.length - 1);
    let d = '';
    let pen = false;
    values.forEach((v, i) => {
      if (v == null) {
        pen = false;
        return;
      }
      const y = H - 2 - (Math.min(v, axisMax) / axisMax) * (H - 4);
      d += `${pen ? 'L' : 'M'}${(i * step).toFixed(1)},${y.toFixed(1)} `;
      pen = true;
    });
    return d.trim();
  }

  function last(values: (number | null)[]): number | null {
    return values.length ? values[values.length - 1] : null;
  }

  let paths = $derived(series.map((s) => pathOf(s.values)));
  let hasData = $derived(paths.some((p) => p !== ''));
</script>

<div class="flex min-w-0 flex-col gap-2 border-t border-border pt-2.5">
  <div class="flex items-start justify-between gap-2">
    <span class="truncate text-sm font-medium text-foreground">{title}</span>
    <div class="flex items-baseline gap-2 shrink-0 font-num">
      {#each series as s (s.color)}
        {@const v = last(s.values)}
        <span class="text-sm font-semibold {s.color}">{v == null ? NA : format(v)}</span>
      {/each}
    </div>
  </div>

  <!-- No box around the chart; no axis until there is data to scale. -->
  <div class="relative">
    {#if hasData}
      <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" class="block h-16 w-full" role="img" aria-label={title}>
        {#each [0.5, 1] as f (f)}
          <line x1="0" x2={W} y1={H * f - (f === 1 ? 0.5 : 0)} y2={H * f - (f === 1 ? 0.5 : 0)} stroke="currentColor" class="text-border" stroke-width="1" vector-effect="non-scaling-stroke" />
        {/each}
        {#each series as s, i (s.color)}
          {#if paths[i]}
            <!-- The second series is dashed so the two lines differ by shape, not only colour. -->
            <path d={paths[i]} fill="none" stroke="currentColor" class={s.color} stroke-width="1.5" stroke-dasharray={i === 1 ? '4 3' : undefined} stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke" />
          {/if}
        {/each}
      </svg>
      <span class="pointer-events-none absolute top-0 right-0 font-num text-xs text-muted-foreground">{format(axisMax)}</span>
    {:else}
      <div class="flex h-16 items-center justify-center text-xs text-muted-foreground">{t('ui.chart_waiting')}</div>
    {/if}
  </div>

  <div class="flex items-center justify-between gap-2 text-xs text-muted-foreground min-h-4">
    <span class="flex items-center gap-2 min-w-0">
      {#each series as s, i (s.color)}
        {#if s.label}
          <span class="flex items-center gap-1"><span class="w-2.5 border-t-2 {i === 1 ? 'border-dashed' : ''} {s.color} border-current" aria-hidden="true"></span>{s.label}</span>
        {/if}
      {/each}
    </span>
    {#if footnote}<span class="font-num truncate">{footnote}</span>{/if}
  </div>
</div>
