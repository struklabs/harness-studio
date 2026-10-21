<script lang="ts">
  import { fixed } from '#lib/format.js';
  // Live numbers of one running server: speed, prompt speed, slots and KV cache, under a header
  // with its colour, name, model and port.
  import type { ServerSummary, TelemetrySnapshot } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { cn } from '#lib/utils.js';
  import { slotColor, slotHatched } from '#lib/serverColor.js';
  import Sparkline from '../common/Sparkline.svelte';

  interface Props {
    server: ServerSummary;
    telemetry: TelemetrySnapshot | null;
    history: number[];
    /** Smaller numbers, for several cards at once. */
    compact?: boolean;
  }

  let { server, telemetry, history, compact = false }: Props = $props();

  let slot = $derived(app.serverSlots[server.id] ?? 0);
  let starting = $derived(server.status === 'Starting');
  let running = $derived(server.status === 'Running');
  let live = $derived(running && telemetry?.server_online ? telemetry : null);
  let model = $derived(server.active?.model_path.split(/[\\/]/).pop() ?? '');
  let peak = $derived(history.length ? Math.max(...history) : 0);
  let genTps = $derived(live?.predicted_tokens_per_sec ?? 0);
  let promptTps = $derived(live?.prompt_tokens_per_sec ?? 0);
</script>

<article class="@container flex flex-col gap-2" aria-label={app.serverLabel(server.id)}>
  <header class="flex min-w-0 items-center gap-2 text-xs">
    <span
      class={cn('size-2.5 shrink-0 rounded-full', slotHatched(slot) && 'hatch')}
      style="background-color: {slotColor(slot)}"
      aria-hidden="true"
    ></span>
    <span class="truncate text-sm font-medium text-foreground">{app.serverLabel(server.id)}</span>
    {#if server.active}<span class="shrink-0 font-num text-muted-foreground">:{server.active.port}</span>{/if}
    <span class="min-w-0 truncate font-mono text-muted-foreground" title={server.active?.model_path}>{model}</span>
    <span class="ml-auto flex shrink-0 items-center gap-1.5">
      {#if live}
        <span class="flex items-center gap-1.5 text-success" title={t('dash.tp_live_tip')}>
          <span class="size-1.5 rounded-full bg-success" aria-hidden="true"></span>
          {t('ui.tp_live')}
        </span>
      {:else if starting}
        <span class="text-warning">{t('dash.tp_card_starting')}</span>
      {:else}
        <span class="text-muted-foreground">{t('dash.tp_card_waiting')}</span>
      {/if}
    </span>
  </header>

  {#if live}
    <!-- One row only when the card is wide enough: the Dashboard column is resizable. -->
    <div class="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-border bg-border @2xl:grid-cols-[minmax(0,1.4fr)_repeat(3,minmax(0,1fr))]">
      <div class="flex flex-col gap-1 bg-card p-3" title={t('dash.tp_gen_speed_tip')}>
        <span class="text-xs text-muted-foreground">{t('dash.tp_gen_speed_label')}</span>
        <div class="flex items-end justify-between gap-2">
          <div class="flex items-baseline gap-1">
            <span class={cn('font-num leading-none font-semibold', compact ? 'text-xl' : 'text-2xl')}>{fixed(genTps, 1)}</span>
            <span class="text-xs text-muted-foreground">t/s</span>
          </div>
          <Sparkline values={history} width={84} height={compact ? 22 : 26} color={slotColor(slot)} />
        </div>
        <span class="font-num text-xs text-muted-foreground">{t('dash.tp_peak', { value: fixed(peak, 1) })}</span>
      </div>

      <div class="flex flex-col gap-1 bg-card p-3" title={t('dash.tp_prompt_tip')}>
        <span class="text-xs text-muted-foreground">{t('dash.tp_prompt_label')}</span>
        <div class="flex items-baseline gap-1">
          <span class="font-num text-lg leading-none font-semibold">{fixed(promptTps, 1)}</span>
          <span class="text-xs text-muted-foreground">t/s</span>
        </div>
      </div>

      <div class="flex flex-col gap-1 bg-card p-3" title={t('dash.tp_slots_tip')}>
        <span class="text-xs text-muted-foreground">{t('dash.tp_slots_label')}</span>
        <div class="flex flex-wrap items-baseline gap-x-1.5">
          <span class="font-num text-lg leading-none font-semibold">{live.active_slots}/{live.total_slots || 1}</span>
          <span class="text-xs whitespace-nowrap text-muted-foreground">
            {live.active_slots ? t('dash.tp_processing') : t('dash.tp_idle')}
          </span>
        </div>
      </div>

      <div class="flex flex-col gap-1 bg-card p-3" title={t('dash.tp_kv_cache_tip')}>
        <span class="text-xs text-muted-foreground">{t('dash.tp_kv_cache_label')}</span>
        <div class="flex items-baseline gap-1">
          <span class="font-num text-lg leading-none font-semibold">
            {live.kv_cache_available ? fixed(live.kv_cache_usage_ratio * 100, 0) : '–'}
          </span>
          {#if live.kv_cache_available}<span class="text-xs text-muted-foreground">%</span>{/if}
        </div>
      </div>
    </div>

    <!-- Speculative decoding: only when the server has drafted (llamacpp:spec_decode_* counters). -->
    {#if live.spec_acceptance_rate != null}
      <div class="flex items-baseline justify-between gap-2 text-xs" title={t('spec.accept_tip')}>
        <span class="text-muted-foreground">{t('spec.accept_label')}</span>
        <span class="font-num text-foreground">{t('spec.accept_value', { pct: fixed(live.spec_acceptance_rate * 100, 0) })}</span>
      </div>
    {/if}
  {/if}
</article>
