<script lang="ts">
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { compareRuns, pctDiff, type BenchmarkRun, type TestKind } from '#lib/benchmark-types.js';
  import * as Table from '#lib/components/ui/table/index.js';
  import Trophy from '@lucide/svelte/icons/trophy';
  import { cn } from '#lib/utils.js';

  interface Props {
    runs: BenchmarkRun[];
  }

  let { runs }: Props = $props();

  const rows = $derived(compareRuns(runs));
  const groups = $derived(
    (['pp', 'tg', 'pg'] as TestKind[])
      .map((kind) => ({ kind, rows: rows.filter((r) => r.kind === kind) }))
      .filter((g) => g.rows.length > 0),
  );

  const groupTitle: Record<TestKind, TKey> = { pp: 'bench.group_pp', tg: 'bench.group_tg', pg: 'bench.group_pg' };

  function fmt(v: number): string {
    return v.toLocaleString(locale(), { maximumFractionDigits: 1, minimumFractionDigits: 1 });
  }

  function fmtPct(p: number): string {
    const s = p.toLocaleString(locale(), { maximumFractionDigits: 1, minimumFractionDigits: 1 });
    return p > 0 ? `+${s}%` : `${s}%`;
  }

  function runLabel(run: BenchmarkRun): string {
    return new Date(run.started_at * 1000).toLocaleString(locale(), { dateStyle: 'short', timeStyle: 'short' });
  }
</script>

<div class="overflow-x-auto rounded-lg border border-border">
  <Table.Root>
    <Table.Header>
      <Table.Row class="border-b border-border bg-muted/40 hover:bg-muted/40">
        <Table.Head class="min-w-[120px]">{t('bench.col_test')}</Table.Head>
        <Table.Head class="min-w-[160px]">{t('bench.compare_config')}</Table.Head>
        {#each runs as run, i (run.id)}
          <Table.Head class="text-right min-w-[140px] max-w-[200px]">
            <div class="flex flex-col items-end">
              <span class="font-semibold text-xs text-foreground truncate max-w-full" title={run.model_name}>
                {run.model_name}
              </span>
              <span class="text-xs text-muted-foreground font-normal">
                {runLabel(run)}{i === 0 ? ` · ${t('bench.compare_base')}` : ''}
              </span>
            </div>
          </Table.Head>
        {/each}
      </Table.Row>
    </Table.Header>
    {#each groups as group (group.kind)}
      <Table.Body>
        <Table.Row class="bg-muted/60 hover:bg-muted/60 border-y border-border">
          <Table.Cell colspan={2 + runs.length} class="py-1.5 px-3 text-sm font-semibold text-foreground">
            {t(groupTitle[group.kind])}
          </Table.Cell>
        </Table.Row>
        {#each group.rows as row (row.key)}
          {@const bestVal = Math.max(...row.values.map((v) => v ?? -Infinity))}
          <Table.Row>
            <Table.Cell class="py-2 font-num text-xs font-semibold text-foreground">
              {row.test}
            </Table.Cell>
            <Table.Cell class="py-2 text-xs text-muted-foreground">
              {row.config}
            </Table.Cell>
            {#each row.values as v, i (i)}
              {@const isBest = v !== null && v === bestVal && runs.length > 1}
              <Table.Cell class={cn('py-2 text-right font-num text-xs', isBest ? 'text-success font-bold' : 'text-foreground')}>
                {#if v === null}
                  <span class="text-muted-foreground">—</span>
                {:else}
                  {#if isBest}<Trophy class="mr-1 inline size-3 align-[-2px]" aria-hidden="true" /><span class="sr-only">{t('bench.best')}: </span>{/if}
                  <span>{fmt(v)}</span>
                  {#if i > 0}
                    {@const d = pctDiff(v, row.values[0])}
                    {#if d !== null}
                      <span
                        class={cn(
                          'ml-1.5 text-xs font-medium',
                          d > 0.5 ? 'text-success' : d < -0.5 ? 'text-destructive' : 'text-muted-foreground'
                        )}
                      >
                        {fmtPct(d)}
                      </span>
                    {/if}
                  {/if}
                {/if}
              </Table.Cell>
            {/each}
          </Table.Row>
        {/each}
      </Table.Body>
    {/each}
  </Table.Root>
</div>
