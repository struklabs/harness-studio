<script lang="ts">
  import type { Chart } from './generativeUi';
  import DataTable from './DataTable.svelte';
  import { t, locale } from '#lib/i18n.svelte.js';
  let { chart }: { chart: Chart } = $props();
  const colors = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-5)', 'var(--muted-foreground)'];
  let low = $derived(Math.min(0, ...chart.series.flatMap(s => s.values)));
  let high = $derived(Math.max(0, ...chart.series.flatMap(s => s.values)));
  let range = $derived(high - low || 1);
  const y = (v: number) => 180 - (v - low) / range * 160;
  const x = (i: number) => 50 + (i + 0.5) * 480 / chart.labels.length;
  const fmt = (n: number) => n.toLocaleString(locale(), { maximumSignificantDigits: 4 });
  function slice(i: number) {
    const values = chart.series[0].values;
    const total = values.reduce((a, b) => a + b, 0);
    const start = values.slice(0, i).reduce((a, b) => a + b, 0) / total * 2 * Math.PI - Math.PI / 2;
    const angle = values[i] / total * 2 * Math.PI;
    // Two arcs also handle a single full-circle slice.
    const p = (a: number) => `${290 + 78 * Math.cos(a)},${100 + 78 * Math.sin(a)}`;
    return `M290,100 L${p(start)} A78,78 0 0,1 ${p(start + angle / 2)} A78,78 0 0,1 ${p(start + angle)} Z`;
  }
</script>

<div class="min-w-0 space-y-3 rounded-lg border border-border bg-card p-3">
  <div class="flex flex-wrap gap-x-4 gap-y-2 text-xs text-muted-foreground">
    {#each chart.type === 'pie' ? chart.labels : chart.series.map(s => s.name) as name, i}<span class="flex min-w-0 items-center gap-1.5 [overflow-wrap:anywhere]"><span class="size-2 shrink-0 rounded-full" style:background={colors[i % colors.length]}></span>{name}</span>{/each}
  </div>
  <svg viewBox="0 0 560 210" role="img" aria-label={t('pg.rich_chart')} class="block w-full text-muted-foreground">
    <title>{t('pg.rich_chart')}</title>
    <desc>{t('pg.rich_chart_description')}</desc>
    {#if chart.type === 'pie'}
      {#each chart.labels as label, i}<path d={slice(i)} fill={colors[i % colors.length]} stroke="var(--card)" stroke-width="2"><title>{label}: {fmt(chart.series[0].values[i])}</title></path>{/each}
    {:else}
      {#each [low, low + range / 2, low + range] as tick}<line x1="50" x2="530" y1={y(tick)} y2={y(tick)} stroke="var(--border)" /><text x="44" y={y(tick) + 3} text-anchor="end" fill="currentColor" font-size="9">{fmt(tick)}</text>{/each}
      <line x1="50" x2="530" y1={y(0)} y2={y(0)} stroke="var(--muted-foreground)" />
      {#each chart.series as series, si}
        {#if chart.type === 'line'}<polyline points={series.values.map((v, i) => `${x(i)},${y(v)}`).join(' ')} fill="none" stroke={colors[si]} stroke-width="2" />{/if}
        {#each series.values as value, i}
          {#if chart.type === 'bar'}
            {@const width = 480 / chart.labels.length / (chart.series.length + 1)}
            <rect x={x(i) - width * chart.series.length / 2 + si * width} y={Math.min(y(0), y(value))} {width} height={Math.abs(y(value) - y(0))} rx="2" fill={colors[si]}><title>{chart.labels[i]} · {series.name}: {fmt(value)}</title></rect>
          {:else}<circle cx={x(i)} cy={y(value)} r="3" fill={colors[si]}><title>{chart.labels[i]} · {series.name}: {fmt(value)}</title></circle>{/if}
        {/each}
      {/each}
      {#each chart.labels as label, i}<text x={x(i)} y="199" text-anchor="middle" fill="currentColor" font-size="9">{i + 1}<title>{label}</title></text>{/each}
    {/if}
  </svg>
  <DataTable headers={[t('pg.rich_label'), ...chart.series.map(s => s.name)]} rows={chart.labels.map((label, i) => [chart.type === 'pie' ? label : `${i + 1}. ${label}`, ...chart.series.map(s => String(s.values[i]))])} />
</div>
