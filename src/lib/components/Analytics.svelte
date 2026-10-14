<script lang="ts">
  import AdvancedPageNotice from './common/AdvancedPageNotice.svelte';
  import { pageLevel } from '#lib/uiLevel.js';
  import SectionSidebar from './common/SectionSidebar.svelte';
  import { panes } from '#lib/stores/panes.svelte.js';
  import { SectionSelection } from '#lib/stores/sectionPage.svelte.js';
  import { ANALYTICS_GROUPS } from './common/screenSections';
  import RequestDetailSheet from './analytics/RequestDetailSheet.svelte';
  import FieldHelp from './common/FieldHelp.svelte';
  const section = new SectionSelection('analytics', ANALYTICS_GROUPS.flatMap(g => g.pages.map(p => p.id)), localStorage.getItem('harness_analytics_view') === 'requests' ? 'requests' : 'overview', undefined, ANALYTICS_GROUPS);
  let selectedRequest = $state<string | null>(null);
  function selectPage(id: string) { section.select(id); if (id === 'policy') openSettings(true); }
  function openRequest(id: string) { selectedRequest = id; section.select('detail'); }
  import { localizeError } from '#lib/network.js';
  // Analytics: statistics of gateway traffic and the request inspector. Everything
  // shown comes from analytics.db; nothing is estimated. Direct llama-server traffic is not
  // observable and the screen says so.
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { listen } from '@tauri-apps/api/event';
  import { toast } from 'svelte-sonner';
  import { ChartColumn, ListTree, RefreshCw, SlidersHorizontal, Info, TriangleAlert, Trash2 } from '@lucide/svelte';
  import type { AnalyticsInfo, AnalyticsSummary } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { windowActive } from '#lib/windowActivity.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import NativeSelect from '#lib/components/dashboard/parts/NativeSelect.svelte';
  import Overview from './analytics/Overview.svelte';
  import RequestLog from './analytics/RequestLog.svelte';
  import ToolCallLog from './analytics/ToolCallLog.svelte';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import { RANGES, rangeStart, fmtBytes, fmtInt, type Range } from './analytics/anFormat';

  const VIEW_KEY = 'harness_analytics_view';
  const RANGE_KEY = 'harness_analytics_range';
  const LOG_KEY = 'harness_analytics_log';

  function stored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
    try {
      const v = localStorage.getItem(key) as T | null;
      return v && allowed.includes(v) ? v : fallback;
    } catch {
      return fallback;
    }
  }
  function remember(key: string, v: string) {
    try {
      localStorage.setItem(key, v);
    } catch {
      // not remembered
    }
  }


  let range = $state<Range>(stored(RANGE_KEY, RANGES, '7d'));
  /** The request inspector shows gateway requests or the tool log. */
  let logView = $state<'gateway' | 'tools'>(stored(LOG_KEY, ['gateway', 'tools'] as const, 'gateway'));

  // The command palette asked for a view ("Analytics: request inspector").
  $effect(() => {
    const requested = app.requestedAnalyticsView;
    if (requested) {
      section.select(requested);
      remember(VIEW_KEY, requested);
      app.requestedAnalyticsView = null;
    }
  });
  let summary = $state<AnalyticsSummary | null>(null);
  let info = $state<AnalyticsInfo | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  /** Bumped to make the request log reload. */
  let reloadKey = $state(0);
  /** Bumped when a gateway request finished: the request log adds it without losing its place. */
  let liveKey = $state(0);

  let enabledDraft = $state(true);
  let retentionDraft = $state<number | null>(30);
  let savingSettings = $state(false);
  let confirmClear = $state(false);

  async function load() {
    loading = true;
    try {
      const first = info === null;
      const [s, i] = await Promise.all([
        invoke<AnalyticsSummary>('get_analytics_summary', { fromMs: rangeStart(range), toMs: null }),
        invoke<AnalyticsInfo>('get_analytics_info'),
      ]);
      summary = s;
      info = i;
      if (first && section.active === 'policy') openSettings(true);
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function refresh() {
    reloadKey++;
    void load();
  }

  onMount(() => {
    const id = new URL(location.href).searchParams.get('request');
    if (id) openRequest(id);
    void load();
    void app.refreshGateway();
    const timer = setInterval(() => {
      if (windowActive()) void load();
    }, 15_000);
    // A finished gateway request (pushed by the backend, already coalesced): the KPIs and the
    // request list follow it. A burst becomes one reload; while the window is hidden the 15 s
    // timer and the next activation catch up.
    let live: ReturnType<typeof setTimeout> | undefined;
    let off: (() => void) | undefined;
    let gone = false;
    listen('gateway-requests-changed', () => {
      if (!windowActive()) return;
      clearTimeout(live);
      live = setTimeout(() => {
        liveKey++;
        void load();
      }, 400);
    })
      .then((fn) => (gone ? fn() : (off = fn)))
      .catch(() => {});
    return () => {
      gone = true;
      clearInterval(timer);
      clearTimeout(live);
      off?.();
    };
  });

  function setRange(r: Range) {
    range = r;
    remember(RANGE_KEY, r);
    void load();
  }

  function openSettings(open: boolean) {
    if (open && info) {
      enabledDraft = info.settings.enabled;
      retentionDraft = info.settings.raw_retention_days;
      confirmClear = false;
    }
  }

  async function saveSettings() {
    savingSettings = true;
    try {
      const days = retentionDraft != null && Number.isFinite(retentionDraft) ? Math.trunc(retentionDraft) : 30;
      info = await invoke<AnalyticsInfo>('set_analytics_settings', {
        settings: { enabled: enabledDraft, raw_retention_days: Math.min(3650, Math.max(1, days)) },
      });
      toast.success(t('an.set_saved'));
      refresh();
    } catch (e) {
      toast.error(String(e));
    } finally {
      savingSettings = false;
    }
  }

  async function clearAll() {
    try {
      info = await invoke<AnalyticsInfo>('clear_analytics');
      confirmClear = false;
      toast.success(t('an.cleared'));
      refresh();
    } catch (e) {
      toast.error(String(e));
    }
  }

  let gatewayOff = $derived(app.gateway != null && !app.gatewayRunning);
  let recorder = $derived(info?.recorder ?? summary?.recorder ?? null);
</script>

<div class="flex h-full min-h-0 overflow-hidden bg-background">
  <SectionSidebar pane={panes.analyticsNav} label={t('nav.analytics')} groups={ANALYTICS_GROUPS} active={section.active} onSelect={selectPage} />
  <main class="flex min-w-0 flex-1 flex-col overflow-hidden">
    <AdvancedPageNotice level={pageLevel(ANALYTICS_GROUPS, section.active)} />
    <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border px-6 pt-3 pb-2">
      <h1 class="text-lg font-semibold">{t(ANALYTICS_GROUPS.flatMap(g => g.pages).find(p => p.id === section.active)!.label)}</h1>
      <div class="flex items-center gap-2">
        <NativeSelect
          class="h-7 w-auto text-xs"
          aria-label={t('an.range')}
          value={range}
          onchange={(e) => setRange((e.currentTarget as HTMLSelectElement).value as Range)}
        >
          {#each RANGES as r (r)}
            <option value={r}>{t(`an.range_${r}`)}</option>
          {/each}
        </NativeSelect>
        <Button variant="outline" size="icon-sm" onclick={refresh} aria-label={t('an.refresh')} title={t('an.refresh')} disabled={loading}>
          <RefreshCw class={loading ? 'animate-spin' : ''} />
        </Button>
        <Button variant="outline" size="icon-sm" aria-label={t('an.settings')} title={t('an.settings')} onclick={() => selectPage('policy')}><SlidersHorizontal /></Button>

      </div>
    </div>

    <div class="flex shrink-0 flex-col gap-1.5 px-6 pt-3">
      <div class="flex items-start gap-2 rounded-md border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
        <Info class="mt-0.5 size-3.5 shrink-0" />
        <p class="flex-1 leading-snug">
          {t('an.direct_note')}
          <button type="button" class="ml-1 text-primary underline-offset-2 hover:underline" onclick={() => (app.requestedTab = 'connections')}>
            {t('an.direct_link')}
          </button>
        </p>
      </div>
      {#if gatewayOff}
        <div class="flex items-center gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs">
          <TriangleAlert class="size-3.5 shrink-0 text-warning" />
          <span class="flex-1">{t('an.gateway_off')}</span>
          <Button variant="outline" size="xs" onclick={() => security.openSettings('gateway')}>{t('an.gateway_settings')}</Button>
        </div>
      {/if}
      {#if info && !info.settings.enabled}
        <div class="flex items-center gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs">
          <TriangleAlert class="size-3.5 shrink-0 text-warning" />
          <span>{t('an.recording_off')}</span>
        </div>
      {/if}
      {#if recorder && recorder.dropped > 0}
        <p class="text-xs text-warning">{t('an.dropped', { n: recorder.dropped })}</p>
      {/if}
      {#if recorder?.last_error}
        <p class="text-xs text-destructive">{t('an.write_error', { error: localizeError(recorder.last_error) })}</p>
      {/if}
      {#if error}
        <p class="text-xs text-destructive" role="alert">{t('an.load_failed', { error })}</p>
      {/if}
    </div>

    <div hidden={!['overview', 'breakdown'].includes(section.active)} class="m-0 min-h-0 flex-1 overflow-y-auto p-0 focus-visible:outline-none">
      <Overview page={section.active} {summary} {range} loadFailed={!!error} onOpenRequests={() => section.select('requests')} />
    </div>
    <div hidden={section.active !== 'requests'} class="m-0 min-h-0 flex-1 overflow-y-auto p-0 focus-visible:outline-none">
      <div class="px-6 pt-3">
        <ToggleGroup.Root
          type="single"
          variant="outline"
          size="sm"
          value={logView}
          onValueChange={(v) => {
            if (v === 'gateway' || v === 'tools') {
              logView = v;
              remember(LOG_KEY, v);
            }
          }}
          aria-label={t('an.tab_requests')}
        >
          <ToggleGroup.Item value="gateway">{t('an.view_requests')}</ToggleGroup.Item>
          <ToggleGroup.Item value="tools">{t('an.view_tools')}</ToggleGroup.Item>
        </ToggleGroup.Root>
      </div>
      {#if logView === 'tools'}
        <ToolCallLog fromMs={rangeStart(range)} {reloadKey} />
      {:else}
        <RequestLog onSelect={openRequest} fromMs={rangeStart(range)} {reloadKey} {liveKey} />
      {/if}
    </div>
    <div hidden={section.active !== 'detail'} class="min-h-0 flex-1 overflow-y-auto">
      {#if selectedRequest}
        {#key selectedRequest}<RequestDetailSheet inline id={selectedRequest} onClose={() => section.select('requests')} onOpen={openRequest} />{/key}
      {:else}
        <div class="flex flex-col items-start gap-3 p-6"><p class="text-sm text-muted-foreground">{t('menus.choose_request')}</p><Button variant="outline" onclick={() => section.select('requests')}>{t('menus.log')}</Button></div>
      {/if}
    </div>
    <div hidden={section.active !== 'policy'} class="min-h-0 flex-1 overflow-y-auto p-6">
      <section class="flex max-w-xl flex-col gap-3">
            <span class="text-xs font-semibold">{t('an.settings')}</span>
            <label class="flex items-start gap-2 text-xs">
              <Switch size="sm" checked={enabledDraft} onCheckedChange={(c) => (enabledDraft = c)} />
              <span class="flex flex-col gap-0.5">
                <span class="font-medium">{t('an.set_enabled')}</span>
                <span class="text-xs text-muted-foreground">{t('an.set_enabled_hint')}</span>
              </span>
            </label>
            <label class="flex flex-col gap-1 text-xs">
              <span class="font-medium">{t('an.set_retention')}</span>
              <FieldHelp text={t('an.set_retention_hint')}><Input type="number" min="1" max="3650" class="h-7 font-num" bind:value={retentionDraft} /></FieldHelp>
              <span class="text-xs text-muted-foreground">{t('an.set_retention_hint')}</span>
            </label>
            <Button size="sm" onclick={saveSettings} disabled={savingSettings}>{t('an.set_save')}</Button>
            {#if info?.storage}
              <p class="font-mono text-xs leading-snug text-muted-foreground" title={info.storage.path}>
                {t('an.storage', {
                  raw: fmtInt(info.storage.raw_rows),
                  rollup: fmtInt(info.storage.rollup_rows),
                  content: fmtInt(info.storage.content_rows),
                  size: fmtBytes(info.storage.file_bytes),
                })}
              </p>
            {/if}
            {#if confirmClear}
              <p class="text-xs text-destructive">{t('an.clear_confirm')}</p>
              <div class="flex gap-2">
                <Button size="sm" variant="outline" class="flex-1" onclick={() => (confirmClear = false)}>{t('ip.cancel')}</Button>
                <Button size="sm" variant="destructive" class="flex-1" onclick={clearAll}>{t('an.clear')}</Button>
              </div>
            {:else}
              <Button size="sm" variant="ghost" class="text-destructive" onclick={() => (confirmClear = true)}>
                <Trash2 />
                {t('an.clear')}
              </Button>
            {/if}
      </section>
    </div>
  </main>
</div>
