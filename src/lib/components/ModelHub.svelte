<script lang="ts">
  import { uiLevel } from '$lib/stores/uiLevel.svelte';
  import { onMount, untrack } from 'svelte';
  import type { HfModelSearchResult, HfSearchOptions, HfSortKey, SystemHardwareInfo } from '$lib/types';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { downloads } from '$lib/stores/downloads.svelte';
  import { library } from '$lib/stores/library.svelte';
  import { app } from '$lib/stores/app.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Empty from '$lib/components/ui/empty';
  import SectionLoading from '$lib/components/common/SectionLoading.svelte';
  import { cn } from '$lib/utils';
  import { CircleAlert, X, SearchX, Compass, LoaderCircle, RotateCcw, Info } from '@lucide/svelte';
  import CustomDownloadModal from './hub/CustomDownloadModal.svelte';
  import DownloadQueuePanel from './hub/DownloadQueuePanel.svelte';
  import ConvertQueuePanel from './hub/ConvertQueuePanel.svelte';
  import FiltersBar from './hub/FiltersBar.svelte';
  import HubSearchBar from './hub/HubSearchBar.svelte';
  import RepoCard from './hub/RepoCard.svelte';
  import TaskPicker from './hub/TaskPicker.svelte';
  import VariantDrawer from './hub/VariantDrawer.svelte';
  import { parseSearchQuery } from './hub/searchParser';
  import { clearPageCache, fetchPage, loadPrefs, savePrefs } from './hub/hubData';
  import { whisperRuntime } from './hub/whisperRuntime.svelte';
  import { sdcppRuntime } from './hub/sdcppRuntime.svelte';
  import { taskInfo } from './hub/tasks';
  import { taskSupportStore } from './hub/taskSupport.svelte';

  interface Props {
    hardware?: SystemHardwareInfo | null;
  }

  let { hardware = null }: Props = $props();

  const PAGE_LIMIT = 30;

  const LIST_TITLES: Record<HfSortKey, TKey> = {
    trendingScore: 'hub.sort_trending',
    createdAt: 'hub.sort_newest',
    likes: 'hub.sort_likes',
    downloads: 'hub.sort_downloads',
    lastModified: 'hub.sort_updated',
  };

  // Remembered between visits (localStorage).
  const prefs = loadPrefs();
  let searchQuery = $state(prefs.query);
  let pipelineTag = $state(prefs.task);
  let licenseFilter = $state(prefs.license);
  let authorFilter = $state(prefs.author);
  let includeNonGguf = $state(prefs.includeNonGguf);
  let sortKey = $state<HfSortKey>(prefs.sort);
  let viewMode = $state<'grid' | 'list'>(prefs.viewMode);
  let selectedQuantFilter = $state(prefs.quant);
  let selectedSizeFilter = $state(prefs.size);
  let showFilters = $state(prefs.showFilters);

  // Client-side tag filter (only meaningful for the loaded results).
  let selectedTagFilter = $state('ALL');

  // List view
  let results = $state<HfModelSearchResult[]>([]);
  let nextCursor = $state<string | null>(null);
  let isSearching = $state(false);
  let loadingMore = $state(false);
  let searchError = $state<string | null>(null);
  let moreError = $state<string | null>(null);
  let smartSearchNotice = $state<string | null>(null);
  /** Query + filters the shown list was fetched with (for "load more"). */
  let listOptions: HfSearchOptions | null = null;
  /** Search text the shown cards were fetched with: the heading names it, not the half-typed field. */
  let shownQuery = $state(prefs.query);
  let searchToken = 0;

  let showCustomModal = $state(false);
  let selectedRepo = $state<HfModelSearchResult | null>(null);
  // A navigation requested from outside (a toast action) must not leave the modal drawer open.
  let lastCloseTick = app.overlayCloseTick;
  $effect(() => {
    const tick = app.overlayCloseTick;
    if (tick === lastCloseTick) return;
    lastCloseTick = tick;
    selectedRepo = null;
    showCustomModal = false;
  });
  let scroller = $state<HTMLElement | null>(null);

  $effect(() => {
    savePrefs({
      query: searchQuery,
      task: pipelineTag,
      license: licenseFilter,
      author: authorFilter,
      includeNonGguf,
      sort: sortKey,
      viewMode,
      quant: selectedQuantFilter,
      size: selectedSizeFilter,
      showFilters,
    });
  });

  let activeFiltersCount = $derived(
    [licenseFilter, authorFilter.trim(), includeNonGguf, selectedTagFilter !== 'ALL', selectedQuantFilter !== 'ALL', selectedSizeFilter !== 'ALL'].filter(Boolean)
      .length
  );

  let task = $derived(taskInfo(pipelineTag));
  let taskLevel = $derived(taskSupportStore.support(pipelineTag));
  let taskInfoRuntime = $derived(taskSupportStore.info(pipelineTag));
  onMount(() => void taskSupportStore.load());

  function currentFilters() {
    return {
      pipeline_tag: pipelineTag || null,
      license: licenseFilter.trim() || null,
      author: authorFilter.trim() || null,
      include_non_gguf: includeNonGguf || null,
    };
  }

  /** Server-side filters of the shown list, applied on commit (select change, Enter / blur in the text inputs) rather than on every keystroke. */
  let baseFilters = $state(currentFilters());

  // Local library for the "downloaded / running" badges; rescans when a download finishes.
  $effect(() => {
    void library.version;
    untrack(() => void library.refresh());
  });

  // Another view sent the user here with a search (the Speech tab's "find a whisper model").
  $effect(() => {
    const query = app.requestedHubSearch;
    if (query === null) return;
    untrack(() => {
      app.requestedHubSearch = null;
      searchQuery = query;
      pipelineTag = '';
      licenseFilter = '';
      authorFilter = '';
      includeNonGguf = false;
      void runList();
    });
  });

  // The Hub always shows the list (an empty query lists by sort + filters): fetch it on open (served from the cache when fresh).
  void runList();

  /** Fetch the first page of the list for the current query + filters + sort. */
  async function runList() {
    const my = ++searchToken;
    isSearching = true;
    searchError = null;
    moreError = null;
    smartSearchNotice = null;
    selectedTagFilter = 'ALL';

    const parsed = parseSearchQuery(searchQuery);
    if (parsed.detectedQuant) {
      selectedQuantFilter = parsed.detectedQuant;
      smartSearchNotice = parsed.detectedQuant;
    }
    baseFilters = currentFilters();
    const options: HfSearchOptions = {
      query: parsed.cleanedQuery || searchQuery.trim(),
      ...baseFilters,
      sort: sortKey,
      limit: PAGE_LIMIT,
    };
    listOptions = options;
    try {
      const page = await fetchPage(options);
      if (my !== searchToken) return;
      results = page.items;
      shownQuery = searchQuery;
      nextCursor = page.next_cursor;
      scroller?.scrollTo({ top: 0 });
    } catch (e) {
      if (my === searchToken) {
        results = [];
        shownQuery = searchQuery;
        nextCursor = null;
        searchError = String(e);
      }
    } finally {
      if (my === searchToken) isSearching = false;
    }
  }

  async function loadMore() {
    if (!listOptions || !nextCursor || loadingMore || isSearching) return;
    const my = searchToken;
    loadingMore = true;
    moreError = null;
    try {
      const page = await fetchPage({ ...listOptions, cursor: nextCursor });
      if (my !== searchToken) return;
      const seen = new Set(results.map((r) => r.id));
      results = [...results, ...page.items.filter((r) => !seen.has(r.id))];
      nextCursor = page.next_cursor;
    } catch (e) {
      if (my === searchToken) moreError = String(e);
    } finally {
      loadingMore = false;
    }
  }

  /** Query, filter or sort changed: the list follows it. */
  function onServerFiltersChange() {
    baseFilters = currentFilters();
    void runList();
  }

  function onTaskChange(next: string) {
    pipelineTag = next;
    onServerFiltersChange();
  }

  function clearVariantFilters() {
    selectedQuantFilter = 'ALL';
    selectedSizeFilter = 'ALL';
  }

  function resetServerFilters() {
    pipelineTag = '';
    licenseFilter = '';
    authorFilter = '';
    includeNonGguf = false;
    onServerFiltersChange();
  }

  /** Infinite scroll: load the next page when the sentinel comes near the bottom. */
  function sentinel(node: HTMLElement) {
    if (typeof IntersectionObserver === 'undefined') return;
    const io = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && void loadMore(), {
      root: scroller,
      rootMargin: '400px',
    });
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }

  let availableTags = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const r of results) {
      for (const tag of r.tags ?? []) {
        const lower = tag.toLowerCase();
        if (
          !lower.startsWith('license:') &&
          !lower.startsWith('arxiv:') &&
          !lower.startsWith('base_model:') &&
          !lower.startsWith('region:') &&
          lower !== 'gguf' &&
          lower.length > 2 &&
          lower.length < 24
        ) {
          counts.set(lower, (counts.get(lower) || 0) + 1);
        }
      }
    }
    return Array.from(counts.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, 8)
      .map((entry) => entry[0]);
  });

  let filteredRepos = $derived(
    selectedTagFilter === 'ALL'
      ? results
      : results.filter((repo) => repo.tags?.some((tag) => tag.toLowerCase() === selectedTagFilter.toLowerCase()))
  );

  /** The shown list has speech models whose whisper.cpp runtime is not installed yet. */
  let needsWhisper = $derived(
    results.some((r) => r.compatibility.status === 'needs_runtime' && r.compatibility.runtime === 'whisper.cpp')
  );

  async function installWhisper() {
    if (await whisperRuntime.install()) {
      clearPageCache();
      void runList();
    }
  }

  /** The shown list has image models whose stable-diffusion.cpp runtime is not installed yet. */
  let needsSdcpp = $derived(
    results.some((r) => r.compatibility.status === 'needs_runtime' && r.compatibility.runtime === 'stable-diffusion.cpp')
  );

  async function installSdcpp() {
    if (await sdcppRuntime.install()) {
      clearPageCache();
      void runList();
    }
  }

  let listTitle = $derived(shownQuery.trim() ? t('hub.list_title_search', { query: shownQuery.trim() }) : t(LIST_TITLES[sortKey]));
</script>

<div class="flex h-full min-h-0 flex-col gap-3 p-4">
  <HubSearchBar
    bind:query={searchQuery}
    bind:sort={sortKey}
    bind:viewMode
    bind:showFilters
    activeFilters={activeFiltersCount}
    searching={isSearching}
    onSearch={runList}
    onSortChange={() => void runList()}
    onCustomUrl={() => (showCustomModal = true)}
  />

  <TaskPicker value={pipelineTag} onchange={onTaskChange} />

  {#if !uiLevel.advanced && activeFiltersCount > 0}
    <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground" role="status">
      <span>{t('hub.hidden_filters_active', { count: activeFiltersCount })}</span>
      <Button variant="ghost" size="xs" disabled={uiLevel.busy} onclick={() => { showFilters = true; void uiLevel.set('advanced'); }}>{t('uiLevel.switch_advanced')}</Button>
    </div>
  {/if}

  {#if smartSearchNotice}
    <div class="flex items-center justify-between gap-2 rounded-lg border border-info/30 bg-info/10 px-3 py-1.5 text-xs text-info" role="status">
      <span>{t('hub.smart_search_hint', { quant: smartSearchNotice })}</span>
      <Button variant="ghost" size="icon-xs" aria-label={t('hub.close')} onclick={() => (smartSearchNotice = null)}><X /></Button>
    </div>
  {/if}

  {#if showFilters && uiLevel.advanced}
    <FiltersBar
      bind:pipelineTag
      bind:license={licenseFilter}
      bind:author={authorFilter}
      bind:includeNonGguf
      bind:tagFilter={selectedTagFilter}
      bind:quantFilter={selectedQuantFilter}
      bind:sizeFilter={selectedSizeFilter}
      {availableTags}
      {onServerFiltersChange}
    />
  {/if}

  {#if task && taskLevel && taskLevel !== 'yes'}
    <div
      class={cn(
        'flex items-start gap-2 rounded-lg border px-3 py-1.5 text-xs',
        taskLevel === 'no' && !taskInfoRuntime?.needs_runtime ? 'border-warning/30 bg-warning/10 text-warning' : 'border-info/30 bg-info/10 text-info'
      )}
      role="status"
    >
      <Info class="mt-px size-3.5 shrink-0" />
      <span>
        {#if taskInfoRuntime?.needs_runtime}
          {t('lib.runtime_needs_hint', { runtime: taskInfoRuntime.needs_runtime })}
        {:else if taskInfoRuntime?.runtime && taskLevel === 'partial'}
          {t('hub.task_alt_partial_banner', { task: t(task.key), runtime: taskInfoRuntime.runtime })}
        {:else if taskInfoRuntime?.runtime_missing}
          {t(uiLevel.advanced ? 'hub.task_runtime_missing_banner' : 'hub.task_runtime_missing_simple', { task: t(task.key), flag: taskInfoRuntime.flag ?? taskInfoRuntime.feature ?? '' })}
        {:else}
          {t(taskLevel === 'no' ? 'hub.task_unsupported_banner' : 'hub.task_partial_banner', { task: t(task.key) })}
        {/if}
      </span>
    </div>
  {/if}

  <div bind:this={scroller} class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto" aria-busy={isSearching}>
    <div class="flex flex-col gap-3 pb-2">
      <!-- "GGUF only" is not true once non-GGUF repos are listed too. -->
      {#if !includeNonGguf && uiLevel.advanced}
        <p class="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Compass class="size-3.5 shrink-0" />{t('hub.discover_scope')}
        </p>
      {/if}
      <div class="mb-2 flex min-w-0 items-center gap-2">
        <h2 class="min-w-0 truncate text-sm font-semibold" title={listTitle}>{listTitle}</h2>
        {#if !isSearching && !searchError && results.length > 0}
          <span class="shrink-0 font-mono text-xs text-muted-foreground tabular">
            {t(nextCursor ? 'hub.results_count_more' : 'hub.results_count', { count: results.length })}
          </span>
        {/if}
      </div>

      {#if searchError}
        <div class="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">
          <CircleAlert class="size-3.5 shrink-0" />
          <span class="min-w-0 flex-1 break-words">{searchError}</span>
          <Button variant="ghost" size="xs" class="shrink-0" onclick={runList}><RotateCcw />{t('hub.retry_button')}</Button>
        </div>
      {:else if isSearching}
        <SectionLoading shape="rows" rows={7} label={t('sl.hub_label')} phases={[t('sl.hub_1'), t('sl.hub_2')]} />
      {:else if results.length === 0}
        <Empty.Root class="h-full">
          <Empty.Header>
            <Empty.Media variant="icon"><SearchX /></Empty.Media>
            <Empty.Title>{t('hub.search_no_results')}</Empty.Title>
            <Empty.Description>{t(searchQuery.trim() ? 'hub.no_results_hint' : 'hub.filter_no_results_hint')}</Empty.Description>
          </Empty.Header>
          {#if pipelineTag || licenseFilter || authorFilter.trim() || includeNonGguf}
            <Empty.Content>
              <Button variant="outline" size="sm" onclick={resetServerFilters}><RotateCcw />{t('hub.reset_filters')}</Button>
            </Empty.Content>
          {/if}
        </Empty.Root>
      {:else if filteredRepos.length === 0}
        <Empty.Root class="h-full">
          <Empty.Header>
            <Empty.Title>{t('hub.no_tag_filter_results')}</Empty.Title>
          </Empty.Header>
          <Empty.Content>
            <Button variant="outline" size="sm" onclick={() => (selectedTagFilter = 'ALL')}>{t('hub.filter_all')}</Button>
          </Empty.Content>
        </Empty.Root>
      {:else}
        {#if needsSdcpp}
          <div class="mb-2 flex items-center gap-2 rounded-lg border border-info/30 bg-info/10 px-3 py-1.5 text-xs text-info" role="status">
            <Info class="size-3.5 shrink-0" />
            <span class="min-w-0 flex-1">{t('hub.compat_needs_sd_banner')}</span>
            <Button variant="outline" size="xs" class="shrink-0" disabled={sdcppRuntime.installing} onclick={installSdcpp}>
              {#if sdcppRuntime.installing}<LoaderCircle class="animate-spin" />{/if}
              {t(sdcppRuntime.installing ? 'hub.compat_installing' : 'hub.compat_install')}
            </Button>
          </div>
        {/if}
        {#if needsWhisper}
          <div class="mb-2 flex items-center gap-2 rounded-lg border border-info/30 bg-info/10 px-3 py-1.5 text-xs text-info" role="status">
            <Info class="size-3.5 shrink-0" />
            <span class="min-w-0 flex-1">{t('hub.compat_needs_whisper_banner')}</span>
            <Button variant="outline" size="xs" class="shrink-0" disabled={whisperRuntime.installing} onclick={installWhisper}>
              {#if whisperRuntime.installing}<LoaderCircle class="animate-spin" />{/if}
              {t(whisperRuntime.installing ? 'hub.compat_installing' : 'hub.compat_install')}
            </Button>
          </div>
        {/if}
        <div
          class={cn(
            'grid gap-2',
            viewMode === 'grid' ? 'grid-cols-[repeat(auto-fill,minmax(min(100%,280px),1fr))]' : 'grid-cols-1'
          )}
        >
          {#each filteredRepos as repo (repo.id)}
            <RepoCard
              {repo}
              layout={viewMode}
              selected={selectedRepo?.id === repo.id}
              quantFilter={selectedQuantFilter}
              onSelect={() => (selectedRepo = repo)}
            />
          {/each}
        </div>
        <div class="flex justify-center py-3">
          {#if moreError}
            <div class="flex items-center gap-2 text-xs text-destructive" role="alert">
              <CircleAlert class="size-3.5 shrink-0" /><span class="min-w-0 break-words">{moreError}</span>
              <Button variant="ghost" size="xs" onclick={loadMore}><RotateCcw />{t('hub.retry_button')}</Button>
            </div>
          {:else if nextCursor}
            {#key nextCursor}
              <div use:sentinel class="flex justify-center">
                <Button variant="outline" size="sm" disabled={loadingMore} onclick={loadMore}>
                  {#if loadingMore}<LoaderCircle class="animate-spin" />{/if}
                  {t(loadingMore ? 'hub.loading_more' : 'hub.load_more')}
                </Button>
              </div>
            {/key}
          {:else}
            <span class="text-xs text-muted-foreground">{t('hub.end_of_list')}</span>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <DownloadQueuePanel queue={downloads} />
  <ConvertQueuePanel />
</div>

<VariantDrawer
  repo={selectedRepo}
  {hardware}
  quantFilter={selectedQuantFilter}
  sizeFilter={selectedSizeFilter}
  onClose={() => (selectedRepo = null)}
  onClearFilters={clearVariantFilters}
/>

<CustomDownloadModal bind:open={showCustomModal} />
