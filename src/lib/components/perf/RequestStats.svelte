<script lang="ts">
  import type { TelemetrySnapshot } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import StatRow from './StatRow.svelte';
  import { NA, fmtCount, fmtDuration, fmtSeconds, rate } from './perfFormat';

  interface Props {
    snapshot: TelemetrySnapshot | null;
    /** Seconds the selected server has been running; `null` when unknown. */
    uptimeSecs: number | null;
    /** Wall-clock seconds, ticking, so "x ago" stays current. */
    nowSecs: number;
  }

  let { snapshot, uptimeSecs, nowSecs }: Props = $props();

  let last = $derived(snapshot?.last_request ?? null);
  let totals = $derived(snapshot?.totals ?? null);

  const tps = (tokens: number, secs: number) => {
    const r = rate(tokens, secs);
    return r == null ? NA : `${r.toFixed(1)} t/s`;
  };
</script>

<div class="grid grid-cols-1 gap-x-8 gap-y-5 lg:grid-cols-2">
  <div class="flex min-w-0 flex-col">
    <div class="flex items-baseline justify-between gap-2 border-b border-border pb-1.5">
      <h3 class="text-xs font-medium text-muted-foreground">{t('perf.last_request')}</h3>
      {#if last}
        <span class="text-xs text-muted-foreground font-num">{t('perf.ago', { time: fmtDuration(nowSecs - last.finished_at) })}</span>
      {/if}
    </div>
    <div class="divide-y divide-border/60">
      {#if last}
        <StatRow label={t('perf.lr_prompt')} value={fmtCount(last.prompt_tokens)} />
        <StatRow label={t('perf.lr_cached')} value={fmtCount(last.cached_tokens)} />
        <StatRow label={t('perf.lr_generated')} value={fmtCount(last.generated_tokens)} />
        <StatRow label={t('perf.lr_ttft')} value={fmtSeconds(last.prompt_seconds)} hint={t('perf.lr_ttft_hint')} />
        <StatRow label={t('perf.lr_duration')} value={fmtSeconds(last.prompt_seconds + last.generation_seconds)} />
        <StatRow label={t('perf.lr_prompt_speed')} value={tps(last.prompt_tokens, last.prompt_seconds)} />
        <StatRow label={t('perf.lr_gen_speed')} value={tps(last.generated_tokens, last.generation_seconds)} />
      {:else}
        <p class="text-xs text-muted-foreground py-3">{t('perf.last_request_none')}</p>
      {/if}
    </div>
  </div>

  <div class="flex min-w-0 flex-col">
    <div class="border-b border-border pb-1.5">
      <h3 class="text-xs font-medium text-muted-foreground">{t('perf.totals')}</h3>
    </div>
    <div class="divide-y divide-border/60">
      <StatRow label={t('perf.tot_uptime')} value={fmtDuration(uptimeSecs)} />
      <StatRow label={t('perf.tot_requests')} value={totals ? fmtCount(totals.requests_observed) : null} hint={t('perf.tot_requests_hint')} />
      <StatRow label={t('perf.tot_prompt')} value={totals ? fmtCount(totals.prompt_tokens) : null} />
      <StatRow label={t('perf.tot_cached')} value={totals?.cached_prompt_tokens != null ? fmtCount(totals.cached_prompt_tokens) : null} />
      <StatRow label={t('perf.tot_generated')} value={totals ? fmtCount(totals.generated_tokens) : null} />
      <StatRow label={t('perf.tot_max_seq')} value={totals?.max_sequence_tokens != null ? fmtCount(totals.max_sequence_tokens) : null} />
      <StatRow
        label={t('perf.tot_avg_speed')}
        value={totals ? `${tps(totals.generated_tokens, totals.generation_seconds)} · ${tps(totals.prompt_tokens, totals.prompt_seconds)}` : null}
        hint={t('perf.tot_avg_speed_hint')}
      />
    </div>
  </div>
</div>
