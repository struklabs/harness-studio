<script lang="ts">
  import { t, locale } from '$lib/i18n.svelte';
  import { formatBytes } from '$lib/format';
  import { bestByTest, flashAttnLabel, maxByTest, testKind, testLabel, type BenchResult } from '$lib/benchmark-types';
  import * as Table from '$lib/components/ui/table';
  import { Badge } from '$lib/components/ui/badge';
  import Trophy from '@lucide/svelte/icons/trophy';
  import { cn } from '$lib/utils';

  interface Props {
    results: BenchResult[];
  }

  let { results }: Props = $props();

  const best = $derived(bestByTest(results));
  const maxes = $derived(maxByTest(results));
  const info = $derived(results[0] ?? null);

  function fmt(v: number): string {
    return v.toLocaleString(locale(), { maximumFractionDigits: v >= 100 ? 1 : 2, minimumFractionDigits: v >= 100 ? 1 : 2 });
  }

  function barWidth(r: BenchResult): number {
    const max = maxes.get(testLabel(r)) ?? 0;
    return max > 0 ? Math.max(2, (r.avg_ts / max) * 100) : 0;
  }

  const kindColors: Record<string, { text: string; bar: string }> = {
    pp: { text: 'text-chart-2', bar: 'bg-chart-2' },
    tg: { text: 'text-chart-3', bar: 'bg-chart-3' },
    pg: { text: 'text-chart-4', bar: 'bg-chart-4' },
  };
</script>

{#if info}
  <div class="flex flex-wrap items-center gap-x-5 gap-y-2 mb-3 px-3 py-2 rounded-lg bg-muted/40 border border-border text-xs">
    <div class="flex items-center gap-1.5">
      <span class="text-muted-foreground">{t('bench.info_model')}:</span>
      <span class="font-medium text-foreground">{info.model_type} · {formatBytes(info.model_size, 2)}</span>
    </div>
    <div class="flex items-center gap-1.5">
      <span class="text-muted-foreground">{t('bench.info_backends')}:</span>
      <span class="font-medium text-foreground">{info.backends || '—'}</span>
    </div>
    {#if info.gpu_info}
      <div class="flex items-center gap-1.5">
        <span class="text-muted-foreground">{t('bench.info_gpu')}:</span>
        <span class="font-medium text-foreground">{info.gpu_info}</span>
      </div>
    {/if}
    <div class="flex items-center gap-1.5 font-num text-xs">
      <span class="text-muted-foreground">{t('bench.info_build')}:</span>
      <span class="text-foreground">{info.build_number ?? '?'} ({info.build_commit || '?'})</span>
    </div>
  </div>
{/if}

<div class="overflow-x-auto rounded-lg border border-border">
  <Table.Root>
    <Table.Header>
      <Table.Row class="border-b border-border bg-muted/30 hover:bg-muted/30">
        <Table.Head class="min-w-[120px]">{t('bench.col_test')}</Table.Head>
        <Table.Head class="w-16 text-right font-num text-xs">{t('bench.col_ngl')}</Table.Head>
        <Table.Head class="w-20 text-xs">{t('bench.col_fa')}</Table.Head>
        <Table.Head class="w-24 text-xs font-mono">{t('bench.col_cache')}</Table.Head>
        <Table.Head class="w-20 text-right font-num text-xs">{t('bench.col_threads')}</Table.Head>
        <Table.Head class="min-w-[240px] text-right font-num text-xs">{t('bench.col_speed')}</Table.Head>
      </Table.Row>
    </Table.Header>
    <Table.Body>
      {#each results as r, i (i)}
        {@const isBest = best.has(i)}
        {@const k = testKind(r)}
        {@const col = kindColors[k] ?? { text: 'text-primary', bar: 'bg-primary' }}
        <Table.Row class={cn(isBest ? 'bg-success/5 hover:bg-success/10' : '')}>
          <Table.Cell class="py-2">
            <div class="flex items-center gap-2">
              <span class={cn('font-num font-semibold text-xs', col.text)}>{testLabel(r)}</span>
              {#if isBest}
                <Badge variant="outline" class="text-xs h-4 px-1.5 py-0 text-success border-success/30 bg-success/10 font-bold">
                  <Trophy aria-hidden="true" />{t('bench.best')}
                </Badge>
              {/if}
            </div>
          </Table.Cell>
          <Table.Cell class="py-2 text-right font-num text-xs text-muted-foreground">
            {r.n_gpu_layers}
          </Table.Cell>
          <Table.Cell class="py-2 text-xs text-muted-foreground">
            {flashAttnLabel(r.flash_attn)}
          </Table.Cell>
          <Table.Cell class="py-2 font-mono text-xs text-muted-foreground">
            {r.type_k}/{r.type_v}
          </Table.Cell>
          <Table.Cell class="py-2 text-right font-num text-xs text-muted-foreground">
            {r.n_threads}
          </Table.Cell>
          <Table.Cell class="py-2">
            <div class="flex items-center justify-end gap-3">
              <div class="w-28 h-2 bg-muted rounded-full overflow-hidden shrink-0 hidden sm:block" aria-hidden="true">
                <div class={cn('h-full rounded-full transition-all duration-300', col.bar)} style="width: {barWidth(r)}%"></div>
              </div>
              <div class="font-num text-xs font-semibold text-foreground whitespace-nowrap min-w-[100px] text-right">
                <span>{fmt(r.avg_ts)}</span>
                <span class="text-xs text-muted-foreground font-normal ml-1">± {fmt(r.stddev_ts)}</span>
              </div>
            </div>
          </Table.Cell>
        </Table.Row>
      {/each}
    </Table.Body>
  </Table.Root>
</div>
