<script lang="ts">
  import { onDestroy, onMount, untrack } from 'svelte';
  import { library } from '#lib/stores/library.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { errorCode } from '#lib/errors.js';
  import type { LocalModel } from '#lib/types.js';
  import {
    bestTs,
    type BenchmarkRequest,
    type BenchmarkRun,
    type BenchmarkStatus,
    type BenchPreflight,
    type RunStatus,
  } from '#lib/benchmark-types.js';
  import BenchConfigForm from './benchmark/BenchConfigForm.svelte';
  import BenchRunDetails from './benchmark/BenchRunDetails.svelte';
  import BenchResultsTable from './benchmark/BenchResultsTable.svelte';
  import BenchCompare from './benchmark/BenchCompare.svelte';
  import { Card, CardHeader, CardTitle, CardContent } from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { RefreshCw, Trash2, ChevronDown, ChevronUp } from '@lucide/svelte';
  import { toast } from 'svelte-sonner';
  import { formatBytes } from '#lib/format.js';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';

  interface Props {
    page?: string;
    onSelectPage?: (id: string) => void;
    onGoToSettings?: () => void;
  }

  let { page = 'new', onSelectPage = () => {}, onGoToSettings }: Props = $props();

  const POLL_MS = 500;

  // --- Models ---
  let models = $state<LocalModel[]>([]);
  let modelsLoaded = $state(false);
  let modelsError = $state<string | null>(null);

  // --- Run State ---
  let status = $state<BenchmarkStatus | null>(null);
  let starting = $state(false);
  let cancelling = $state(false);
  let startError = $state<string | null>(null);
  let showLog = $state(false);
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  let destroyed = false;

  const running = $derived(status?.running ?? false);
  // `current` is the test running now (1-based): the bar shows the tests already finished.
  const progressPct = $derived(status && status.total > 0 ? Math.min(100, (Math.max(0, status.current - 1) / status.total) * 100) : 0);
  const runtimeMissing = $derived(!!startError && /runtime|ayarlar|settings|llama-server|llama-bench/i.test(startError));

  let lastSeenRunId: string | null | undefined = undefined;

  async function refreshStatus() {
    try {
      const s = await invoke<BenchmarkStatus>('get_benchmark_status');
      if (destroyed) return;
      const wasRunning = status?.running ?? false;
      status = s;
      const finishedId = s.last_run?.id ?? null;
      const isNew = lastSeenRunId !== undefined && finishedId !== lastSeenRunId;
      lastSeenRunId = finishedId;
      if (!s.running && (wasRunning || isNew)) {
        cancelling = false;
        await loadHistory();
        if (destroyed) return;
        if (s.last_run) viewRunId = s.last_run.id;
      }
    } catch {
      // Transient IPC error: keep last known state.
    }
  }

  async function pollLoop() {
    pollTimer = null;
    await refreshStatus();
    if (!destroyed && status?.running) pollTimer = setTimeout(pollLoop, POLL_MS);
  }

  function startPolling() {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = setTimeout(pollLoop, POLL_MS);
  }

  /** Servers hold GPU memory: the user decides before the measurement starts. */
  let busyAsk = $state<{ req: BenchmarkRequest; preflight: BenchPreflight } | null>(null);

  async function startBenchmark(req: BenchmarkRequest) {
    if (running || starting) return;
    try {
      const preflight = await invoke<BenchPreflight>('benchmark_preflight');
      if (preflight.contended) {
        busyAsk = { req, preflight };
        return;
      }
    } catch {
      // No information about running servers: let the run start.
    }
    await launchBenchmark(req, false);
  }

  async function launchBenchmark(req: BenchmarkRequest, stopServers: boolean) {
    busyAsk = null;
    if (running || starting) return;
    starting = true;
    startError = null;
    compareMode = false;
    try {
      await invoke('start_benchmark', { request: req, stopServers });
      viewRunId = null;
      showLog = false;
      await refreshStatus();
      startPolling();
    } catch (e) {
      startError = localizeError(e);
      toast.error(startError);
    } finally {
      starting = false;
    }
  }

  async function cancel() {
    cancelling = true;
    try {
      await invoke('cancel_benchmark');
    } catch (e) {
      startError = localizeError(e);
      toast.error(startError);
      cancelling = false;
    }
  }

  // --- History ---
  let history = $state<BenchmarkRun[]>([]);
  let historyError = $state<string | null>(null);
  /** The first read of the history is pending (no rows yet to show). */
  let historyLoading = $state(true);
  let viewRunId = $state<string | null>(null);
  let runToDelete = $state<BenchmarkRun | null>(null);
  let confirmClearDialog = $state(false);
  let compareMode = $state(false);
  let compareIds = $state<string[]>([]);
  $effect(() => { compareMode = page === 'compare'; });

  const viewedRun = $derived(
    viewRunId ? (history.find((r) => r.id === viewRunId) ?? (status?.last_run?.id === viewRunId ? status.last_run : null)) : null,
  );
  const compareRunsSel = $derived(compareIds.map((id) => history.find((r) => r.id === id)).filter((r): r is BenchmarkRun => !!r));

  async function loadHistory() {
    historyLoading = true;
    try {
      history = await invoke<BenchmarkRun[]>('list_benchmark_runs');
      historyError = null;
    } catch (e) {
      historyError = String(e);
      toast.error(String(e));
    } finally {
      historyLoading = false;
    }
  }

  async function deleteRun(id: string) {
    try {
      history = await invoke<BenchmarkRun[]>('delete_benchmark_run', { runId: id });
      historyError = null;
    } catch (e) {
      historyError = String(e);
      toast.error(String(e));
    }
    runToDelete = null;
    compareIds = compareIds.filter((c) => c !== id);
    if (viewRunId === id) viewRunId = null;
  }

  async function clearHistory() {
    try {
      await invoke('clear_benchmark_runs');
      history = [];
      historyError = null;
    } catch (e) {
      historyError = String(e);
      toast.error(String(e));
    }
    confirmClearDialog = false;
    compareIds = [];
    compareMode = false;
    viewRunId = null;
  }

  function toggleCompareMode() {
    compareMode = !compareMode;
    onSelectPage(compareMode ? 'compare' : 'results');
    if (compareMode && compareIds.length === 0 && viewRunId) compareIds = [viewRunId];
  }

  function toggleCompareId(id: string) {
    if (compareIds.includes(id)) {
      compareIds = compareIds.filter((c) => c !== id);
    } else {
      compareIds = [...compareIds, id];
    }
  }

  function formatDate(secs: number): string {
    return new Date(secs * 1000).toLocaleString(locale());
  }

  function formatTs(v: number | null): string {
    return v === null ? '—' : `${v.toLocaleString(locale(), { maximumFractionDigits: 1 })} t/s`;
  }

  const statusKey: Record<RunStatus, TKey> = {
    completed: 'bench.status_completed',
    cancelled: 'bench.status_cancelled',
    error: 'bench.status_error',
  };

  const statusBadgeVariant: Record<RunStatus, 'default' | 'secondary' | 'destructive' | 'outline'> = {
    completed: 'secondary',
    cancelled: 'outline',
    error: 'destructive',
  };

  async function loadModels() {
    try {
      const list = await invoke<LocalModel[]>('scan_local_models');
      // llama-bench runs a whole model: a vision projector or a LoRA adapter alone fails.
      models = list.filter((m) => m.kind !== 'mmproj' && m.kind !== 'adapter' && !m.gguf?.is_mmproj && m.is_ready !== false);
      modelsError = null;
    } catch (e) {
      modelsError = String(e);
    } finally {
      modelsLoaded = true;
    }
  }

  // A download finished: refresh the model list.
  let seenLibraryVersion = library.version;
  $effect(() => {
    const v = library.version;
    if (v === seenLibraryVersion) return;
    seenLibraryVersion = v;
    untrack(() => void loadModels());
  });

  onMount(() => {
    loadModels();
    loadHistory();
    refreshStatus().then(() => {
      if (status?.running) startPolling();
    });
  });

  onDestroy(() => {
    destroyed = true;
    if (pollTimer) clearTimeout(pollTimer);
  });
</script>

<div class="flex h-full min-h-0 w-full min-w-0 flex-col gap-6 overflow-y-auto p-4 sm:p-6">
  <!-- Page Header -->
  <div class="flex flex-col gap-1 shrink-0">
    <h1 class="text-lg font-semibold text-foreground truncate">{t('bench.title')}</h1>
    <p class="text-xs text-muted-foreground truncate">{t('bench.subtitle')}</p>
  </div>

<div hidden={page !== 'new'} class="flex flex-col gap-3">
  <!-- Configuration Form -->
  <div class="shrink-0">
    <BenchConfigForm
      {models}
      {modelsLoaded}
      {modelsError}
      {running}
      {starting}
      {startError}
      {runtimeMissing}
      {onGoToSettings}
      onStart={startBenchmark}
    />
  </div>

</div>
  <!-- Live Progress -->
  {#if running && status}
    <Card class="border-border border-primary/30 shrink-0" role="status" aria-live="polite" aria-labelledby="bench-running-title">
      <CardHeader class="pb-3 border-b border-border">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <span class="size-2 rounded-full bg-primary animate-pulse"></span>
            <CardTitle id="bench-running-title" class="text-base font-semibold">{t('bench.running_title')}</CardTitle>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            class="text-destructive border-destructive/30 hover:bg-destructive/10"
            disabled={cancelling}
            onclick={cancel}
          >
            {cancelling ? t('bench.cancelling') : t('bench.cancel')}
          </Button>
        </div>
      </CardHeader>
      <CardContent class="pt-4 flex flex-col gap-4">
        <div class="flex items-center justify-between text-xs">
          <span class="font-medium text-foreground">
            {#if status.total > 0}
              {t('bench.progress', { current: status.current, total: status.total })}
            {:else}
              {t('bench.preparing')}
            {/if}
          </span>
          {#if status.phase}
            <span class="text-muted-foreground font-mono text-xs">{status.phase}</span>
          {/if}
        </div>

        <Progress value={status.total > 0 ? progressPct : undefined} max={100} class="h-2" />

        <div class="flex flex-col gap-2 mt-2">
          <h3 class="text-sm font-semibold text-foreground">{t('bench.live_results')}</h3>
          {#if status.results.length > 0}
            <BenchResultsTable results={status.results} />
          {:else}
            <div class="py-6 text-center text-xs text-muted-foreground">{t('bench.no_results')}</div>
          {/if}
        </div>

        <div class="pt-2 border-t border-border">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            class="text-xs text-muted-foreground hover:text-foreground p-0 h-auto"
            aria-expanded={showLog}
            onclick={() => (showLog = !showLog)}
          >
            {#if showLog}
              <ChevronUp size={13} class="mr-1" />
              {t('bench.hide_log')}
            {:else}
              <ChevronDown size={13} class="mr-1" />
              {t('bench.show_log')}
            {/if}
          </Button>
          {#if showLog}
            <pre class="mt-2 p-3 rounded-lg bg-terminal text-terminal-foreground font-mono text-xs max-h-48 overflow-y-auto whitespace-pre-wrap">{status.log_tail.length ? status.log_tail.join('\n') : t('bench.log_empty')}</pre>
          {/if}
        </div>
      </CardContent>
    </Card>
  {/if}

<div hidden={page === 'new'} class="flex flex-col gap-3">
  <!-- Comparison Mode / Run Details -->
  {#if compareMode}
    <div class="shrink-0">
      <Card class="border-border" aria-labelledby="bench-compare-title">
        <CardHeader class="pb-3 border-b border-border flex flex-row items-center justify-between">
          <CardTitle id="bench-compare-title" class={page === 'compare' ? 'sr-only' : 'text-base font-semibold'}>{t('bench.compare_title')}</CardTitle>
          <Button variant="outline" size="sm" onclick={toggleCompareMode}>
            {t('bench.compare_exit')}
          </Button>
        </CardHeader>
        <CardContent class="pt-4">
          {#if compareRunsSel.length >= 2}
            <BenchCompare runs={compareRunsSel} />
          {:else}
            <div class="py-8 text-center text-xs text-muted-foreground">{t('bench.compare_hint')}</div>
          {/if}
        </CardContent>
      </Card>
    </div>
  {:else if viewedRun && !running}
    <div class="shrink-0">
      <BenchRunDetails run={viewedRun} onClose={() => (viewRunId = null)} />
    </div>
  {:else if !running && !historyLoading && history.length === 0}
    <div class="p-8 rounded-lg border border-dashed border-border text-center shrink-0">
      <p class="text-sm font-semibold text-foreground mb-1">{t('bench.no_results')}</p>
      <p class="text-xs text-muted-foreground">{t('bench.no_results_hint')}</p>
    </div>
  {/if}

</div>
<div hidden={page === 'new'} class="flex flex-col gap-3">
  <!-- History Section -->
  <Card class="border-border shrink-0" aria-labelledby="bench-history-title">
    <CardHeader class="pb-3 border-b border-border flex flex-row items-center justify-between flex-wrap gap-2">
      <CardTitle id="bench-history-title" class="text-base font-semibold">{t('bench.history_title')}</CardTitle>
      <div class="flex items-center gap-2">
        <Button variant="outline" size="sm" class="h-8 w-8 p-0" aria-label={t('bench.refresh')} title={t('bench.refresh')} onclick={loadHistory}>
          <RefreshCw size={13} />
        </Button>
        {#if history.length > 1}
          <Button variant="secondary" size="sm" aria-pressed={compareMode} onclick={toggleCompareMode}>
            {compareMode ? t('bench.compare_exit') : t('bench.compare')}
          </Button>
        {/if}
        {#if history.length > 0}
          <Button
            variant="outline"
            size="sm"
            class="text-destructive border-destructive/30 hover:bg-destructive/10"
            onclick={() => (confirmClearDialog = true)}
          >
            {t('bench.clear_all')}
          </Button>
        {/if}
      </div>
    </CardHeader>
    <CardContent class="p-0">
      {#if historyError}
        <div class="p-3 text-xs text-destructive border-b border-border">{t('bench.history_error', { error: historyError })}</div>
      {/if}

      {#if historyLoading && history.length === 0}
        <SectionLoading shape="rows" rows={3} class="m-3" label={t('sl.bench_label')} phases={[t('sl.bench_1'), t('sl.bench_2')]} />
      {:else if history.length === 0}
        <div class="p-8 text-center text-xs text-muted-foreground">{t('bench.history_empty')}</div>
      {:else}
        <ul class="divide-y divide-border">
          {#each history as run (run.id)}
            {@const isViewed = !compareMode && viewRunId === run.id}
            <li class="flex items-center justify-between gap-4 p-3 hover:bg-muted/30 transition-colors {isViewed ? 'bg-primary/5' : ''}">
              <div class="flex items-center gap-3 min-w-0 flex-1">
                {#if compareMode}
                  <input
                    type="checkbox"
                    checked={compareIds.includes(run.id)}
                    aria-label={t('bench.compare_select', { name: run.model_name })}
                    onchange={() => toggleCompareId(run.id)}
                    class="rounded border-input text-primary focus:ring-primary size-4 shrink-0 cursor-pointer"
                  />
                {/if}
                <button
                  type="button"
                  class="flex flex-col text-left min-w-0 flex-1 cursor-pointer disabled:cursor-not-allowed"
                  disabled={running}
                  onclick={() => {
                    if (compareMode) toggleCompareId(run.id);
                    else viewRunId = isViewed ? null : run.id;
                  }}
                >
                  <div class="flex items-center gap-2">
                    <span class="text-xs font-semibold text-foreground truncate" title={run.model_name}>{run.model_name}</span>
                    <span class="text-xs text-muted-foreground">{formatDate(run.started_at)}</span>
                  </div>
                  <div class="flex items-center gap-3 mt-1 text-xs font-num text-muted-foreground">
                    <span>{t('bench.best_pp')}: <strong class="text-foreground">{formatTs(bestTs(run.results, 'pp'))}</strong></span>
                    <span>{t('bench.best_tg')}: <strong class="text-foreground">{formatTs(bestTs(run.results, 'tg'))}</strong></span>
                  </div>
                </button>
              </div>

              <div class="flex items-center gap-2 shrink-0">
                <Badge variant={statusBadgeVariant[run.status]} class="text-xs">
                  {t(errorCode(run.error) === 'benchmark.timeout' ? 'bench.status_timeout' : statusKey[run.status])}
                </Badge>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-7 w-7 p-0 text-destructive hover:bg-destructive/10"
                  aria-label={t('bench.delete_run')}
                  title={t('bench.delete_run')}
                  disabled={running}
                  onclick={() => (runToDelete = run)}
                >
                  <Trash2 size={13} />
                </Button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </CardContent>
  </Card>
</div>
</div>

<!-- Clear All Confirmation Dialog -->
<Dialog.Root open={confirmClearDialog} onOpenChange={(open) => { if (!open) confirmClearDialog = false; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('bench.confirm_clear')}</Dialog.Title>
    </Dialog.Header>
    <Dialog.Footer class="gap-2 sm:gap-0 mt-4">
      <Button variant="outline" onclick={() => (confirmClearDialog = false)}>{t('bench.no')}</Button>
      <Button variant="destructive" onclick={clearHistory}>{t('bench.yes')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Running servers distort the measurement -->
<Dialog.Root open={!!busyAsk} onOpenChange={(open) => { if (!open) busyAsk = null; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('bench.busy_title')}</Dialog.Title>
    </Dialog.Header>
    {#if busyAsk}
      <p class="text-sm text-muted-foreground">
        {t('bench.busy_body', {
          n: busyAsk.preflight.servers.length,
          names: busyAsk.preflight.servers.map((s) => s.name).join(', '),
          free: busyAsk.preflight.vram_free_bytes != null ? formatBytes(busyAsk.preflight.vram_free_bytes, 1) : '?',
        })}
      </p>
    {/if}
    <Dialog.Footer class="gap-2 sm:gap-0 mt-4">
      <Button variant="outline" onclick={() => (busyAsk = null)}>{t('bench.no')}</Button>
      <Button variant="outline" onclick={() => busyAsk && launchBenchmark(busyAsk.req, false)}>{t('bench.busy_anyway')}</Button>
      <Button onclick={() => busyAsk && launchBenchmark(busyAsk.req, true)}>{t('bench.busy_stop')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Delete Run Confirmation Dialog -->
<Dialog.Root open={!!runToDelete} onOpenChange={(open) => { if (!open) runToDelete = null; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('bench.confirm_delete')}</Dialog.Title>
    </Dialog.Header>
    <Dialog.Footer class="gap-2 sm:gap-0 mt-4">
      <Button variant="outline" onclick={() => (runToDelete = null)}>{t('bench.no')}</Button>
      <Button variant="destructive" onclick={() => runToDelete && deleteRun(runToDelete.id)}>{t('bench.yes')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
