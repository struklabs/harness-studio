<script lang="ts">
  import type { DailyPoint } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { fmtCompact, fmtDay, fmtInt } from './anFormat';

  interface Props {
    days: DailyPoint[];
  }

  let { days }: Props = $props();

  const DAY = 86_400_000;
  /** Consecutive days from the first to the last recorded one (empty days as zero-height gaps). */
  let series = $derived.by(() => {
    if (!days.length) return [];
    const byDay = new Map(days.map((d) => [d.day_start_ms, d]));
    const out: (DailyPoint & { empty: boolean })[] = [];
    const first = days[0].day_start_ms;
    const last = days[days.length - 1].day_start_ms;
    for (let ms = first; ms <= last && out.length < 400; ms += DAY) {
      const d = byDay.get(ms);
      out.push(d ? { ...d, empty: false } : { day_start_ms: ms, requests: 0, errors: 0, prompt_tokens: 0, completion_tokens: 0, empty: true });
    }
    return out;
  });
  let max = $derived(Math.max(1, ...series.map((d) => d.requests)));
</script>

<div class="flex h-36 items-end gap-[3px]" role="img" aria-label={t('an.daily')}>
  {#each series as d (d.day_start_ms)}
    {@const h = (d.requests / max) * 100}
    {@const eh = d.requests ? (d.errors / d.requests) * h : 0}
    <div
      class="group relative flex h-full min-w-[3px] flex-1 flex-col justify-end"
      title={t('an.daily_tooltip', {
        day: fmtDay(d.day_start_ms),
        n: fmtInt(d.requests),
        e: fmtInt(d.errors),
        t: fmtCompact(d.prompt_tokens + d.completion_tokens),
      })}
    >
      {#if d.requests > 0}
        <div class="keep-colors hatch w-full rounded-t-sm bg-destructive/80" style="height: {eh}%"></div>
        <div class="keep-colors w-full bg-primary/80 group-hover:bg-primary {eh > 0 ? '' : 'rounded-t-sm'}" style="height: {h - eh}%"></div>
      {:else}
        <div class="h-px w-full bg-border"></div>
      {/if}
    </div>
  {/each}
</div>
{#if series.length}
  <div class="mt-1 flex justify-between font-num text-xs text-muted-foreground">
    <span>{fmtDay(series[0].day_start_ms)}</span>
    {#if series.length > 1}<span>{fmtDay(series[series.length - 1].day_start_ms)}</span>{/if}
  </div>
{/if}
