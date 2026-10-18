<script lang="ts">
  import { testFailureSummary } from './generatorProgress';
  import AttemptHistory from './AttemptHistory.svelte';
  import InfoTip from '#lib/components/common/InfoTip.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import HelpPopover from '#lib/components/common/HelpPopover.svelte';
  import { onMount, tick } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { ArrowUp, ArrowDown, GripVertical, Workflow, Pause, Square, Play, CircleCheck, ShieldAlert, Check } from '@lucide/svelte';
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import ToolTestReport from '../tools/ToolTestReport.svelte';
  import ModelPicker from './ModelPicker.svelte';
  import GatewayError from '../common/GatewayError.svelte';
  import { rawError } from '#lib/errors.js';
  import { tokenEstimate, action, retryFailed as retryAllFailed, start, state as getRunState, categories, categoryLabel, statusLabel, type RunState } from './client';
  import type { GeneratorEstimate, GeneratorUsage, GenerateToolRequest } from '#lib/tools-client.js';
  let { onChanged }: { onChanged: () => void } = $props();
  const filterHelpId = $props.id();
  let run = $state<RunState | null>(null);
  let endpoint = $state<GenerateToolRequest['endpoint'] | null>(null);
  let selectedModel = $state('');
  let count = $state(8);
  let estimate = $state<GeneratorEstimate | null>(null);
  // The previous estimate stays (dimmed) until the new one arrives: dropping it moved the
  // start button left for 400 ms, so a click right after editing the count missed (S41).
  let estimating = $state(false);
  const fmt = (n: number) => Math.round(n).toLocaleString(locale());
  const total = (usage?: GeneratorUsage) => (usage?.prompt_tokens ?? 0) + (usage?.completion_tokens ?? 0);
  const range = (e: GeneratorEstimate) => e.high >= 10_000 ? `≈ ${(e.low / 1000).toLocaleString(locale(), {maximumFractionDigits:1})}–${(e.high / 1000).toLocaleString(locale(), {maximumFractionDigits:1})} ${t('create.token_thousand')} token` : `≈ ${fmt(e.low)}–${fmt(e.high)} token`;
  $effect(() => {
    const selected = endpoint; const n = count;
    let cancelled = false;
    estimating = true;
    const timer = setTimeout(() => {
      if (selected && n >= 1 && n <= 20) void tokenEstimate({endpoint:selected,description:'Useful transformation',network:[],fs_read:[],fs_write:[]}, n, true)
        .then(value => { if (!cancelled) estimate = value; }).catch(() => { if (!cancelled) estimate = null; })
        .finally(() => { if (!cancelled) estimating = false; });
      else { estimate = null; estimating = false; }
    }, 400);
    return () => { cancelled = true; clearTimeout(timer); };
  });
  let busy = $state(false);
  let error = $state('');
  type QueueFilter = 'all' | 'queued' | 'running' | 'installed' | 'awaiting_approval' | 'failed' | 'built';
  const filters: QueueFilter[] = ['all', 'queued', 'running', 'installed', 'awaiting_approval', 'failed'];
  let filter = $state<QueueFilter>('all');
  function accepts(status: string, draft: unknown, selected: QueueFilter) {
    return selected === 'all' || (selected === 'running' ? ['building', 'testing', 'installing'].includes(status) : selected === 'built' ? !!draft : status === selected);
  }
  let visible = $derived(run?.queue.filter(q => accepts(q.status, q.draft, filter)) ?? []);
  let reorderEnabled = $derived(filter === 'all' || filter === 'queued');
  let failedCount = $derived(run?.queue.filter(q => q.status === 'failed').length ?? 0);
  function chooseFilter(selected: QueueFilter) {
    filter = selected; dragged = null;
    try { localStorage.setItem('harness.autoTools.filter', selected); } catch { /* Storage may be unavailable. */ }
  }
  const filterLabel = (selected: QueueFilter) => t(('create.filter_' + selected) as TKey);
  async function retry(id: string | null = null) {
    await work(async () => {
      if (!running && !endpoint) throw new Error(t('create.retry_endpoint_required'));
      const selectedEndpoint = endpoint;
      const retried = id === null ? await retryAllFailed() : await action('retry', id);
      run = retried;
      if (retried.status !== 'running') {
        if (!selectedEndpoint) throw new Error(t('create.retry_endpoint_required'));
        await start(selectedEndpoint, retried.count, true);
      }
    });
  }
  function itemError(text: string) {
    const prefix = 'Test/name/category validation failed:';
    return text.startsWith(prefix) ? t('create.tests_failed', { detail: localizeError(text.slice(prefix.length).trimStart()) }) : localizeError(text);
  }
  let dragged = $state<string | null>(null);
  let terminal: HTMLDivElement | undefined;
  const stages = ['scan', 'ideas', 'queue', 'build', 'test', 'install'];
  let running = $derived(run?.status === 'running');
  let stageIndex = $derived(Math.max(0, stages.indexOf(run?.stage ?? 'scan')));
  let queued = $derived(run?.queue.filter(q => q.status === 'queued') ?? []);
  let canResume = $derived(!!run && (['paused', 'stopped', 'failed'].includes(run.status) || (run.status === 'completed' && queued.length > 0)));
  let counters = $derived([
    ['create.scanned', run?.inventory.length ?? 0], ['create.planned', run?.queue.length ?? 0],
    ['create.built', run?.queue.filter(q => q.draft).length ?? 0],
    ['create.installed', run?.queue.filter(q => q.status === 'installed').length ?? 0],
    ['create.awaiting', run?.queue.filter(q => q.status === 'awaiting_approval').length ?? 0],
    ['create.failed', run?.queue.filter(q => q.status === 'failed').length ?? 0],
  ] as [TKey, number][]);
  const counterFilters: QueueFilter[] = ['all', 'all', 'built', 'installed', 'awaiting_approval', 'failed'];
  onMount(() => {
    try { const savedFilter = localStorage.getItem('harness.autoTools.filter'); if (savedFilter && [...filters, 'built'].includes(savedFilter as QueueFilter)) filter = savedFilter as QueueFilter; } catch { /* Use all by default. */ }
    let dead = false; let off: UnlistenFn | undefined;
    void listen<RunState>('auto-tools-state', e => { if (!dead) { run = e.payload; if (e.payload.status === 'completed') onChanged(); } })
      .then(async fn => { if (dead) return fn(); off = fn; const saved = await getRunState(); if (!dead) { run = saved; count = saved.count; } })
      .catch(e => { if (!dead) error = localizeError(e); });
    return () => { dead = true; off?.(); };
  });
  $effect(() => {
    void run?.logs.length;
    void tick().then(() => { if (terminal) terminal.scrollTop = terminal.scrollHeight; });
  });
  async function work(fn: () => Promise<unknown>) {
    if (busy) return; busy = true; error = '';
    try { await fn(); run = await getRunState(); } catch (e) { error = rawError(e); }
    finally { busy = false; }
  }
  const launch = (resume = false) => work(async () => { if (endpoint) await start(endpoint, count, resume); });
  const act = (kind: string, id: string | null = null) => work(async () => { await action(kind, id); if (kind === 'approve') onChanged(); });
  function move(id: string, target: number) {
    if (!reorderEnabled) return;
    const ids = queued.map(q => q.id); const index = ids.indexOf(id);
    if (index < 0 || target < 0 || target >= ids.length) return;
    ids.splice(index, 1); ids.splice(target, 0, id);
    void work(() => action('reorder', null, ids));
  }
</script>
<p class="text-sm text-muted-foreground">{t('create.auto_desc')}</p>
<div class="flex flex-col gap-3 border-b border-border pb-4">
  <ModelPicker bind:endpoint bind:selectedModel disabled={running || busy} />
  <div class="flex flex-wrap items-end gap-3">
    <label class="flex flex-col gap-1 text-xs text-muted-foreground">{t('create.count')}<FieldHelp text={t('help.count')}><Input type="number" min={1} max={20} bind:value={count} disabled={running || busy} class="w-24 font-num" /></FieldHelp></label>
    {#if estimate}<span data-token-estimate class="text-xs text-muted-foreground transition-opacity" class:opacity-50={estimating}>{range(estimate)} <InfoTip text={t('create.token_estimate_help', { prompt: fmt(estimate.prompt_per_tool), reply: fmt(estimate.reply_per_tool) })} /></span>{/if}
    {#if running}
      <Button variant="outline" disabled={busy} onclick={() => act('pause')}><Pause class="size-4" />{t('create.pause')}</Button>
      <Button variant="outline" disabled={busy} onclick={() => act('stop')}><Square class="size-4" />{t('create.stop')}</Button>
    {:else if canResume}
      <Button disabled={!endpoint || busy} onclick={() => launch(true)}><Play class="size-4" />{t('create.resume')}</Button>
    {:else}
      <Button disabled={!endpoint || busy || !count || count < 1 || count > 20 || !!queued.length || !!run?.queue.some(q => q.status === 'awaiting_approval')} onclick={() => launch()}><Workflow class="size-4" />{t('create.start')}</Button>
    {/if}
    {#if run}<Badge variant="outline" class={running ? 'text-primary' : run.status === 'completed' ? 'text-success' : ''}>{statusLabel(run.status)}</Badge>{/if}
  </div>
  <p class="text-xs text-muted-foreground">{t('create.policy')}</p>
</div>
{#if error || run?.error}<GatewayError error={error || run?.error} onUse={model => { selectedModel = model; error = ''; if (run) run.error = null; }} onRetry={() => launch(canResume)} />{/if}
<ol class="grid grid-cols-[repeat(auto-fit,minmax(min(100%,7rem),1fr))] gap-2" aria-label={t('create.auto')}>
  {#each stages as stage, i}
    {@const active = running && run?.stage === stage}
    {@const done = !!run && (run.status === 'completed' || (stageIndex > i && run.status !== 'idle'))}
    <li class="stage relative flex min-w-0 items-center gap-2 overflow-hidden rounded-md border border-border px-3 py-3 text-xs" class:stage-active={active} class:stage-done={done && !active} class:text-primary={active} aria-current={active ? 'step' : undefined}>
      <span class="stage-index font-num">{#if done && !active}<Check class="size-3.5 text-success" aria-hidden="true" />{:else}{i + 1}{/if}</span><span>{t(('create.stage_' + stage) as TKey)}</span>
      {#if active}<span class="pipeline-progress absolute inset-x-0 bottom-0 h-0.5 bg-primary" aria-hidden="true"></span>{/if}
    </li>
  {/each}
</ol>
<!-- One help for the counter group: a help icon beside every counter broke the grid alignment. -->
<div class="relative grid grid-cols-2 gap-2 border-y border-border py-3 pr-8 sm:grid-cols-3 lg:grid-cols-4 2xl:grid-cols-7" aria-live="polite">
  <span class="absolute right-0 top-3"><HelpPopover text={t('create.filter_help')} /></span><span id={filterHelpId} class="sr-only">{t('create.filter_help')}</span>
  <span data-run-tokens class="relative flex min-w-0 flex-col items-start gap-1 px-2 py-1 pr-8"><span class="font-num inline-block w-full whitespace-nowrap text-lg">{run?.usage?.estimated ? '≈ ' : ''}{fmt(total(run?.usage))}</span><span class="text-xs text-muted-foreground">{t('create.tokens_spent')}</span><span class="absolute right-0 top-0"><InfoTip text={t('create.token_usage_help', { input: fmt(run?.usage?.prompt_tokens ?? 0), output: fmt(run?.usage?.completion_tokens ?? 0) })} /></span></span>
  {#each counters as [label, value], i}<button type="button" data-queue-counter={i} aria-describedby={filterHelpId} class="flex w-full min-w-0 flex-col items-start gap-1 rounded-md px-2 py-1 hover:bg-muted" aria-pressed={filter === counterFilters[i]} onclick={() => chooseFilter(counterFilters[i])}><span class="font-num inline-block min-w-[3ch] text-lg font-medium">{value}</span><span class="text-xs text-muted-foreground">{t(label)}</span></button>{/each}
</div>

{#if run?.inventory.length}
  <details><summary class="cursor-pointer text-xs font-medium">{t('create.inventory')} · {run.inventory.length}</summary>
    <div class="mt-3 flex flex-col gap-2">{#each categories as category}
      {@const tools = run.inventory.filter(tool => tool.category === category)}
      {#if tools.length}<div class="flex flex-wrap items-center gap-2"><span class="w-28 text-xs text-muted-foreground">{categoryLabel(category)}</span>{#each tools as tool}<span title={tool.description + ' · ' + tool.source + (tool.server ? ' · ' + tool.server : '')} class="rounded-md border border-border px-2 py-1 font-mono text-xs">{tool.name}</span>{/each}</div>{/if}
    {/each}</div>
  </details>
{/if}
<div class="grid items-start gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
  <section class="min-w-0">
    <h2 class="text-sm font-semibold">{t('create.queue')}</h2><p class="mt-1 text-xs text-muted-foreground">{t('create.queue_hint')}</p>
    <div class="mt-3 flex flex-wrap items-center gap-2" role="group" aria-label={t('create.filter_help')}>
      {#each filters as selected}
        <Button variant={filter === selected ? 'default' : 'outline'} size="sm" data-queue-filter={selected} aria-describedby={filterHelpId} aria-pressed={filter === selected} onclick={() => chooseFilter(selected)}>{filterLabel(selected)} <span class="font-num inline-block w-[3ch] text-right">{run?.queue.filter(q => accepts(q.status, q.draft, selected)).length ?? 0}</span></Button>
      {/each}
      <HelpPopover text={t('create.filter_help')} />
      {#if filter === 'built'}<Badge variant="outline">{filterLabel('built')}</Badge>{/if}
      <FieldHelp text={t('create.retry_failed_help')}><Button variant="outline" size="sm" data-retry-failed disabled={busy || !failedCount} onclick={() => retry()}>{t('create.retry_failed')} (<span class="font-num inline-block w-[3ch] text-right">{failedCount}</span>)</Button></FieldHelp>
    </div>
    {#if run?.queue.length && !visible.length}<p class="py-8 text-sm text-muted-foreground">{t('create.filter_empty')}</p>{/if}
    {#if !run?.queue.length}<p class="py-8 text-sm text-muted-foreground">{t('create.empty')}</p>{/if}
    <ol class="mt-3 flex flex-col gap-2">
      {#each visible as item (item.id)}
        <li class="min-w-0 rounded-lg border border-border bg-card p-3" draggable={item.status === 'queued' && !busy && reorderEnabled}
          ondragstart={e => { dragged = item.id; e.dataTransfer?.setData('text/plain', item.id); }}
          ondragend={() => dragged = null}
          ondragover={e => { if (reorderEnabled && !busy && item.status === 'queued') e.preventDefault(); }}
          ondrop={e => { e.preventDefault(); if (reorderEnabled && !busy && dragged && item.status === 'queued') move(dragged, queued.findIndex(q => q.id === item.id)); dragged = null; }}>
          <div class="flex items-start justify-between gap-2">
            <div class="min-w-0"><p class="flex items-center gap-1.5 font-mono text-sm font-medium">{#if item.status === 'queued'}<GripVertical class="size-4 shrink-0 text-muted-foreground" />{:else if item.status === 'installed'}<CircleCheck class="size-4 shrink-0 text-success" />{:else if item.status === 'awaiting_approval'}<ShieldAlert class="size-4 shrink-0 text-warning" />{/if}<span class="truncate">{item.proposal.name}</span></p><p class="mt-1 text-xs text-muted-foreground">{item.proposal.description}</p></div>
            <Badge variant="outline" class={'shrink-0 ' + (item.status === 'failed' ? 'text-destructive' : item.status === 'installed' ? 'text-success' : item.status === 'awaiting_approval' ? 'text-warning' : 'text-muted-foreground')}>{statusLabel(item.status)}</Badge>
          </div>
          <div class="mt-2 flex flex-wrap items-center gap-1"><span class="mr-auto text-xs text-muted-foreground">{categoryLabel(item.proposal.category)}</span>
            {#if item.status === 'queued'}
              {@const position = queued.findIndex(q => q.id === item.id)}
              <Button variant="ghost" size="icon-sm" aria-label={t('create.up') + ': ' + item.proposal.name} disabled={busy || !reorderEnabled || position === 0} onclick={() => move(item.id, position - 1)}><ArrowUp class="size-4" /></Button>
              <Button variant="ghost" size="icon-sm" aria-label={t('create.down') + ': ' + item.proposal.name} disabled={busy || !reorderEnabled || position === queued.length - 1} onclick={() => move(item.id, position + 1)}><ArrowDown class="size-4" /></Button>
            {/if}
            {#if item.status === 'failed'}<Button variant="outline" size="sm" data-retry-item={item.id} disabled={busy} onclick={() => retry(item.id)}>{t('create.retry')}</Button>{/if}
            {#if ['queued', 'failed', 'awaiting_approval'].includes(item.status)}<Button variant="ghost" size="sm" disabled={busy} onclick={() => act('remove', item.id)}>{t('create.remove')}</Button>{/if}
          </div>
          <p data-item-tokens class="font-num mt-2 text-xs text-muted-foreground">{t('create.tokens_spent')}: {item.usage?.estimated ? '≈ ' : ''}<span class="inline-block min-w-[12ch] whitespace-nowrap">{fmt(total(item.usage))}</span></p>
          {#if item.error}
            {@const summary = item.draft && !item.draft.report.passed ? testFailureSummary(item.draft.report) : null}
            <p class="mt-2 text-xs text-destructive">{summary ? t(summary.key, { count: summary.count }) : t('create.package_failed')}</p>
          {/if}
          {#if item.draft}
            <details class="mt-3 text-xs"><summary class="cursor-pointer text-muted-foreground">{t('create.details')}</summary><div class="mt-2 flex min-w-0 flex-col gap-3">
              {#if item.error}<pre class="max-h-32 overflow-auto whitespace-pre-wrap break-words text-destructive">{itemError(item.error)}</pre>{/if}
              <AttemptHistory history={item.history ?? item.draft.history ?? []} />
              <ToolTestReport report={item.draft.report} />
              {#each [['tools.gen_manifest', item.draft.manifest_json], ['tools.gen_code', item.draft.code], ['tools.gen_test_code', item.draft.test]] as [label, code]}<div><p class="mb-1 font-medium">{t(label as TKey)}</p><pre class="max-h-56 overflow-auto rounded-md bg-muted p-2 font-mono whitespace-pre">{code}</pre></div>{/each}
            </div></details>
          {/if}
          {#if item.error && !item.draft}
            <details class="mt-3 text-xs"><summary class="cursor-pointer text-muted-foreground">{t('create.details')}</summary><pre class="mt-2 max-h-32 overflow-auto whitespace-pre-wrap break-words">{itemError(item.error)}</pre>{#if item.history?.length}<AttemptHistory history={item.history} />{/if}</details>
          {/if}
          {#if item.status === 'awaiting_approval' && item.draft}
            <div class="mt-3 border-t border-border pt-3 text-xs">
              <p class="font-medium">{t('tools.inst_confirm_asks')}</p>
              <ul class="my-2 space-y-1">
                <li>{item.draft.info.network.includes('*') ? t('tools.network_any') : item.draft.info.network.length ? t('tools.inst_perm_network', { hosts: item.draft.info.network.join(', ') }) : t('tools.inst_perm_no_network')}</li>
                <li>{item.draft.info.fs_read.length ? t('tools.inst_perm_read', { folders: item.draft.info.fs_read.join(', ') }) : t('tools.inst_perm_no_files')}</li>
                {#if item.draft.info.fs_write.length}<li>{t('tools.inst_perm_write', { folders: item.draft.info.fs_write.join(', ') })}</li>{/if}
                <li>{t('tools.inst_perm_shell')}</li>
              </ul>
              <p class="mb-2 text-muted-foreground">{t('create.approval_note')}</p>
              <Button variant="outline" disabled={busy} onclick={() => act('approve', item.id)}>{t('create.approve')}</Button>
            </div>
          {/if}
        </li>
      {/each}
    </ol>
  </section>
  <section class="min-w-0 xl:sticky xl:top-0">
    <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
      <h2 class="text-sm font-semibold">{t('create.log')}</h2>
      <Button variant="outline" size="sm" disabled={busy || !run?.logs.length} onclick={() => act('clear_logs')}>{t('create.clear_log')}</Button>
    </div>
    <div bind:this={terminal} class="h-80 overflow-auto rounded-lg border border-border bg-muted/30 p-3 font-mono font-num text-xs leading-relaxed" role="log" aria-live="off">
      {#each run?.logs ?? [] as entry, i}<p class="break-words"><span class="text-muted-foreground">{new Date(entry.at).toLocaleTimeString()} </span><span class="text-primary">[{entry.stage}] </span>{entry.message}</p>{/each}{#if running}<p class="text-primary"><span class="cursor" aria-hidden="true">&#9608;</span></p>{/if}
      {#if !run?.logs.length}<p class="text-muted-foreground">{t('create.log_empty')}</p>{/if}
    </div>
    <p class="mt-2 text-xs text-muted-foreground">{t('create.persisted')}</p>
  </section>
</div>
{#if run?.status === 'completed' && run.estimate?.high}
  <div data-token-comparison class="rounded-lg border border-border p-3 text-xs"><p class="font-medium">{t('create.token_comparison')}</p><p>{t('create.token_estimated')}: {range(run.estimate)} · {t('create.token_actual')}: {run.usage?.estimated ? '≈ ' : ''}{fmt(total(run.usage))} · {t('create.token_difference')}: {fmt(total(run.usage) - (run.estimate.low + run.estimate.high) / 2)} <InfoTip text={t('create.token_difference_help')} /></p></div>
{/if}
<style>
  .stage { transition: border-color .3s, background-color .3s, box-shadow .3s; }
  .stage-active { border-color: var(--primary); background: color-mix(in oklab, var(--primary) 8%, transparent); box-shadow: 0 0 18px -6px var(--primary); }
  .stage-done { border-color: color-mix(in oklab, var(--success, var(--primary)) 45%, var(--border)); }
  .cursor { animation: blink 1s steps(1) infinite; }
  @keyframes blink { 50% { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .cursor { animation: none; } .stage { transition: none; } }
  .pipeline-progress { animation: pipeline 1.4s ease-in-out infinite; transform-origin: left; }
  @keyframes pipeline { 0%, 100% { transform: scaleX(.15); } 50% { transform: scaleX(1); } }
  @media (prefers-reduced-motion: reduce) { .pipeline-progress { animation: none; } }
</style>
