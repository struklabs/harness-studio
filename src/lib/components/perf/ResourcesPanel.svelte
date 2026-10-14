<script lang="ts">
  import type { PerfDetails, TelemetrySnapshot } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { formatBytes } from '#lib/format.js';
  import StatRow from './StatRow.svelte';
  import { fmtPercent } from './perfFormat';

  interface Props {
    snapshot: TelemetrySnapshot | null;
    details: PerfDetails | null;
    /** Without a running server its column is left out instead of showing empty rows. */
    serverRunning: boolean;
  }

  let { snapshot, details, serverRunning }: Props = $props();

  const bytes = (v: number | null | undefined) => (v ? formatBytes(v, 1) : null);
  const used = (u: number | undefined, total: number | undefined) =>
    u && total ? `${formatBytes(u, 1)} / ${formatBytes(total, 1)}` : null;
  const cpu = (v: number | null | undefined) => (v == null ? null : fmtPercent(v, 1));

  let vram = $derived(used(snapshot?.vram_used_bytes, snapshot?.vram_total_bytes));
  let ram = $derived(used(snapshot?.system_ram_used_bytes, snapshot?.system_ram_total_bytes));
  let gpu = $derived(details?.gpu ?? null);
  let webview = $derived(details && details.app_webview.process_count > 0 ? details.app_webview : null);
  let appTotal = $derived(details ? details.app.rss_bytes + (webview?.rss_bytes ?? 0) : null);
  let appCpu = $derived(
    details?.app.cpu_percent == null ? null : details.app.cpu_percent + (webview?.cpu_percent ?? 0),
  );
</script>

<div class={serverRunning ? 'grid grid-cols-1 gap-x-8 gap-y-5 lg:grid-cols-3' : 'grid grid-cols-1 gap-x-8 gap-y-5 lg:grid-cols-2'}>
  <div class="flex min-w-0 flex-col">
    <h3 class="border-b border-border pb-1.5 text-xs font-medium text-muted-foreground">{t('perf.res_system')}</h3>
    <div class="divide-y divide-border/60">
      <StatRow label={t('perf.res_vram')} value={vram} />
      <StatRow hideMissing label={t('perf.res_gpu_load')} value={gpu?.utilization_percent != null ? fmtPercent(gpu.utilization_percent) : null} hint={t('perf.res_gpu_hint')} />
      <StatRow hideMissing label={t('perf.res_gpu_temp')} value={gpu?.temperature_c != null ? `${Math.round(gpu.temperature_c)} °C` : null} hint={t('perf.res_gpu_hint')} />
      <StatRow label={t('perf.res_ram')} value={ram} />
      <StatRow label={t('perf.res_cpu')} value={cpu(details?.system_cpu_percent)} hint={details ? t('perf.res_cores', { n: details.cpu_cores }) : undefined} />
    </div>
  </div>

  {#if serverRunning}
  <div class="flex min-w-0 flex-col">
    <h3 class="border-b border-border pb-1.5 text-xs font-medium text-muted-foreground">{t('perf.res_server')}</h3>
    <div class="divide-y divide-border/60">
      <StatRow label={t('perf.res_rss')} value={bytes(details?.server?.rss_bytes)} hint={t('perf.res_rss_hint')} />
      <StatRow label={t('perf.res_cpu')} value={cpu(details?.server?.cpu_percent)} hint={t('perf.res_cpu_hint')} />
    </div>
  </div>
  {/if}

  <div class="flex min-w-0 flex-col">
    <h3 class="border-b border-border pb-1.5 text-xs font-medium text-muted-foreground">{t('perf.res_app')}</h3>
    <div class="divide-y divide-border/60">
      <StatRow label={t('perf.res_rss')} value={bytes(appTotal)} hint={t('perf.res_app_hint')} />
      <StatRow label={t('perf.res_app_core')} value={bytes(details?.app.rss_bytes)} />
      <StatRow hideMissing label={t('perf.res_app_webview')} value={bytes(webview?.rss_bytes)} />
      <StatRow label={t('perf.res_cpu')} value={cpu(appCpu)} hint={t('perf.res_cpu_hint')} />
    </div>
  </div>
</div>
