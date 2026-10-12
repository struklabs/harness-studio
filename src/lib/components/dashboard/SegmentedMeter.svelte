<script lang="ts">
  // A memory bar (VRAM / RAM) split by who uses it: Harness Studio, each llama-server (its own
  // colour), other programs (gray) and free space (empty). The numbers are the backend's
  // measurements; without a split (`breakdown` null) the bar is a single "used" segment.
  import { formatBytes } from '$lib/format';
  import { app } from '$lib/stores/app.svelte';
  import { t } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import type { MemoryBreakdown } from '$lib/types';
  import { buildSegments, legendPct, type Segment } from '$lib/memorySegments';
  import { slotColor, slotHatched } from '$lib/serverColor';
  import * as Tooltip from '$lib/components/ui/tooltip';

  interface Props {
    label: string;
    tip: string;
    used: number;
    total: number;
    breakdown: MemoryBreakdown | null;
    /** Tooltip title and footnote when split; `unsplitNote` when it cannot be. */
    breakdownTitle: string;
    note: string;
    unsplitNote?: string;
  }

  let { label, tip, used, total, breakdown, breakdownTitle, note, unsplitNote }: Props = $props();

  let p = $derived(total > 0 ? Math.min(100, (used / total) * 100) : 0);
  let bar = $derived(breakdown ? buildSegments(used, total, breakdown) : null);

  /** "15.9 / 31.1 GB": one unit when both sides share it, so the pair fits a narrow column. */
  let pair = $derived.by(() => {
    const [uv, uu] = formatBytes(used, 1).split(' ');
    const [tv, tu] = formatBytes(total, 1).split(' ');
    return uu === tu ? { used: uv, rest: ` / ${tv} ${tu}` } : { used: `${uv} ${uu}`, rest: ` / ${tv} ${tu}` };
  });

  let tone = $derived(p >= 92 ? 'text-destructive' : p >= 80 ? 'text-warning' : 'text-foreground');
  let ring = $derived(p >= 92 ? 'ring-1 ring-destructive' : p >= 80 ? 'ring-1 ring-warning' : '');

  const HARNESS = 'color-mix(in oklab, var(--foreground) 85%, transparent)';
  const OTHER = 'color-mix(in oklab, var(--muted-foreground) 80%, transparent)';

  function segName(s: Segment): string {
    if (s.kind === 'harness') return t('dash.tp_ram_harness');
    if (s.kind === 'other') return t('dash.tp_ram_other');
    return app.serverLabel(s.serverId ?? '');
  }
  function segColor(s: Segment): string {
    if (s.kind === 'harness') return HARNESS;
    if (s.kind === 'other') return OTHER;
    return slotColor(app.serverSlots[s.serverId ?? ''] ?? 0);
  }
  function segHatch(s: Segment): boolean {
    return s.kind === 'server' && slotHatched(app.serverSlots[s.serverId ?? ''] ?? 0);
  }

  /** Spoken form of the bar: used / total, then each consumer. */
  let valueText = $derived(
    [
      `${formatBytes(used, 1)} / ${formatBytes(total, 1)}`,
      ...(bar ? bar.segments.filter((s) => s.bytes > 0).map((s) => `${segName(s)} ${formatBytes(s.bytes, 1)}`) : []),
    ].join(', '),
  );
</script>

<Tooltip.Root>
  <Tooltip.Trigger>
    {#snippet child({ props })}
      <div {...props} class="flex min-w-0 flex-col gap-1.5">
        <div class="flex flex-wrap items-baseline justify-between gap-x-2 gap-y-0.5 text-xs">
          <span class="whitespace-nowrap text-muted-foreground">{label}</span>
          <span class="font-num whitespace-nowrap text-muted-foreground">
            <span class={tone}>{pair.used}</span>{pair.rest}{#if p >= 80}<span class={tone}> · {Math.round(p)}%</span>{/if}
          </span>
        </div>
        <!-- Segments are plain widths (no transition): a bar moves only when a sample changes. -->
        <!-- Not focusable itself: the tooltip trigger around it is the single tab stop that opens the legend. -->
        <div
          class={cn('keep-colors flex h-1.5 overflow-hidden rounded-full bg-muted', ring)}
          role="meter"
          aria-valuenow={Math.round(p)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuetext={valueText}
          aria-label={label}
        >
          {#if bar}
            {#each bar.segments as s (s.key)}
              {#if s.pct > 0}
                <div
                  class={cn('h-full shrink-0 border-r border-card last:border-r-0', segHatch(s) && 'hatch')}
                  style="width: {s.pct}%; background-color: {segColor(s)}"
                ></div>
              {/if}
            {/each}
          {:else}
            <div class="h-full shrink-0 bg-foreground/70" style="width: {p}%"></div>
          {/if}
        </div>
      </div>
    {/snippet}
  </Tooltip.Trigger>
  <Tooltip.Content class="flex-col items-stretch gap-1 py-2">
    <span class="font-medium">{bar ? breakdownTitle : label}</span>
    {#if bar}
      <div class="grid grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-x-3 gap-y-0.5 font-num">
        {#each bar.segments as s (s.key)}
          <span
            class={cn('size-2 rounded-full', segHatch(s) && 'hatch')}
            style="background-color: {segColor(s)}"
            aria-hidden="true"
          ></span>
          <span class="truncate" title={s.kind === 'harness' ? t('dash.tp_ram_harness_tip') : undefined}>
            {segName(s)}{#if s.model}<span class="font-mono opacity-70"> · {s.model}</span>{/if}
          </span>
          <span class="text-right">{formatBytes(s.bytes, 1)}</span>
          <span class="text-right opacity-70">{legendPct(s.bytes, total)}</span>
        {/each}
        <span class="size-2 rounded-full border border-current opacity-60" aria-hidden="true"></span>
        <span>{t('dash.tp_mem_free')}</span>
        <span class="text-right">{formatBytes(bar.freeBytes, 1)}</span>
        <span class="text-right opacity-70">{legendPct(bar.freeBytes, total)}</span>
      </div>
      <span class="max-w-72 opacity-70">{note}</span>
    {:else}
      <span class="max-w-64 opacity-90">{tip}</span>
      {#if unsplitNote}<span class="max-w-64 opacity-70">{unsplitNote}</span>{/if}
    {/if}
  </Tooltip.Content>
</Tooltip.Root>
