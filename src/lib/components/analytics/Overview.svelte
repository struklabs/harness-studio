<script lang="ts">
  import { ChartColumn, TriangleAlert } from '@lucide/svelte';
  import { getClientName } from '$lib/utils/clientName';
  import type { AnalyticsSummary, ModelStats } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { Card, CardHeader, CardTitle, CardContent } from '$lib/components/ui/card';
  import * as Table from '$lib/components/ui/table';
  import * as Empty from '$lib/components/ui/empty';
  import { Button } from '$lib/components/ui/button';
  import SectionLoading from '$lib/components/common/SectionLoading.svelte';
  import DailyBars from './DailyBars.svelte';
  import { NA, fmtAgo, fmtBytes, fmtCompact, fmtInt, fmtMs, fmtRate, fmtTps, type Range } from './anFormat';
  import { formatTokens } from '$lib/utils/tokens';
  import { locale } from '$lib/i18n.svelte';

  interface Props {
    page?: string;
    summary: AnalyticsSummary | null;
    range: Range;
    onOpenRequests: () => void;
    /** The summary could not be read (the screen shows the error): no loading animation. */
    loadFailed?: boolean;
  }

  let { page = 'overview', summary, range, onOpenRequests, loadFailed = false }: Props = $props();

  let totals = $derived(summary?.totals ?? null);
  let hasData = $derived((totals?.requests ?? 0) > 0);
  let direct = $derived(summary?.direct ?? null);
  let hasDirect = $derived((direct?.prompt_tokens ?? 0) + (direct?.completion_tokens ?? 0) > 0);
  let withBench = $derived((summary?.models ?? []).filter((m) => m.benchmark && m.alias));
  let pair = (a: string, b: string) => (a === NA && b === NA ? NA : `${a} / ${b}`);

  const modelName = (m: ModelStats) => (m.alias ? m.alias : t('an.unresolved'));

  /** Width of a comparison bar relative to the larger of the two values. */
  function barPct(v: number | null | undefined, other: number | null | undefined): number {
    const max = Math.max(v ?? 0, other ?? 0);
    return max > 0 && v ? Math.max(2, (v / max) * 100) : 0;
  }
</script>

{#snippet directCard()}
  {#if direct && hasDirect}
    <Card class="gap-0 border-border p-0">
      <CardHeader class="px-4 pt-4 pb-2">
        <CardTitle class="text-sm font-semibold">{t('an.direct_title')}</CardTitle>
        <p class="text-xs text-muted-foreground">{t('an.direct_hint')}</p>
      </CardHeader>
      <CardContent class="flex flex-col gap-3 px-4 pb-4">
        <div class="flex items-baseline justify-between gap-2 text-xs">
          <span class="text-muted-foreground">{t('an.direct_tokens')}</span>
          <span class="font-num text-base font-semibold">{fmtCompact(direct.prompt_tokens)} / {fmtCompact(direct.completion_tokens)}</span>
        </div>
        <Table.Root>
          <Table.Header>
            <Table.Row class="bg-muted/30 hover:bg-muted/30">
              <Table.Head class="text-xs">{t('an.direct_server')}</Table.Head>
              <Table.Head class="text-right text-xs">{t('an.direct_tokens')}</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each direct.servers as sv (sv.server_id)}
              <Table.Row>
                <Table.Cell class="max-w-64 truncate py-2 text-xs" title={sv.server_id}>{sv.label || sv.server_id}</Table.Cell>
                <Table.Cell class="py-2 text-right font-num text-xs">{fmtCompact(sv.prompt_tokens)} / {fmtCompact(sv.completion_tokens)}</Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      </CardContent>
    </Card>
  {/if}
{/snippet}

<div class="flex flex-col gap-4 px-6 py-4">
  {#if !summary}
    {#if loadFailed}
      <p class="text-xs text-muted-foreground">{t('an.loading')}</p>
    {:else}
      <SectionLoading shape="cards" minHeight="24rem" label={t('sl.an_label')} phases={[t('sl.an_1'), t('sl.an_2'), t('sl.an_3')]} />
    {/if}
  {:else if !hasData && !hasDirect}
    <Empty.Root class="py-16">
      <Empty.Header>
        <Empty.Media variant="icon"><ChartColumn /></Empty.Media>
        <Empty.Title>{range === 'all' ? t('an.empty_title') : t('an.empty_range')}</Empty.Title>
        <Empty.Description>{t('an.empty_desc')}</Empty.Description>
      </Empty.Header>
    </Empty.Root>
  {:else if totals}
    <!-- One flat strip of measurements, vertical rules between them. The -ml-px/-mb-px grid hides
         the rule of whichever item starts a wrapped line, so it works at any width. -->
    <div hidden={page === 'breakdown'} class="shrink-0 overflow-hidden rounded-lg border border-border bg-card">
      <dl class="-mb-px -ml-px grid grid-cols-2 md:grid-cols-3 xl:grid-cols-6">
      {#each [
        { label: t('an.kpi_requests'), value: fmtInt(totals.requests), sub: totals.cancelled ? `${fmtInt(totals.cancelled)} ${t('an.cancelled')}` : '' },
        { label: t('an.kpi_tokens'), value: `${fmtCompact(totals.prompt_tokens)} / ${fmtCompact(totals.completion_tokens)}`, sub: '' },
        { label: t('an.kpi_total_tokens'), value: formatTokens(totals.prompt_tokens + totals.completion_tokens + (direct?.prompt_tokens ?? 0) + (direct?.completion_tokens ?? 0), locale()), sub: '' },
        { label: t('an.kpi_gen_tps'), value: fmtTps(totals.avg_gen_tps), sub: '' },
        { label: t('an.kpi_ttft'), value: pair(fmtMs(totals.ttft_p50_ms), fmtMs(totals.ttft_p95_ms)), sub: t('an.percentile_hint', { n: totals.percentile_samples }) },
        { label: t('an.kpi_latency'), value: pair(fmtMs(totals.latency_p50_ms), fmtMs(totals.latency_p95_ms)), sub: '' },
        { label: t('an.kpi_errors'), value: `${fmtRate(totals.error_rate)} / ${fmtRate(totals.oom_rate)}`, sub: `${t('an.kpi_busy')}: ${fmtMs(totals.busy_ms)}` },
      ] as k (k.label)}
        <div class="flex min-w-0 flex-col gap-0.5 border-b border-l border-border px-4 py-2.5">
          <dt class="truncate text-xs text-muted-foreground">{k.label}</dt>
          <dd class="truncate font-num text-base font-semibold">{k.value}</dd>
          {#if k.sub}<dd class="truncate text-xs text-muted-foreground">{k.sub}</dd>{/if}
        </div>
      {/each}
      </dl>
    </div>

    <div class="flex flex-col gap-4">
<div hidden={page === 'breakdown'}>      <Card class="gap-0 border-border p-0">
        <CardHeader class="px-4 pt-4 pb-2">
          <CardTitle class="text-sm font-semibold">{t('an.daily')}</CardTitle>
          <p class="text-xs text-muted-foreground">{t('an.daily_hint')}</p>
        </CardHeader>
        <CardContent class="px-4 pb-4">
          <DailyBars days={summary.daily} />
        </CardContent>
      </Card>
</div><div hidden={page !== 'breakdown'}>      <Card class="gap-0 border-border p-0">
        <CardHeader class="px-4 pt-4 pb-2">
          <CardTitle class="text-sm font-semibold">{t('an.token_breakdown')}</CardTitle>
          <p class="text-xs text-muted-foreground">{t('an.token_breakdown_hint')}</p>
        </CardHeader>
        <CardContent class="overflow-x-auto px-4 pb-4">
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
        </CardContent>
      </Card></div>
    </div>

<div hidden={page !== 'breakdown'}>    <Card class="gap-0 border-border p-0">
      <CardHeader class="flex flex-row items-center justify-between px-4 pt-4 pb-2">
        <CardTitle class="text-sm font-semibold">{t('an.models')}</CardTitle>
        <Button variant="ghost" size="xs" onclick={onOpenRequests}>{t('an.tab_requests')}</Button>
      </CardHeader>
      <CardContent class="px-0 pb-2">
        <div class="overflow-x-auto">
          <Table.Root>
            <Table.Header>
              <Table.Row class="bg-muted/30 hover:bg-muted/30">
                <Table.Head class="pl-4 text-xs">{t('an.col_model')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_runs')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_tokens')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_gen_tps')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_prompt_tps')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_ttft')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_latency')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_vram')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_errors')}</Table.Head>
                <Table.Head class="text-right text-xs">{t('an.col_busy')}</Table.Head>
                <Table.Head class="pr-4 text-right text-xs">{t('an.col_last')}</Table.Head>
              </Table.Row>
            </Table.Header>
            <Table.Body>
              {#each summary.models as m (m.alias)}
                <Table.Row>
                  <Table.Cell class="py-2 pl-4 text-xs">
                    <div class="flex flex-col">
                      <span class="max-w-56 truncate font-medium" title={m.alias}>{modelName(m)}</span>
                      {#if m.loads > 0 || m.spec_accept_rate != null}
                        <span class="text-xs text-muted-foreground">
                          {#if m.loads > 0}{t('an.loads', { n: m.loads, ms: fmtMs(m.avg_load_ms) })}{/if}
                          {#if m.spec_accept_rate != null}{m.loads > 0 ? ' · ' : ''}{t('an.spec_rate', { rate: fmtRate(m.spec_accept_rate) })}{/if}
                        </span>
                      {/if}
                    </div>
                  </Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{fmtInt(m.requests)}</Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{fmtCompact(m.prompt_tokens)} / {fmtCompact(m.completion_tokens)}</Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{fmtTps(m.avg_gen_tps)}</Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{fmtTps(m.avg_prompt_tps)}</Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs" title={t('an.percentile_hint', { n: m.percentile_samples })}>
                    {pair(fmtMs(m.ttft_p50_ms), fmtMs(m.ttft_p95_ms))}
                  </Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs" title={t('an.percentile_hint', { n: m.percentile_samples })}>
                    {pair(fmtMs(m.latency_p50_ms), fmtMs(m.latency_p95_ms))}
                  </Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs" title={t('an.vram_hint')}>
                    {pair(fmtBytes(m.vram_avg_bytes), fmtBytes(m.vram_peak_bytes))}
                  </Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs {m.errors > 0 ? 'text-destructive' : ''}">
                    {#if m.errors > 0}<TriangleAlert class="mr-1 inline size-3 align-[-2px]" aria-hidden="true" />{/if}{fmtRate(m.error_rate)} / {fmtRate(m.oom_rate)}
                  </Table.Cell>
                  <Table.Cell class="py-2 text-right font-num text-xs">{fmtMs(m.busy_ms)}</Table.Cell>
                  <Table.Cell class="py-2 pr-4 text-right text-xs text-muted-foreground">{fmtAgo(m.last_used_ms)}</Table.Cell>
                </Table.Row>
              {/each}
            </Table.Body>
          </Table.Root>
        </div>
        {#if summary.rollup_days > 0}
          <p class="px-4 pt-2 text-xs text-muted-foreground">
            {t('an.percentile_note', { days: summary.retention_days, n: summary.rollup_days })}
          </p>
        {/if}
      </CardContent>
    </Card></div>

    <Card class="gap-0 border-border p-0">
      <CardHeader class="px-4 pt-4 pb-2">
        <CardTitle class="text-sm font-semibold">{t('an.bench')}</CardTitle>
        <p class="text-xs text-muted-foreground">{t('an.bench_hint')}</p>
      </CardHeader>
      <CardContent class="flex flex-col gap-4 px-4 pb-4">
        {#if withBench.length === 0}
          <p class="text-xs text-muted-foreground">{t('an.bench_none')}</p>
        {/if}
        {#each withBench as m (m.alias)}
          {@const b = m.benchmark!}
          <div class="flex flex-col gap-2">
            <span class="text-xs font-medium">{m.alias}</span>
            <div class="grid grid-cols-1 gap-x-6 gap-y-1.5 md:grid-cols-2">
              {#each [
                { label: t('an.bench_real_gen'), v: m.avg_gen_tps, other: b.gen_tps, color: 'bg-chart-3' },
                { label: t('an.bench_bench_gen', { n: b.n_gen ?? '' }), v: b.gen_tps, other: m.avg_gen_tps, color: 'bg-chart-3/40' },
                { label: t('an.bench_real_prompt'), v: m.avg_prompt_tps, other: b.prompt_tps, color: 'bg-chart-2' },
                { label: t('an.bench_bench_prompt', { n: b.n_prompt ?? '' }), v: b.prompt_tps, other: m.avg_prompt_tps, color: 'bg-chart-2/40' },
              ] as row (row.label)}
                <div class="grid grid-cols-[8rem_minmax(0,1fr)_5.5rem] items-center gap-2 text-xs">
                  <span class="truncate text-muted-foreground">{row.label}</span>
                  <div class="h-2 overflow-hidden rounded-full bg-muted">
                    <div class="h-full rounded-full {row.color}" style="width: {barPct(row.v, row.other)}%"></div>
                  </div>
                  <span class="text-right font-num">{fmtTps(row.v)}</span>
                </div>
              {/each}
            </div>
          </div>
        {/each}
      </CardContent>
    </Card>

    {@render directCard()}
  {/if}
</div>
