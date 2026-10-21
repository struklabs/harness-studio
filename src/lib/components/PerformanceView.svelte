<script lang="ts">
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Empty from '#lib/components/ui/empty/index.js';
  import { formatBytes, fixed } from '#lib/format.js';
  import { onMount, untrack } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import type { TelemetrySnapshot, TelemetryHistoryPoint, PerfDetails, ServerStatus, AppSettings, AnalyticsSummary } from '#lib/types.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { otherRunningServers } from '#lib/runningServers.js';
  import { windowActive, onWindowActive } from '#lib/windowActivity.js';
  import { localizeError } from '#lib/network.js';
  import { toast } from 'svelte-sonner';
  import FieldHelp from './common/FieldHelp.svelte';
  import Sparkline from './common/Sparkline.svelte';
  import ServerOffState from './common/ServerOffState.svelte';
  import LiveCharts from './perf/LiveCharts.svelte';
  import RequestStats from './perf/RequestStats.svelte';
  import ResourcesPanel from './perf/ResourcesPanel.svelte';
  import SlotStrip from './perf/SlotStrip.svelte';
  import SectionLoading from './common/SectionLoading.svelte';
  import { NA } from './perf/perfFormat';
  import * as Table from '#lib/components/ui/table/index.js';
  import { getClientName } from '#lib/utils/clientName.js';
  import { formatTokens } from '#lib/utils/tokens.js';
  import { RANGES, rangeStart, type Range, fmtInt, fmtRate } from './analytics/anFormat';

  interface Props {
    visible?: boolean;
    page?: string;
    settings: AppSettings;
    serverStatus: ServerStatus;
  }

  let { visible = true, page = 'overview', settings, serverStatus }: Props = $props();

  let snapshot = $state<TelemetrySnapshot | null>(null);
  let history = $state<TelemetryHistoryPoint[]>([]);
  let details = $state<PerfDetails | null>(null);
  let summary = $state<AnalyticsSummary | null>(null);
  let range = $state<Range>('7d');
  let summaryFailed = $state(false);
  let summaryLoading = false;
  async function fetchSummary() {
    if (!visible || !windowActive() || summaryLoading) return;
    summaryLoading = true;
    const selectedRange = range;
    try {
      const result = await invoke<AnalyticsSummary>('get_analytics_summary', { fromMs: rangeStart(selectedRange), toMs: null });
      if (range === selectedRange) { summary = result; summaryFailed = false; }
    } catch { summaryFailed = true; }
    finally { summaryLoading = false; }
  }
  $effect(() => { void range; void page; untrack(() => { void fetchSummary(); }); });
  /** The first reading failed: show the (empty) screen instead of loading forever. */
  let readFailed = $state(false);
  const loadingPhases = $derived([t('sl.perf_1'), t('sl.perf_2')]);
  let nowSecs = $state(Math.floor(Date.now() / 1000));

  let isRunning = $derived(serverStatus === 'Running');
  let online = $derived(isRunning && !!snapshot?.server_online);

  // The view is only mounted while the Performance tab is open, so this is the only place
  // that samples CPU / process memory; hidden windows are skipped too.
  async function fetchTelemetryData() {
    if (!visible || !windowActive()) return; // window minimized / hidden: skip redraws
    const serverId = app.selectedServerId;
    try {
      const [snap, hist, perf] = await Promise.all([
        invoke<TelemetrySnapshot>('get_telemetry_snapshot', { serverId }),
        invoke<TelemetryHistoryPoint[]>('get_telemetry_history', { serverId, limit: 300 }),
        invoke<PerfDetails>('get_perf_details', { serverId })
      ]);
      if (serverId !== app.selectedServerId) return; // switched servers while waiting
      snapshot = snap;
      history = hist;
      details = perf;
      nowSecs = Math.floor(Date.now() / 1000);
      readFailed = false;
    } catch {
      // Background poll
      readFailed = true;
    }
  }

  onMount(() => {
    fetchTelemetryData();
    const pollTimer = setInterval(fetchTelemetryData, 1000);
    const summaryTimer = setInterval(fetchSummary, 15000);
    const offActive = onWindowActive(() => { void fetchTelemetryData(); void fetchSummary(); });

    return () => {
      clearInterval(pollTimer);
      clearInterval(summaryTimer);
      offActive();
    };
  });

  // A different server has its own history: clear the charts, samples, and refetch right away.
  let lastServerId: string | null = null;
  $effect(() => {
    const id = app.selectedServerId;
    untrack(() => {
      if (lastServerId !== null && lastServerId !== id) {
        snapshot = null;
        history = [];
        details = null;
        fetchTelemetryData();
      }
      lastServerId = id;
    });
  });

  // Average / peak over the backend's history of this run (samples with generation only).
  let genSamples = $derived(history.map((p) => p.predicted_tokens_per_sec).filter((v) => v > 0));
  let avgTps = $derived(genSamples.length ? genSamples.reduce((a, v) => a + v, 0) / genSamples.length : null);
  let peakTps = $derived(genSamples.length ? Math.max(...genSamples) : null);
  let peakVram = $derived(Math.max(0, ...history.map((p) => p.vram_used_bytes), snapshot?.vram_used_bytes ?? 0));

  /** The selected server is stopped but others (e.g. the gateway's) run: offer them. */
  let otherRunning = $derived(otherRunningServers(app.servers, app.selectedServerId));

  let starting = $state(false);
  async function startSelected() {
    const profile = app.selectedProfile;
    if (!profile) return;
    starting = true;
    try {
      await engineInstall.runWithEngine(() => invoke('start_server_profile', { profileId: profile.id }));
    } catch (e) {
      toast.error(engineInstall.autoFailureText(e) ?? localizeError(e));
    } finally {
      starting = false;
      await app.refreshServer();
    }
  }

  const fmtTps = (v: number | null | undefined) => (v == null ? NA : fixed(v, 1));
</script>

<div class="performance-content @container flex h-full w-full min-w-0 flex-col gap-6 overflow-y-auto p-4 sm:p-6">
  {#if (page === 'clients' && summaryFailed) || (page !== 'clients' && readFailed)}
    <div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-destructive/30 bg-destructive/5 p-4" role="alert">
      <p class="text-sm">{t('perf.read_failed')}</p>
      <Button variant="outline" onclick={() => page === 'clients' ? fetchSummary() : fetchTelemetryData()}>{t('perf.retry')}</Button>
    </div>
  {/if}
  {#if page === 'clients'}
    <section hidden={page !== 'clients'} class="flex flex-col gap-3" aria-labelledby="perf-clients-title">
      <h2 id="perf-clients-title" class="text-sm font-semibold text-foreground">{t('an.clients')}</h2>
      <div class="flex flex-wrap items-center gap-2">
        <label for="perf-token-range" class="text-xs">{t('an.range')}</label>
        <select id="perf-token-range" bind:value={range} class="rounded border border-border bg-card px-2 py-1 text-xs">
          {#each RANGES as r}<option value={r}>{t(`an.range_${r}`)}</option>{/each}
        </select>
        <FieldHelp text={t('an.token_breakdown_hint')} />
      </div>
      {#if summary && summary.clients.length === 0}
        <Empty.Root class="border border-border bg-card py-10"><Empty.Header><Empty.Title>{t('perf.clients_empty')}</Empty.Title><Empty.Description>{t('perf.clients_empty_hint')}</Empty.Description></Empty.Header></Empty.Root>
      {:else if summary}
        <div class="text-xs text-muted-foreground">{t('an.kpi_total_tokens')}: <strong class="font-num text-foreground">{formatTokens(summary.clients.reduce((n,c) => n + c.total_tokens, 0), locale())}</strong></div>
        <div class="overflow-x-auto rounded-lg border border-border bg-card">
          <Table.Root>
            <Table.Header>
              <Table.Row class="bg-muted/30 hover:bg-muted/30">
                <Table.Head class="text-xs">{t('an.client')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.requests')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.input_tokens')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.output_tokens')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.total_tokens')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.share')}</Table.Head>
              </Table.Row>
            </Table.Header>
            <Table.Body>
              {#each summary.clients as c (c.client)}
                <Table.Row>
                  <Table.Cell class="py-2 text-xs font-medium">
                    {getClientName(c.client)}
                  </Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{fmtInt(c.requests)}</Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{formatTokens(c.prompt_tokens, locale())}</Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{formatTokens(c.completion_tokens, locale())}</Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs font-semibold">{formatTokens(c.total_tokens, locale())}</Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{fmtRate(c.token_share, 1)}</Table.Cell>
                </Table.Row>
              {/each}
            </Table.Body>
          </Table.Root>
        </div>
      {:else if !summaryFailed && page === 'clients'}
        <SectionLoading shape="panel" minHeight="9rem" label={t('sl.perf_label')} phases={loadingPhases} />
      {/if}
    </section>
  {:else if !isRunning}
    <ServerOffState
      status={serverStatus}
      description={t('ui.off_perf_desc')}
      onStart={app.selectedProfile ? startSelected : undefined}
      busy={starting}
      secondary={{ label: app.selectedProfile ? t('srv.open_dashboard') : t('ui.off_open_dashboard'), onclick: () => (app.requestedTab = 'dashboard') }}
      class="rounded-lg border border-dashed border-border py-12"
    />

    {#if otherRunning.length > 0}
      <div class="flex flex-col items-center gap-2 text-sm" role="group" aria-label={t('perf.other_running')}>
        <span class="text-muted-foreground">{t('perf.other_running')}</span>
        <div class="flex flex-wrap justify-center gap-2">
          {#each otherRunning as s (s.id)}
            <button type="button" class="rounded-md border border-border bg-card px-3 py-1.5 text-xs font-medium hover:bg-accent" title={s.file ?? undefined} onclick={() => app.selectServer(s.id)}>
              {s.label}{s.port ? ` :${s.port}` : ''}
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Machine and app usage are real even without a server; shown once they have been read. -->
    {#if details}
      <section hidden={page !== 'resources'} class="flex flex-col gap-3" aria-labelledby="perf-res-title">
        <h2 id="perf-res-title" class="text-sm font-semibold text-foreground">{t('perf.section_resources')}</h2>
        <LiveCharts page="resources" {history} {snapshot} />
        <ResourcesPanel {snapshot} {details} serverRunning={false} />
      </section>
    {:else if !readFailed}
      <SectionLoading shape="panel" minHeight="9rem" label={t('sl.perf_label')} phases={loadingPhases} />
    {/if}
  {:else if snapshot === null && !readFailed}
    <SectionLoading shape="cards" class="flex-1" minHeight="22rem" label={t('sl.perf_label')} phases={loadingPhases} />
  {:else}
    <!-- Headline numbers in one strip: generation speed carries the weight. `shrink-0`: as an
         overflow-hidden item of the scrolling column it would otherwise shrink to its border
         in a short window (seen at 900x600). -->
    <div hidden={page !== 'overview'}>
    <div class="metric-grid shrink-0 gap-px overflow-hidden rounded-lg border border-border bg-border">
      <div class="flex flex-col gap-1.5 bg-card p-4">
        <span class="text-xs text-muted-foreground">{t('perf.chart_gen')}</span>
        <div class="flex flex-wrap items-end justify-between gap-3">
          <div class="flex min-w-0 flex-wrap items-baseline gap-1.5">
            <span class="font-num text-3xl font-semibold text-foreground">{online ? fmtTps(snapshot?.predicted_tokens_per_sec) : NA}</span>
            <span class="text-xs text-muted-foreground">t/s</span>
          </div>
          <Sparkline values={app.tpsHistory} width={96} height={28} class="text-primary" />
        </div>
        <span class="font-num text-xs text-muted-foreground">{t('ui.perf_avg_peak', { avg: fmtTps(avgTps), peak: fmtTps(peakTps) })}</span>
      </div>

      <div class="flex flex-col gap-1.5 bg-card p-4">
        <span class="text-xs text-muted-foreground">{t('perf.chart_prompt')}</span>
        <div class="flex min-w-0 flex-wrap items-baseline gap-1.5">
          <span class="font-num text-2xl font-semibold text-foreground">{online ? fmtTps(snapshot?.prompt_tokens_per_sec) : NA}</span>
          <span class="text-xs text-muted-foreground">t/s</span>
        </div>
      </div>

      <div class="flex flex-col gap-1.5 bg-card p-4">
        <span class="text-xs text-muted-foreground">{t('perf.res_vram')}</span>
        <span class="font-num text-2xl font-semibold text-foreground">{snapshot?.vram_used_bytes ? formatBytes(snapshot.vram_used_bytes) : NA}</span>
        {#if snapshot?.vram_total_bytes}
          <span class="truncate font-num text-xs text-muted-foreground">
            {t('ui.perf_vram_of', { total: formatBytes(snapshot.vram_total_bytes), peak: formatBytes(peakVram) })}
          </span>
        {/if}
      </div>

      <div class="flex flex-col gap-1.5 bg-card p-4">
        <span class="text-xs text-muted-foreground">{t('perf.chart_slots')}</span>
        <span class="font-num text-2xl font-semibold text-foreground">{snapshot?.total_slots ? `${snapshot.active_slots} / ${snapshot.total_slots}` : NA}</span>
        {#if snapshot?.kv_cache_available}
          <span class="font-num text-xs text-muted-foreground">{t('ui.perf_kv', { pct: fixed(snapshot.kv_cache_usage_ratio * 100, 0) })}</span>
        {/if}
      </div>
    </div>
    {#if summary}<div class="mt-3 text-xs text-muted-foreground">{t('an.kpi_total_tokens')}: <strong class="font-num">{formatTokens(summary.clients.reduce((n,c) => n + c.total_tokens, 0), locale())}</strong></div>{/if}
    </div>
    <section hidden={page !== 'speed'} class="flex flex-col gap-3" aria-labelledby="perf-charts-title">
      <h2 id="perf-charts-title" class="text-sm font-semibold text-foreground">{t('perf.section_charts')}</h2>
      <LiveCharts {history} {snapshot} />
      <SlotStrip slots={snapshot?.slots ?? []} />
    </section>

    <section hidden={page !== 'speed'} class="flex flex-col gap-3" aria-labelledby="perf-req-title">
      <h2 id="perf-req-title" class="text-sm font-semibold text-foreground">{t('perf.section_requests')}</h2>
      <RequestStats {snapshot} uptimeSecs={details?.server?.uptime_secs ?? null} {nowSecs} />
    </section>

    <section hidden={page !== 'resources'} class="flex flex-col gap-3" aria-labelledby="perf-res-title">
      <h2 id="perf-res-title" class="text-sm font-semibold text-foreground">{t('perf.section_resources')}</h2>
      <LiveCharts page="resources" {history} {snapshot} />
      <ResourcesPanel {snapshot} {details} serverRunning />
    </section>


  {/if}
</div>
