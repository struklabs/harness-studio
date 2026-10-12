<script lang="ts">
  import type { TelemetryHistoryPoint, TelemetrySnapshot } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import SparkChart from './SparkChart.svelte';
  import { fmtCount, fmtPercent } from './perfFormat';
  import { pct, pick, recent } from './perfSeries';

  interface Props {
    page?: string;
    history: TelemetryHistoryPoint[];
    snapshot: TelemetrySnapshot | null;
  }

  let { page = 'speed', history, snapshot }: Props = $props();

  let pts = $derived(recent(history));
  const tps = (v: number) => `${v.toFixed(v < 10 ? 1 : 0)} t/s`;
  let totalSlots = $derived(Math.max(1, ...pts.map((p) => p.total_slots), snapshot?.total_slots ?? 0));
  let ctxNote = $derived(
    snapshot?.context_size != null && snapshot.context_used_tokens != null
      ? t('perf.context_tokens', { used: fmtCount(snapshot.context_used_tokens), size: fmtCount(snapshot.context_size) })
      : undefined,
  );
</script>

<div class="grid grid-cols-1 gap-x-8 gap-y-5 md:grid-cols-2 xl:grid-cols-3">
<div hidden={page === 'resources'} class="contents">  <SparkChart
    title={t('perf.chart_gen')}
    format={tps}
    series={[{ values: pick(pts, (p) => p.predicted_tokens_per_sec), color: 'text-chart-3' }]}
  />
  <SparkChart
    title={t('perf.chart_prompt')}
    format={tps}
    series={[{ values: pick(pts, (p) => p.prompt_tokens_per_sec), color: 'text-chart-1' }]}
  />
  <SparkChart
    title={t('perf.chart_context')}
    max={100}
    format={(v) => fmtPercent(v, v < 10 ? 1 : 0)}
    footnote={ctxNote}
    series={[{ values: pick(pts, (p) => (p.kv_usage_ratio == null ? null : p.kv_usage_ratio * 100)), color: 'text-chart-5' }]}
  />
  <SparkChart
    title={t('perf.chart_slots')}
    max={totalSlots}
    format={(v) => String(Math.round(v))}
    footnote={t('perf.slots_of', { busy: snapshot?.active_slots ?? 0, total: snapshot?.total_slots ?? 0 })}
    series={[{ values: pick(pts, (p) => p.active_slots), color: 'text-chart-2' }]}
  />
  <SparkChart
    title={t('perf.chart_requests')}
    format={(v) => String(Math.round(v))}
    series={[
      { values: pick(pts, (p) => p.requests_processing), color: 'text-chart-3', label: t('perf.legend_processing') },
      { values: pick(pts, (p) => p.requests_deferred), color: 'text-chart-4', label: t('perf.legend_deferred') },
    ]}
  />
</div>
<div hidden={page !== 'resources'} class="contents">  <SparkChart
    title={t('perf.chart_memory')}
    max={100}
    format={(v) => fmtPercent(v)}
    series={[
      { values: pick(pts, (p) => pct(p.vram_used_bytes, p.vram_total_bytes)), color: 'text-chart-4', label: t('perf.legend_gpu_vram') },
      { values: pick(pts, (p) => pct(p.ram_used_bytes, p.ram_total_bytes)), color: 'text-chart-2', label: t('perf.legend_system_ram') },
    ]}
  />
</div>
</div>
