<script lang="ts">
  import { fixed } from '#lib/format.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedOnly from '#lib/components/common/AdvancedOnly.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import type { Snippet } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import type { LogFileInfo, LogMatch } from '#lib/types.js';
  import { toast } from 'svelte-sonner';
  import { Search, ArrowUp, ArrowDown, Copy, Download, Trash2, Clock, SlidersHorizontal, TriangleAlert, OctagonX } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import * as Popover from '#lib/components/ui/popover/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { cn } from '#lib/utils.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { logFilter } from '#lib/stores/serverFilter.svelte.js';
  import { slotColor, slotHatched } from '#lib/serverColor.js';
  import ServerChips from '../common/ServerChips.svelte';
  import NativeSelect from './parts/NativeSelect.svelte';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';
  import {
    buildMatcher,
    filterLines,
    type LogEntry,
    linesFromText,
    splitHighlight,
    type LevelFilter,
    type LogRow,
  } from './logFilter';

  interface Props {
    /** Live lines of the shown servers, in arrival order (each says which server it is from). */
    logs: LogEntry[];
    /** Servers the console can show (the chips) and the ones it shows now. */
    sourceIds: string[];
    shownIds: string[];
    onClear: () => void;
  }

  let { logs, sourceIds, shownIds, onClear }: Props = $props();

  /** The one server shown: saved runs and export are about it. null while several are shown. */
  let serverId = $derived(shownIds.length === 1 ? shownIds[0] : null);
  /** Lines from several servers carry a tag with the server's colour. */
  let tagged = $derived(shownIds.length > 1);

  const uid = $props.id();

  /** Saved run logs above this size are searched by the backend instead of in memory. */
  const BACKEND_SEARCH_BYTES = 2 * 1024 * 1024;
  /** Most lines rendered at once (a saved run can hold ~100k lines). */
  const RENDER_CAP = 5000;
  const DEBOUNCE_MS = 200;

  let autoScroll = $state<boolean>(true);
  let logContainer = $state<HTMLDivElement | null>(null);
  let cardEl = $state<HTMLElement | null>(null);
  let searchInput = $state<HTMLElement | null>(null);

  // ---- Search / filter state ----
  let queryInput = $state('');
  let query = $state('');
  let caseSensitive = $state(false);
  let useRegex = $state(false);
  let onlyMatching = $state(true);
  let level = $state<LevelFilter>('all');
  let currentMatch = $state(-1);
  let levelOptions = $derived<{ value: LevelFilter; label: string }[]>([
    { value: 'all', label: t('dash.lc_level_all') },
    { value: 'warn', label: t('dash.lc_level_warn') },
    { value: 'error', label: t('dash.lc_level_error') },
  ]);

  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const q = queryInput;
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => (query = q), DEBOUNCE_MS);
    return () => clearTimeout(debounceTimer);
  });

  // ---- History (saved run logs) ----
  let files = $state<LogFileInfo[]>([]);
  /** '' = live buffer, otherwise a run log file name. */
  let selectedFile = $state('');
  let historyEntries = $state<LogEntry[]>([]);
  let historyLoading = $state(false);
  let historyError = $state<string | null>(null);
  let backendResults = $state<LogEntry[] | null>(null);
  let backendError = $state<string | null>(null);
  let loadToken = 0;
  let searchToken = 0;

  let selectedInfo = $derived(files.find((f) => f.name === selectedFile) ?? null);
  let isHistory = $derived(selectedFile !== '');
  let useBackendSearch = $derived(
    isHistory && !!query && (selectedInfo?.size_bytes ?? 0) > BACKEND_SEARCH_BYTES
  );

  function fmtDate(unixSecs: number): string {
    return new Date(unixSecs * 1000).toLocaleString(locale());
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${fixed(bytes / 1024, 0)} KB`;
    return `${fixed(bytes / 1024 / 1024, 1)} MB`;
  }

  async function refreshFiles() {
    const id = serverId;
    if (!id) {
      files = [];
      return;
    }
    try {
      const list = await invoke<LogFileInfo[]>('list_server_log_files', { serverId: id });
      if (id === serverId) files = list;
    } catch {
      if (id === serverId) files = [];
    }
  }

  // Switching servers: back to the live view and reload that server's run list.
  $effect(() => {
    void serverId;
    backToLive();
    refreshFiles();
  });

  async function openFile(name: string) {
    selectedFile = name;
    historyError = null;
    backendResults = null;
    currentMatch = -1;
    if (!name) {
      historyEntries = [];
      return;
    }
    autoScroll = false;
    historyLoading = true;
    const token = ++loadToken;
    try {
      const text = await invoke<string>('read_server_log_file', { serverId: serverId ?? '', name });
      if (token !== loadToken) return;
      historyEntries = linesFromText(text);
      requestAnimationFrame(() => {
        if (logContainer) logContainer.scrollTop = logContainer.scrollHeight;
      });
    } catch (e) {
      if (token !== loadToken) return;
      historyEntries = [];
      historyError = String(e);
    } finally {
      if (token === loadToken) historyLoading = false;
    }
  }

  function backToLive() {
    loadToken++;
    selectedFile = '';
    historyEntries = [];
    historyError = null;
    historyLoading = false;
    backendResults = null;
    autoScroll = true;
  }

  // Large saved runs: let the backend do the search (it reads the file itself).
  $effect(() => {
    if (!useBackendSearch) {
      backendResults = null;
      backendError = null;
      return;
    }
    const args = { serverId: serverId ?? '', name: selectedFile, query, regex: useRegex, caseSensitive, max: RENDER_CAP };
    const token = ++searchToken;
    invoke<LogMatch[]>('search_server_logs', args)
      .then((res) => {
        if (token !== searchToken) return;
        backendResults = res.map((m) => ({ seq: m.line, text: m.text }));
        backendError = null;
      })
      .catch((e) => {
        if (token !== searchToken) return;
        backendResults = [];
        backendError = String(e);
      });
  });

  // ---- Derived view ----
  let matcher = $derived(buildMatcher(query, { regex: useRegex, caseSensitive }));
  let queryError = $derived(useBackendSearch ? backendError : matcher.error);

  let rows = $derived.by<LogRow[]>(() => {
    if (!uiLevel.advanced) return filterLines(logs, { re: null, level: 'all', onlyMatching: false });
    if (useBackendSearch) {
      // Backend already returned only matching lines.
      return filterLines(backendResults ?? [], { re: null, level, onlyMatching: false }).map((r) => ({
        ...r,
        hit: true,
      }));
    }
    const source = isHistory ? historyEntries : logs;
    return filterLines(source, { re: matcher.re, level, onlyMatching });
  });

  let truncated = $derived(rows.length > RENDER_CAP);
  let shownRows = $derived(truncated ? rows.slice(rows.length - RENDER_CAP) : rows);
  let matchSeqs = $derived(query ? shownRows.filter((r) => r.hit).map((r) => r.seq) : []);
  let currentSeq = $derived(currentMatch >= 0 ? (matchSeqs[currentMatch] ?? null) : null);
  let highlightRe = $derived(!uiLevel.advanced ? null : useBackendSearch ? buildMatcher(query, { regex: useRegex, caseSensitive }).re : matcher.re);
  let totalLines = $derived(isHistory ? historyEntries.length : logs.length);

  // A new query / filter resets the "current match" cursor.
  $effect(() => {
    void query;
    void level;
    void onlyMatching;
    void caseSensitive;
    void useRegex;
    currentMatch = -1;
  });

  $effect(() => {
    // Track the number of lines so this runs whenever new logs arrive.
    const count = shownRows.length;
    if ((!uiLevel.advanced || (autoScroll && !isHistory)) && logContainer && count > 0) {
      requestAnimationFrame(() => {
        if (logContainer) logContainer.scrollTop = logContainer.scrollHeight;
      });
    }
  });

  function jump(delta: 1 | -1) {
    const n = matchSeqs.length;
    if (n === 0) return;
    autoScroll = false; // otherwise the next poll would scroll away from the match
    currentMatch = currentMatch < 0 ? (delta === 1 ? 0 : n - 1) : (currentMatch + delta + n) % n;
    const seq = matchSeqs[currentMatch];
    requestAnimationFrame(() => {
      logContainer
        ?.querySelector<HTMLElement>(`[data-seq="${seq}"]`)
        ?.scrollIntoView({ block: 'center' });
    });
  }

  function onSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      queryInput = '';
      query = '';
    } else if (e.key === 'Enter') {
      e.preventDefault();
      jump(e.shiftKey ? -1 : 1);
    }
  }

  // Ctrl+F while focus is inside the console focuses the search box.
  function onWindowKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'f') {
      const target = e.target as Node | null;
      if (cardEl && target && cardEl.contains(target)) {
        e.preventDefault();
        searchInput?.focus();
        (searchInput as HTMLInputElement | null)?.select();
      }
    }
  }

  // ---- Copy ----
  async function copyVisible() {
    try {
      const tag = (r: LogRow) => (tagged && r.server ? `[${app.serverLabel(r.server)}] ` : '');
      await navigator.clipboard.writeText(shownRows.map((r) => tag(r) + r.text).join('\n'));
      toast.success(t('dash.lc_copied', { count: shownRows.length }));
    } catch {
      toast.error(t('dash.lc_copy_failed'));
    }
  }

  // ---- Export ----
  let exportOpen = $state(false);
  let exportSource = $state<'live' | 'run'>('live');
  let exportSanitize = $state(true);
  let exporting = $state(false);
  let exportMsg = $state<{ ok: boolean; text: string } | null>(null);

  function onExportOpenChange(open: boolean) {
    if (open) {
      exportSource = isHistory ? 'run' : 'live';
      exportMsg = null;
    }
  }

  async function doExport() {
    exporting = true;
    exportMsg = null;
    try {
      if (!serverId) return;
      const name = exportSource === 'run' && isHistory ? selectedFile : null;
      const path = await invoke<string | null>('export_server_logs', {
        serverId,
        name,
        sanitize: exportSanitize,
      });
      exportMsg = path
        ? { ok: true, text: t('dash.lc_export_saved', { path }) }
        : { ok: true, text: t('dash.lc_export_cancelled') };
    } catch (e) {
      exportMsg = { ok: false, text: t('dash.lc_export_failed', { error: String(e) }) };
    } finally {
      exporting = false;
    }
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

{#snippet iconBtn(label: string, onclick: () => void, icon: Snippet, disabled = false)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="ghost" size="icon-xs" {onclick} {disabled} aria-label={label}>{@render icon()}</Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}
{#snippet upIcon()}<ArrowUp />{/snippet}
{#snippet downIcon()}<ArrowDown />{/snippet}
{#snippet copyIcon()}<Copy />{/snippet}

<section class="flex h-full min-h-0 flex-col" bind:this={cardEl} aria-label={t('dash.lc_title')}>
  <div class="flex shrink-0 flex-col gap-2 border-b border-border px-4 py-2.5">
    <div class="flex flex-wrap items-center gap-2">
      <h2 class="text-sm font-semibold text-foreground" title={t('dash.lc_title_tip')}>
        {t('dash.n_logs_title')}
      </h2>
      {#if isHistory && selectedInfo}
        <span class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground" role="status">
          <Clock class="size-3 shrink-0" />
          <span class="break-words">{t('dash.lc_history_chip', { date: fmtDate(selectedInfo.modified) })}</span>
          <Button variant="link" size="xs" class="h-auto px-0 text-xs" onclick={backToLive}>{t('dash.lc_back_to_live')}</Button>
        </span>
      {/if}
      <AdvancedOnly>
        <div class="ml-auto flex items-center gap-3">
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <label {...props} class="flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground">
                  <FieldHelp text={t('help.log_scroll')}><Switch size="sm" bind:checked={autoScroll} disabled={isHistory} aria-label={t('dash.lc_autoscroll')} /></FieldHelp>
                  {t('dash.lc_autoscroll')}
                </label>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-64">{t('dash.lc_autoscroll_tip')}</Tooltip.Content>
          </Tooltip.Root>
          {#if !isHistory}
            <Tooltip.Root>
              <Tooltip.Trigger>
                {#snippet child({ props })}
                  <Button {...props} variant="ghost" size="xs" onclick={onClear}><Trash2 />{t('dash.lc_clear_btn')}</Button>
                {/snippet}
              </Tooltip.Trigger>
              <Tooltip.Content class="max-w-64">{t('dash.lc_clear_tip')}</Tooltip.Content>
            </Tooltip.Root>
          {/if}
        </div>
      </AdvancedOnly>
    </div>

    <AdvancedOnly>
      {#if sourceIds.length > 1}
        <ServerChips
          filter={logFilter}
          ids={sourceIds}
          label={t('dash.lc_servers_label')}
          hint={(id) => (app.activeServers.some((s) => s.id === id) ? null : t('dash.lc_server_stopped'))}
        />
      {/if}

      <div class="flex flex-wrap items-center gap-1.5">
        <div class="relative min-w-[min(100%,16rem)] flex-1">
          <Search class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <label class="sr-only" for="{uid}-search">{t('dash.lc_search_label')}</label>
          <Input
            id="{uid}-search"
            type="search"
            class="h-7 pl-7 text-xs"
            placeholder={t('dash.lc_search_placeholder')}
            bind:value={queryInput}
            bind:ref={searchInput}
            onkeydown={onSearchKeydown}
            aria-invalid={queryError ? 'true' : undefined}
            aria-describedby="{uid}-status"
          />
        </div>
        {@render iconBtn(t('dash.lc_prev_match'), () => jump(-1), upIcon, matchSeqs.length === 0)}
        {@render iconBtn(t('dash.lc_next_match'), () => jump(1), downIcon, matchSeqs.length === 0)}
        <span
          id="{uid}-status"
          class={cn('min-w-0 font-num text-xs text-muted-foreground', queryError && 'text-destructive')}
          aria-live="polite"
        >
          {#if queryError}
            {t('dash.lc_invalid_query', { error: queryError })}
          {:else if query}
            {currentMatch >= 0
              ? t('dash.lc_match_pos', { pos: currentMatch + 1, count: matchSeqs.length })
              : t('dash.lc_match_count', { count: matchSeqs.length })}
          {:else if level !== 'all'}
            {t('dash.lc_lines_shown', { shown: rows.length, total: totalLines })}
          {/if}
        </span>

        <FieldHelp text={t('help.log_level')}><ToggleGroup.Root
          type="single"
          variant="outline"
          size="sm"
          value={level}
          onValueChange={(v) => v && (level = v as LevelFilter)}
          aria-label={t('dash.lc_level_legend')}
          class="ml-auto h-7"
        >
          {#each levelOptions as opt (opt.value)}
            <ToggleGroup.Item value={opt.value} class="h-7 px-2 text-xs data-[state=on]:bg-primary/15 data-[state=on]:text-primary">
              {opt.label}
            </ToggleGroup.Item>
          {/each}
        </ToggleGroup.Root></FieldHelp>

        <Popover.Root>
          <Popover.Trigger>
            {#snippet child({ props })}
              <Button {...props} variant="outline" size="icon-sm" aria-label={t('dash.n_search_options')} title={t('dash.n_search_options')}>
                <SlidersHorizontal />
              </Button>
            {/snippet}
          </Popover.Trigger>
          <Popover.Content align="end" class="w-64">
            <label class="flex cursor-pointer items-center gap-2 text-xs">
              <FieldHelp text={t('help.case')}><Checkbox bind:checked={caseSensitive} /></FieldHelp>{t('dash.lc_case_sensitive')}
            </label>
            <label class="flex cursor-pointer items-center gap-2 text-xs">
              <FieldHelp text={t('help.regex')}><Checkbox bind:checked={useRegex} /></FieldHelp>{t('dash.lc_regex')}
            </label>
            <label class="flex cursor-pointer items-center gap-2 text-xs" title={t('dash.lc_only_matching_tip')}>
              <FieldHelp text={t('dash.lc_only_matching_tip')}><Checkbox bind:checked={onlyMatching} disabled={useBackendSearch} /></FieldHelp>{t('dash.lc_only_matching')}
            </label>
          </Popover.Content>
        </Popover.Root>

        <NativeSelect
          class="h-7 w-auto max-w-40 text-xs"
          aria-label={t('dash.lc_history_label')}
          title={serverId ? undefined : t('dash.lc_pick_one_server')}
          disabled={!serverId}
          value={selectedFile}
          onfocus={refreshFiles}
          onchange={(e) => openFile(e.currentTarget.value)}
        >
          <option value="">{t('dash.lc_history_live')}</option>
          {#each files as f (f.name)}
            <option value={f.name}>{fmtDate(f.modified)} · {fmtSize(f.size_bytes)}</option>
          {/each}
        </NativeSelect>

        {@render iconBtn(t('dash.lc_copy_label'), copyVisible, copyIcon, shownRows.length === 0)}

        <Popover.Root bind:open={exportOpen} onOpenChange={onExportOpenChange}>
          <Popover.Trigger>
            {#snippet child({ props })}
              <Button {...props} variant="ghost" size="icon-xs" disabled={!serverId} aria-label={t('dash.lc_export_btn')} title={serverId ? t('dash.lc_export_btn') : t('dash.lc_pick_one_server')}>
                <Download />
              </Button>
            {/snippet}
          </Popover.Trigger>
          <Popover.Content align="end" class="w-72">
            <fieldset class="flex flex-col gap-1.5">
              <legend class="mb-1 text-xs font-medium">{t('dash.lc_export_title')}</legend>
              <label class="flex cursor-pointer items-center gap-2 text-xs">
                <FieldHelp text={t('help.live_log')}><input type="radio" class="accent-primary" name="{uid}-export-src" value="live" bind:group={exportSource} /></FieldHelp>
                {t('dash.lc_export_live')}
              </label>
              {#if isHistory && selectedInfo}
                <label class="flex cursor-pointer items-center gap-2 text-xs">
                  <FieldHelp text={t('help.run_log')}><input type="radio" class="accent-primary" name="{uid}-export-src" value="run" bind:group={exportSource} /></FieldHelp>
                  {t('dash.lc_export_run', { date: fmtDate(selectedInfo.modified) })}
                </label>
              {/if}
            </fieldset>
            <label class="flex cursor-pointer items-center gap-2 text-xs">
              <FieldHelp text={t('help.sanitize')}><Checkbox bind:checked={exportSanitize} /></FieldHelp>{t('dash.lc_export_sanitize')}
            </label>
            <Button size="sm" variant="outline" onclick={doExport} disabled={exporting}>
              {exporting ? t('dash.lc_export_saving') : t('dash.lc_export_save')}
            </Button>
            <div class={cn('min-h-4 text-xs break-all', exportMsg && !exportMsg.ok ? 'text-destructive' : 'text-muted-foreground')} aria-live="polite">
              {exportMsg?.text ?? ''}
            </div>
          </Popover.Content>
        </Popover.Root>
      </div>

      {#if useBackendSearch}
        <p class="text-xs text-muted-foreground">{t('dash.lc_backend_search_note')}</p>
      {/if}
      {#if truncated}
        <p class="text-xs text-muted-foreground">{t('dash.lc_truncated', { shown: RENDER_CAP, total: rows.length })}</p>
      {/if}
    </AdvancedOnly>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="min-h-0 flex-1 overflow-auto bg-terminal px-4 py-2 font-mono text-xs leading-5 text-terminal-foreground"
    bind:this={logContainer}
    tabindex="0"
    role="region"
    aria-label={t('dash.lc_region_label')}
  >
    {#if historyLoading}
      <div class="flex justify-center py-6 font-sans text-terminal-foreground/80">
        <SectionLoading shape="inline" label={t('dash.lc_history_loading')} phases={[t('sl.logs_1'), t('sl.logs_2')]} />
      </div>
    {:else if historyError}
      <div class="py-6 text-center font-sans text-xs text-log-error">{t('dash.lc_history_error', { error: historyError })}</div>
    {:else if totalLines === 0 && !useBackendSearch}
      <div class="py-6 text-center font-sans text-xs text-terminal-foreground/80">
        {isHistory ? t('dash.lc_history_empty') : t('dash.n_logs_empty')}
      </div>
    {:else if shownRows.length === 0}
      <div class="py-6 text-center font-sans text-xs text-terminal-foreground/80">{t('dash.lc_no_matches')}</div>
    {:else}
      {#each shownRows as row (row.seq)}
        <div
          class={cn(
            'break-words whitespace-pre-wrap',
            row.text.includes('[stderr]') && 'text-terminal-foreground/70',
            row.text.includes('[Harness Studio]') && 'text-log-info',
            row.level === 'warn' && 'text-log-warn',
            row.level === 'error' && 'text-log-error',
            row.seq === currentSeq && 'bg-primary/20'
          )}
          data-seq={row.seq}
        >
          <!-- Shape cue next to the colour: triangle = warning, octagon = error. -->
          {#if row.level === 'error'}<OctagonX class="mr-1 inline size-3 align-[-2px]" aria-hidden="true" />{:else if row.level === 'warn'}<TriangleAlert class="mr-1 inline size-3 align-[-2px]" aria-hidden="true" />{/if}
          {#if tagged && row.server}
            {@const slot = app.serverSlots[row.server] ?? 0}
            <span
              class="mr-1.5 inline-flex max-w-32 items-center gap-1 rounded-sm bg-terminal-foreground/10 px-1 align-baseline font-sans text-xs leading-4 text-terminal-foreground/80 select-none"
              title={app.serverLabel(row.server)}
            >
              <span class={cn('size-1.5 shrink-0 rounded-full', slotHatched(slot) && 'hatch')} style="background-color: {slotColor(slot)}" aria-hidden="true"></span>
              <span class="truncate">{app.serverLabel(row.server)}</span>
            </span>
          {/if}
          {#if row.hit && highlightRe}
            {#each splitHighlight(row.text, highlightRe) as part, i (i)}
              {#if part.match}<mark class="rounded-sm bg-warning/30 text-inherit">{part.text}</mark>{:else}{part.text}{/if}
            {/each}
          {:else}
            {row.text}
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</section>
