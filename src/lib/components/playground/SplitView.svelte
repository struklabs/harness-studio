<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  // Playground split view (P2): two panes, each with its own model (gateway alias or a running
  // server) and preset. Both show the shared history; a split turn shows each side's answer.
  // One composer (in the parent) sends to both.
  import { presetName } from '#lib/components/presets/presetText.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import * as Collapsible from '#lib/components/ui/collapsible/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import NativeSelect from '../dashboard/parts/NativeSelect.svelte';
  import ChatMessage from './ChatMessage.svelte';
  import CompactionDivider from './CompactionDivider.svelte';
  import { activeCompaction } from './compaction.svelte';
  import type { ChatMessage as Msg } from './types';
  import type { ModelLoadState } from './gatewayModels';
  import { isRemoteModel } from './remoteModels';
  import { isCloudModel } from './cloudModels';
  import {
    SIDES,
    chatServers,
    gwTarget,
    isSplit,
    modelFileName,
    parseTarget,
    sideOf,
    srvTarget,
    summarize,
    type CompareSettings,
    type Side,
    type SideState,
  } from './split';
  import { paneShortcut } from './splitKeys';
  import { ChevronRight, Info, MessageSquareShare } from '@lucide/svelte';

  interface Props {
    messages: Msg[];
    compare: CompareSettings;
    /** Gateway model aliases (empty while the gateway is off). */
    gatewayModels: string[];
    modelStates: Record<string, ModelLoadState>;
    /** A split turn (or a compaction) is running. */
    busy: boolean;
    /** A server or the gateway runs (needed to regenerate and rewind). */
    canChat?: boolean;
    sideState: Record<Side, SideState>;
    /** Why the sides run one after the other (empty = they run together). */
    notice: string;
    /** Tools are offered to both sides (each side runs its own tool loop on the selected server). */
    toolsEnabled?: boolean;
    onAdopt: (side: Side) => void;
    /** Edit from here: rewind to before the user message at this index (the parent confirms). */
    onRewind?: (index: number) => void;
    /** Regenerate the last split turn (both sides). */
    onRegenerate?: () => void;
    /** Show another answer pair of a regenerated turn. */
    onVariant?: (index: number, dir: -1 | 1) => void;
    /** Switch between branches of an edited user message. */
    onSibling?: (index: number, dir: -1 | 1) => void;
    onFork?: (index: number) => void;
    onApproveTool?: (side: Side, callId: string, always: boolean) => void;
    onDenyTool?: (side: Side, callId: string) => void;
  }

  let {
    messages,
    compare = $bindable(),
    gatewayModels,
    modelStates,
    busy,
    canChat = false,
    sideState,
    notice,
    toolsEnabled = false,
    onAdopt,
    onRewind,
    onRegenerate,
    onVariant,
    onSibling,
    onFork,
    onApproveTool,
    onDenyTool,
  }: Props = $props();

  /** The compaction both sides' requests apply (made before the split view was turned on). */
  let activeCmp = $derived(activeCompaction(messages));

  const uid = $props.id();
  const LETTER: Record<Side, string> = { a: 'A', b: 'B' };

  // ---- Layout: side by side, stacked under ~900 px of available width ----
  let width = $state(0);
  let stacked = $derived(width > 0 && width < 900);

  // ---- Targets ----
  let servers = $derived(chatServers(app.servers));

  function targetLabel(key: string): string {
    const tg = parseTarget(key);
    if (!tg) return t('pgs.no_target');
    if (tg.kind === 'gateway') return tg.alias;
    const s = app.servers.find((x) => x.id === tg.serverId);
    const model = s?.active ? modelFileName(s.active.model_path) : '';
    return model ? `${app.serverLabel(tg.serverId)} · ${model}` : app.serverLabel(tg.serverId);
  }

  function targetAvailable(key: string): boolean {
    const tg = parseTarget(key);
    if (!tg) return false;
    if (tg.kind === 'gateway') return app.gatewayRunning && gatewayModels.includes(tg.alias);
    return servers.some((s) => s.id === tg.serverId);
  }

  function stateLabel(alias: string): string {
    if (isRemoteModel(alias)) return t('link.remote');
    if (isCloudModel(alias)) return t('cloud.location', { provider: alias.slice(6, alias.indexOf('/')) });
    const st = modelStates[alias];
    return st === 'loaded' ? t('pgs.state_loaded') : st === 'loading' ? t('pgs.state_loading') : t('pgs.state_jit');
  }

  // ---- Scrolling: each pane sticks to the bottom while streaming; optional sync ----
  let scrollEls = $state<Record<Side, HTMLDivElement | undefined>>({ a: undefined, b: undefined });
  const stick: Record<Side, boolean> = { a: true, b: true };
  // Position a sync just set on a pane: its scroll event arrives a frame later and must not
  // steer the other pane back (that cut keyboard smooth scrolls — Home/End/PageDown — short).
  const echo: Record<Side, number | null> = { a: null, b: null };
  let queued = false;

  function onScroll(side: Side) {
    const el = scrollEls[side];
    if (!el) return;
    stick[side] = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    const expected = echo[side];
    echo[side] = null;
    if (expected !== null && Math.abs(el.scrollTop - expected) < 2) return;
    if (!compare.syncScroll) return;
    const otherSide: Side = side === 'a' ? 'b' : 'a';
    const other = scrollEls[otherSide];
    if (!other) return;
    const range = el.scrollHeight - el.clientHeight;
    const ratio = range > 0 ? el.scrollTop / range : 0;
    const before = other.scrollTop;
    other.scrollTop = ratio * (other.scrollHeight - other.clientHeight);
    if (other.scrollTop !== before) echo[otherSide] = other.scrollTop;
  }

  /** Keeps the panes at the bottom while answers stream (unless the user scrolled up). */
  export function scrollToBottom(force = false) {
    if (force) stick.a = stick.b = true;
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      for (const side of SIDES) {
        const el = scrollEls[side];
        if (el && stick[side]) el.scrollTop = el.scrollHeight;
      }
    });
  }

  // Alt+1 / Alt+2 (⌥1 / ⌥2 on macOS) focus a pane (arrow keys then scroll it).
  function onKeydown(e: KeyboardEvent) {
    if (!width) return;
    const side: Side | null = paneShortcut(e);
    if (!side) return;
    e.preventDefault();
    scrollEls[side]?.focus();
  }

  // ---- Latest split turn: stats and comparison ----
  let lastTurn = $derived.by(() => {
    for (let i = messages.length - 1; i >= 0; i--) if (isSplit(messages[i])) return messages[i];
    return null;
  });
  let lastDone = $derived(!!lastTurn && !lastTurn.compareSides?.some((s) => s.streaming));
  let summary = $derived(lastTurn && lastDone ? summarize(sideOf(lastTurn, 'a'), sideOf(lastTurn, 'b')) : null);
  let diffOpen = $state(false);

  function num(v: number, digits = 1): string {
    return v.toLocaleString(locale(), { maximumFractionDigits: digits, minimumFractionDigits: digits });
  }
  function duration(ms: number): string {
    return ms < 1000 ? `${Math.round(ms)} ms` : `${num(ms / 1000, 2)} s`;
  }
  function pct(v: number): string {
    return `${Math.round(v * 100)}%`;
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#snippet paneHeader(side: Side)}
  {@const pane = compare[side]}
  {@const turnSide = lastTurn ? sideOf(lastTurn, side) : null}
  {@const stats = turnSide?.stats}
  <div class="flex shrink-0 flex-col gap-1.5 border-b border-border bg-card/40 px-3 py-2">
    <div class="flex min-w-0 items-center gap-1.5">
      <span
        class="flex size-5 shrink-0 items-center justify-center rounded bg-muted font-mono text-xs font-semibold text-muted-foreground"
        aria-hidden="true">{LETTER[side]}</span
      >
      <label for={`${uid}-target-${side}`} class="sr-only">{t('pgs.target_label', { side: LETTER[side] })}</label>
      <NativeSelect
        id={`${uid}-target-${side}`}
        class="min-w-0 flex-1 font-mono text-xs"
        value={pane.target}
        disabled={busy}
        onchange={(e: Event) => (compare[side].target = (e.currentTarget as HTMLSelectElement).value)}
      >
        {#if !targetAvailable(pane.target)}
          <option value={pane.target}>
            {pane.target ? t('pgs.target_unavailable', { name: targetLabel(pane.target) }) : t('pgs.no_target')}
          </option>
        {/if}
        {#if app.gatewayRunning && gatewayModels.length}
          <optgroup label={t('pgs.group_gateway')}>
            {#each gatewayModels as alias (alias)}
              <option value={gwTarget(alias)}>{alias} — {stateLabel(alias)}</option>
            {/each}
          </optgroup>
        {/if}
        {#if servers.length}
          <optgroup label={t('pgs.group_servers')}>
            {#each servers as s (s.id)}
              <option value={srvTarget(s.id)}>{targetLabel(srvTarget(s.id))}</option>
            {/each}
          </optgroup>
        {/if}
      </NativeSelect>
      <label for={`${uid}-preset-${side}`} class="sr-only">{t('pgs.preset_label', { side: LETTER[side] })}</label>
      <NativeSelect
        id={`${uid}-preset-${side}`}
        class="w-36 shrink-0 text-xs max-[1100px]:w-28"
        value={pane.presetId}
        disabled={busy}
        onchange={(e: Event) => (compare[side].presetId = (e.currentTarget as HTMLSelectElement).value)}
      >
        <option value="">{t('ip.no_preset')}</option>
        {#if pane.presetId && !domain.preset(pane.presetId)}
          <option value={pane.presetId}>{t('ip.missing_preset')}</option>
        {/if}
        <optgroup label={t('ip.builtins')}>
          {#each domain.presets.filter((p) => p.builtin) as p (p.id)}
            <option value={p.id}>{presetName(p)}</option>
          {/each}
        </optgroup>
        {#if domain.presets.some((p) => !p.builtin)}
          <optgroup label={t('ip.mine')}>
            {#each domain.presets.filter((p) => !p.builtin) as p (p.id)}
              <option value={p.id}>{presetName(p)}</option>
            {/each}
          </optgroup>
        {/if}
      </NativeSelect>
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              disabled={busy || !lastTurn}
              aria-label={t('pgs.adopt', { side: LETTER[side] })}
              onclick={() => onAdopt(side)}
            >
              <MessageSquareShare />
            </Button>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content class="max-w-72">
          <p class="font-medium">{t('pgs.adopt', { side: LETTER[side] })}</p>
          <p class="mt-0.5">{t('pgs.adopt_desc')}</p>
        </Tooltip.Content>
      </Tooltip.Root>
    </div>
    <div class="flex min-h-4 flex-wrap items-center gap-x-2.5 gap-y-0.5 font-num text-xs text-muted-foreground" aria-live="polite">
      {#if sideState[side] === 'queued'}
        <span class="text-warning">{t('pgs.queued')}</span>
      {:else if sideState[side] === 'running'}
        <span class="animate-pulse text-foreground/80">{t('pg.generating')}</span>
      {:else if stats}
        <AdvancedOnly>
        {#if stats.ttftMs !== undefined}<span title={t('pg.stat_ttft_title')}>{t('pg.stat_ttft', { value: duration(stats.ttftMs) })}</span>{/if}
        {#if stats.predictedPerSecond !== undefined}<span title={t('pg.stat_speed_title')}>{#if stats.source === 'usage'}≈{/if}{t('pg.stat_speed', { value: num(stats.predictedPerSecond) })}</span>{/if}
        {#if stats.predictedN !== undefined}<span title={t('pg.stat_tokens_title')}>{t('pg.stat_tokens', { n: stats.predictedN })}</span>{/if}
        </AdvancedOnly>
        {#if turnSide?.compareMeta?.sequential}<span class="text-warning" title={t('pgs.ran_after_title')}>{t('pgs.ran_after')}</span>{/if}
      {:else}
        <span>{t('pgs.no_answer_yet')}</span>
      {/if}
    </div>
  </div>
{/snippet}

<div class="flex min-h-0 flex-1 flex-col" bind:clientWidth={width}>
  <div class="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-b border-border px-3 py-1.5 text-xs">
    <div class="flex items-center gap-2">
      <Switch id={`${uid}-sync`} bind:checked={compare.syncScroll} size="sm" />
      <label for={`${uid}-sync`} class="text-muted-foreground">{t('pgs.sync_scroll')}</label>
    </div>
    <div class="flex items-center gap-2">
      <Switch id={`${uid}-seq`} bind:checked={compare.sequential} size="sm" disabled={busy} />
      <label for={`${uid}-seq`} class="text-muted-foreground" title={t('pgs.sequential_title')}>{t('pgs.sequential')}</label>
    </div>
    {#if toolsEnabled}
      <span class="text-xs text-muted-foreground">{t('pgs.tools_on')}</span>
    {/if}
    <span class="ml-auto hidden text-xs text-muted-foreground min-[700px]:inline">{t('pgs.keys_hint')}</span>
  </div>

  {#if notice}
    <div class="flex shrink-0 items-start gap-2 border-b border-warning/30 bg-warning/10 px-3 py-1.5" role="status">
      <Info class="mt-px size-3.5 shrink-0 text-warning" aria-hidden="true" />
      <span class="min-w-0 flex-1 text-xs text-foreground [overflow-wrap:anywhere]">{notice}</span>
    </div>
  {/if}

  <div class={cn('flex min-h-0 flex-1', stacked ? 'flex-col' : 'flex-row')}>
    {#each SIDES as side (side)}
      <section
        class={cn(
          'flex min-h-0 min-w-0 flex-1 flex-col',
          side === 'a' && (stacked ? 'border-b border-border' : 'border-r border-border')
        )}
        aria-label={t('pgs.pane_label', { side: LETTER[side], name: targetLabel(compare[side].target) })}
      >
        {@render paneHeader(side)}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          class="min-h-0 flex-1 overflow-y-auto outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
          bind:this={scrollEls[side]}
          onscroll={() => onScroll(side)}
          role="log"
          aria-live="polite"
          aria-label={t('pgs.messages_label', { side: LETTER[side] })}
          tabindex="0"
        >
          <div class="flex min-h-full w-full flex-col gap-5 px-3 py-4">
            {#if messages.length === 0}
              <p class="my-auto text-center text-xs text-muted-foreground">{t('pgs.empty')}</p>
            {/if}
            {#each messages as m, i (m.id)}
              {#if m.role === 'compaction'}
                <CompactionDivider message={m} active={activeCmp?.markerIndex === i} />
              {:else}
                {@const shown = sideOf(m, side)}
                {@const last = i === messages.length - 1}
                <div class={cn(m.role === 'assistant' && !isSplit(m) && 'opacity-80')}>
                  <ChatMessage
                    message={shown}
                    {busy}
                    {canChat}
                    isTurnEnd={m.role === 'assistant'}
                    isLast={last}
                    variantNav={m.compareNav ? { index: m.compareNav.index, count: m.compareNav.count } : null}
                    onRegenerate={last && isSplit(m) ? onRegenerate : undefined}
                    onVariant={onVariant ? (dir) => onVariant(i, dir) : undefined}
                    onSibling={onSibling ? (dir) => onSibling(i, dir) : undefined}
                    onRewind={m.role === 'user' && onRewind ? () => onRewind(i) : undefined}
                    onFork={onFork ? () => onFork(i) : undefined}
                    onApproveTool={(callId, always) => onApproveTool?.(side, callId, always)}
                    onDenyTool={(callId) => onDenyTool?.(side, callId)}
                  />
                </div>
              {/if}
            {/each}
          </div>
        </div>
      </section>
    {/each}
  </div>

  {#if summary}
    <div class="shrink-0 border-t border-border px-3 py-1.5 text-xs" aria-live="polite">
      <Collapsible.Root bind:open={diffOpen}>
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span class="font-medium text-foreground">{t('pgs.summary')}</span>
          <span class="text-muted-foreground" title={t('pgs.similarity_title')}>
            {t('pgs.similarity', { pct: pct(summary.similarity) })}
          </span>
          <AdvancedOnly><span class="text-muted-foreground">{t('pgs.words', { a: summary.wordsA, b: summary.wordsB })}</span>
          {#if summary.faster && summary.speedRatio !== null}
            <span class="text-muted-foreground">
              {t('pgs.faster', { side: LETTER[summary.faster], ratio: num(summary.speedRatio) })}
            </span>
          {/if}
          {#if summary.quicker && summary.ttftDeltaMs !== null}
            <span class="text-muted-foreground">
              {t('pgs.quicker', { side: LETTER[summary.quicker], value: duration(summary.ttftDeltaMs) })}
            </span>
          {/if}
          </AdvancedOnly>
          {#if summary.diff}
            <Collapsible.Trigger
              class="group ml-auto flex items-center gap-1 rounded-sm font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <ChevronRight class="size-3.5 transition-transform group-data-[state=open]:rotate-90" aria-hidden="true" />
              {t('pgs.show_diff')}
            </Collapsible.Trigger>
          {:else}
            <span class="ml-auto text-xs text-muted-foreground">{t('pgs.diff_too_long')}</span>
          {/if}
        </div>
        {#if summary.diff}
          <Collapsible.Content>
            <p class="mt-1.5 text-xs text-muted-foreground">
              {t('pgs.diff_legend')}
              <del class="rounded bg-diff-remove/15 px-1 text-diff-remove">A</del>
              <ins class="rounded bg-diff-add/15 px-1 text-diff-add no-underline">B</ins>
            </p>
            <p
              class="mt-1 max-h-48 overflow-y-auto rounded-md border border-border bg-card p-2 text-sm leading-relaxed [overflow-wrap:anywhere]"
            >
              {#each summary.diff as d, i (i)}
                {#if d.op === 'same'}
                  <span>{d.text} </span>
                {:else if d.op === 'del'}
                  <del class="rounded bg-diff-remove/15 text-diff-remove">{d.text}</del>{' '}
                {:else}
                  <ins class="rounded bg-diff-add/15 text-diff-add no-underline">{d.text}</ins>{' '}
                {/if}
              {/each}
            </p>
          </Collapsible.Content>
        {/if}
      </Collapsible.Root>
    </div>
  {/if}
</div>
