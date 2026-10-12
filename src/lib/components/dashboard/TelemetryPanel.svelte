<script lang="ts">
  import { onMount } from 'svelte';
  import type { ServerStatus, TelemetrySnapshot } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { telemetryFilter } from '#lib/stores/serverFilter.svelte.js';
  import ServerOffState from '../common/ServerOffState.svelte';
  import ServerChips from '../common/ServerChips.svelte';
  import ServerTelemetryCard from './ServerTelemetryCard.svelte';
  import SegmentedMeter from './SegmentedMeter.svelte';

  interface Props {
    /** Status of the controlled server (the empty state is about it). */
    serverStatus: ServerStatus;
    /** Latest sample of the controlled server: system RAM / VRAM come from it. */
    telemetry: TelemetrySnapshot | null;
    /** Start button inside the empty state (only when the controls with their own are hidden). */
    onStart?: () => void;
  }

  let { serverStatus, telemetry, onStart }: Props = $props();

  // The memory meters show real values even while no server runs, and the cards follow every
  // running server, not only the controlled one: ask the backend for both while mounted.
  onMount(() => {
    const offMemory = app.watchMemory();
    const offAll = app.watchAllServers();
    return () => {
      offMemory();
      offAll();
    };
  });

  let running = $derived(app.activeServers);
  let ids = $derived(running.map((s) => s.id));
  let shown = $derived(telemetryFilter.effective(ids));
  let cards = $derived(running.filter((s) => shown.includes(s.id)));
  let selectedActive = $derived(app.selectedServer?.active != null);
  /** The controlled server is down while others run: say so instead of an empty panel. */
  let othersOnly = $derived(running.length > 0 && !selectedActive);

  /** System-wide memory is the same in every snapshot; the freshest known one is used. */
  let memory = $derived(telemetry ?? Object.values(app.telemetryAll)[0] ?? null);
</script>

<section class="@container flex flex-col gap-3 border-b border-border p-4" aria-label={t('dash.tp_title')}>
  <div class="flex items-center justify-between gap-2">
    <h2 class="text-sm font-semibold text-foreground">{t('dash.tp_title')}</h2>
    {#if running.length > 1}
      <span class="text-xs text-muted-foreground" aria-live="polite">{t('dash.tp_running_count', { n: running.length })}</span>
    {/if}
  </div>

  {#if running.length > 1}
    <ServerChips filter={telemetryFilter} {ids} label={t('dash.tp_servers_label')} />
  {/if}

  {#if running.length === 0}
    <ServerOffState
      status={serverStatus}
      description={onStart ? t('ui.off_dash_desc') : t('ui.off_dash_desc_left')}
      failedDescription={onStart ? undefined : t('ui.off_failed_dash_desc')}
      hideError={!onStart}
      {onStart}
      class="py-6"
    />
  {:else}
    {#if othersOnly}
      <p class="text-xs text-muted-foreground">
        {t('dash.tp_selected_stopped', { name: app.serverLabel(app.selectedServerId) })}
      </p>
    {/if}
    <div class="flex flex-col gap-4">
      {#each cards as server (server.id)}
        <ServerTelemetryCard
          {server}
          telemetry={app.telemetryAll[server.id] ?? null}
          history={app.tpsByServer[server.id] ?? []}
          compact={cards.length > 1}
        />
      {/each}
    </div>
  {/if}

  <!-- Memory: real system values even while no server runs (what is free before starting).
       Container query: two meters side by side only when the column has room; the value wraps
       under its label rather than being cut. -->
  {#if memory}
    <div class="grid grid-cols-1 gap-x-4 gap-y-3 @lg:grid-cols-2">
      {#if (memory.gpus?.length ?? 0) > 1}
        <!-- Several graphics cards: one bar per card, each split by who uses that card. -->
        {#each memory.gpus as gpu (gpu.index)}
          {#if gpu.total_bytes > 0}
            <SegmentedMeter
              label={t('dash.tp_vram_gpu_label', { name: gpu.name })}
              tip={t('dash.tp_vram_tip')}
              used={gpu.used_bytes}
              total={gpu.total_bytes}
              breakdown={gpu.breakdown}
              breakdownTitle={t('dash.tp_vram_gpu_breakdown_title', { name: gpu.name })}
              note={t('dash.tp_vram_note')}
              unsplitNote={t('dash.tp_vram_unsplit')}
            />
          {/if}
        {/each}
      {:else if memory.vram_total_bytes > 0}
        <SegmentedMeter
          label={t('dash.tp_vram_label')}
          tip={t('dash.tp_vram_tip')}
          used={memory.vram_used_bytes}
          total={memory.vram_total_bytes}
          breakdown={memory.vram_breakdown ?? null}
          breakdownTitle={t('dash.tp_vram_breakdown_title')}
          note={t('dash.tp_vram_note')}
          unsplitNote={t('dash.tp_vram_unsplit')}
        />
      {/if}
      {#if memory.system_ram_total_bytes > 0}
        <SegmentedMeter
          label={t('dash.tp_ram_label')}
          tip={t('dash.tp_ram_tip')}
          used={memory.system_ram_used_bytes}
          total={memory.system_ram_total_bytes}
          breakdown={memory.ram_breakdown ?? null}
          breakdownTitle={t('dash.tp_ram_breakdown_title')}
          note={t('dash.tp_ram_note')}
        />
      {/if}
    </div>
  {/if}
</section>
