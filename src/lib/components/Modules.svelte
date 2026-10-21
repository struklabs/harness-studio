<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Search from '@lucide/svelte/icons/search';
  import LayoutGrid from '@lucide/svelte/icons/layout-grid';
  import List from '@lucide/svelte/icons/list';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import { invoke } from '#lib/ipc.js';
  import { t, getLanguage, type TKey } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import type { ModuleView, LocaleKeys } from '#lib/modules.js';
  import {
    DEFAULT_FILTERS, activeFilterCount, facets, filterAndSort, navigate, rowWindow, toEntry, updatable,
    type CatalogFilters, type SortKey,
  } from '#lib/moduleCatalog.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Skeleton } from '#lib/components/ui/skeleton/index.js';
  import ModuleCard from './modules/ModuleCard.svelte';
  import ModuleDetail from './modules/ModuleDetail.svelte';
  import { categoryLabel, capabilityLabel, platformLabel } from './modules/labels.js';

  const text = (keys: LocaleKeys) => t(keys[getLanguage()] as TKey);
  const store = (key: string, value?: string): string | null => {
    try { if (value !== undefined) localStorage.setItem(key, value); return localStorage.getItem(key); } catch { return null; }
  };

  let modules = $state<ModuleView[]>([]);
  let fixtureTotal = $state(0);
  let simulated = $derived(fixtureTotal > 0);
  let selectedId = $state<string | null>(app.requestedModule ?? null);
  let detailOpen = $state(app.requestedModule !== null);
  app.requestedModule = null;
  let error = $state('');
  let notice = $state('');
  let pending = $state(false);
  let bulk = $state<{ done: number; total: number } | null>(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let reading = false;
  let disposed = false;

  let filters = $state<CatalogFilters>({ ...DEFAULT_FILTERS });
  let typed = $state('');
  let sort = $state<SortKey>((['recommended', 'name', 'updated', 'popular'] as const).find(s => s === store('hs_modules_sort')) ?? 'recommended');
  let layout = $state<'grid' | 'list'>(store('hs_modules_view') === 'list' ? 'list' : 'grid');
  $effect(() => { store('hs_modules_sort', sort); store('hs_modules_view', layout); });
  let debounce: ReturnType<typeof setTimeout> | undefined;
  function onType() {
    clearTimeout(debounce);
    debounce = setTimeout(() => { filters.query = typed; }, 80);
  }

  let advanced = $derived(uiLevel.advanced);
  let busy = $derived(pending || bulk !== null || modules.some(m => m.state.status === 'installing' || m.progress));
  let entries = $derived(modules.map(m => toEntry(m, text)));
  let results = $derived(filterAndSort(entries, filters, sort));
  let counts = $derived(facets(entries, filters));
  let pendingUpdates = $derived(updatable(entries));
  let filterCount = $derived(activeFilterCount(filters));
  let categoryIds = $derived([...new Set(entries.map(e => e.category))].sort((a, b) => categoryLabel(a).localeCompare(categoryLabel(b))));
  let capabilityIds = $derived([...new Set(entries.flatMap(e => e.capabilities))].sort());
  let platformIds = $derived([...new Set(entries.flatMap(e => e.view.manifest.platforms))].sort());
  let selected = $derived(entries.find(e => e.id === selectedId) ?? null);

  // Windowing: fixed row heights keep 500+ entries cheap; only visible rows are mounted.
  const metrics = $derived(layout === 'grid' ? { row: 176, gap: 12, min: 290 } : { row: 80, gap: 8, min: 100000 });
  let scroller = $state<HTMLDivElement>();
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);
  let columns = $derived(Math.max(1, Math.floor((width + metrics.gap) / (metrics.min + metrics.gap))));
  let rowCount = $derived(Math.ceil(results.length / columns));
  let win = $derived(rowWindow(rowCount, metrics.row + metrics.gap, scrollTop, height));
  let visible = $derived(results.slice(win.first * columns, (win.last + 1) * columns));
  let startIndex = $derived(win.first * columns);
  $effect(() => { void [filters.query, filters.status, filters.enabled, filters.origin, filters.category, filters.capability, filters.platform, filters.compatibleOnly, sort]; if (scroller) { scroller.scrollTop = 0; scrollTop = 0; } });

  // Roving focus: one card is in the tab order, the arrow keys move it.
  let activeId = $state<string | null>(null);
  let activeIndex = $derived(Math.max(0, results.findIndex(e => e.id === activeId)));

  async function focusIndex(index: number) {
    const target = results[index];
    if (!target || !scroller) return;
    activeId = target.id;
    const pitch = metrics.row + metrics.gap;
    const top = Math.floor(index / columns) * pitch;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + metrics.row > scroller.scrollTop + height) scroller.scrollTop = top + metrics.row - height;
    scrollTop = scroller.scrollTop;
    await tick();
    scroller.querySelector<HTMLElement>(`[data-module-id="${CSS.escape(target.id)}"] [data-card-button]`)?.focus();
  }
  function onListKey(event: KeyboardEvent) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const card = (event.target as HTMLElement).closest<HTMLElement>('[data-module-id]');
    if (!card) return;
    const index = results.findIndex(e => e.id === card.dataset.moduleId);
    const page = Math.max(1, Math.floor(height / (metrics.row + metrics.gap)));
    const next = navigate(index, event.key, results.length, columns, page);
    if (next < 0) return;
    event.preventDefault();
    void focusIndex(next);
  }

  let searchBox = $state<HTMLInputElement | null>(null);
  function onWindowKey(event: KeyboardEvent) {
    if (event.key !== '/' || event.ctrlKey || event.metaKey || event.altKey || event.defaultPrevented) return;
    const el = event.target as HTMLElement | null;
    if (el && (el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName))) return;
    if (document.querySelector('[role="dialog"]')) return;
    event.preventDefault();
    searchBox?.focus();
    searchBox?.select();
  }
  function onSearchKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      if (typed) { typed = ''; filters.query = ''; } else searchBox?.blur();
    } else if (event.key === 'ArrowDown' && results.length) {
      event.preventDefault();
      void focusIndex(activeIndex);
    }
  }
  function clearFilters() { typed = ''; filters = { ...DEFAULT_FILTERS }; }

  async function refresh() {
    if (reading || simulated) return;
    reading = true;
    try {
      const result = await invoke<ModuleView[]>('list_modules');
      if (!disposed) { modules = result; loadFailed = false; }
    } catch (e) { if (!disposed) { error = errorText(e); loadFailed = true; } }
    finally { reading = false; if (!disposed) loading = false; }
  }
  onMount(() => {
    disposed = false;
    void (async () => {
      // Dev only: `?modules_fixture=500` (or localStorage `hs_modules_fixture`) swaps in synthetic modules.
      if (import.meta.env.DEV) {
        const fixtures = await import('#lib/dev/moduleFixtures.js');
        const count = fixtures.fixtureCount(location.search, store('hs_modules_fixture'));
        if (count > 0 && !disposed) {
          modules = fixtures.generateModules(count);
          fixtureTotal = count;
          loading = false;
          return;
        }
      }
      await refresh();
    })();
    const timer = setInterval(() => void refresh(), 1500);
    // The first list answers at once with what is known; sizes and the agent probe finish in
    // the background, so ask again shortly until nothing is measuring.
    const soon = setInterval(() => { if (modules.some(m => m.measuring)) void refresh(); }, 400);
    return () => { disposed = true; clearInterval(timer); clearInterval(soon); clearTimeout(debounce); };
  });

  /** Fixture mode: apply the command to the in-memory module instead of calling the backend. */
  function simulate(command: string, id: string) {
    modules = modules.map(m => {
      if (m.manifest.id !== id) return m;
      const state = { ...m.state };
      let update = m.update_available;
      if (command === 'install_module') { state.status = 'installed'; state.installed_version = m.manifest.version; state.error = null; }
      else if (command === 'enable') state.status = 'installed';
      else if (command === 'disable') state.status = 'disabled';
      else if (command === 'update') { state.installed_version = m.manifest.version; update = false; }
      else if (command === 'remove_module_data') { state.status = 'not_installed'; state.installed_version = null; update = false; }
      return { ...m, state, update_available: update };
    });
  }
  async function run(command: string, id: string, options?: { delete_history: boolean; delete_logs: boolean }) {
    if (simulated) { simulate(command, id); return; }
    if (command === 'remove_module_data') await invoke(command, { id, options });
    else if (['enable', 'disable', 'repair', 'update'].includes(command)) await invoke('module_action', { id, action: command });
    else await invoke(command, { id });
  }
  async function action(command: string, id: string, options?: { delete_history: boolean; delete_logs: boolean }) {
    pending = true;
    error = '';
    notice = '';
    let failure = '';
    try { await run(command, id, options); }
    catch (e) { failure = errorText(e); }
    finally { if (!disposed) { pending = false; await refresh(); if (failure) error = failure; } }
  }
  async function updateAll() {
    const targets = pendingUpdates;
    if (!targets.length) return;
    error = '';
    notice = '';
    bulk = { done: 0, total: targets.length };
    const failed: string[] = [];
    // One at a time: modules share components and the backend allows a single operation.
    for (const target of targets) {
      try { await run('update', target.id); } catch { failed.push(target.name); }
      if (disposed) return;
      bulk = { done: bulk.done + 1, total: targets.length };
    }
    bulk = null;
    await refresh();
    if (failed.length) error = t('modules.update_all_failed', { names: failed.join(', ') });
  }
  /** Offline install: pick an official .hsmod bundle; nothing is downloaded. */
  async function installFromFile() {
    pending = true;
    error = '';
    let failure = '';
    try {
      const path = await invoke<string | null>('pick_module_bundle_file', { label: t('modules.bundle_filter') });
      if (path) {
        const report = await invoke<{ id: string }>('import_module_bundle', { path });
        selectedId = report.id;
        detailOpen = true;
      }
    } catch (e) { failure = errorText(e); }
    finally { if (!disposed) { pending = false; await refresh(); if (failure) error = failure; } }
  }
  function openModule(id: string) {
    if (id === 'agents') return;
    app.requestedPlaygroundTab = 'transcription';
    app.requestedTab = 'playground';
  }
  const names = (id: string) => entries.find(e => e.id === id)?.name ?? id;
  function openDetails(id: string) { selectedId = id; detailOpen = true; }

  const chip = 'inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-xs transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50 aria-pressed:border-primary aria-pressed:bg-primary/10 aria-pressed:font-medium';
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="flex h-full min-h-0 flex-col gap-4 p-4 md:p-6">
  <div class="flex flex-wrap items-start justify-between gap-4">
    <div class="min-w-0">
      <h1 class="text-xl font-semibold">{t('modules.title')}</h1>
      <p class="mt-1 text-sm text-muted-foreground">{t('modules.intro')}</p>
      <p class="mt-1 text-xs text-muted-foreground">{t('discover.registry_scope')}</p>
    </div>
    <div class="flex flex-wrap justify-end gap-2">
      {#if pendingUpdates.length}
        <Button disabled={busy} title={t('modules.update_all_hint')} onclick={() => void updateAll()}>{t('modules.update_all', { count: pendingUpdates.length })}</Button>
      {/if}
      {#if advanced && !simulated}
        <Button variant="outline" disabled={busy} title={t('modules.install_from_file_hint')} onclick={() => void installFromFile()}>{t('modules.install_from_file')}</Button>
      {/if}
      <Button variant="outline" onclick={() => { error = ''; void refresh(); }}><RefreshCw class="size-4" />{t('modules.refresh')}</Button>
    </div>
  </div>

  {#if simulated}<p role="note" class="rounded-md border border-dashed p-2 text-xs text-muted-foreground">{t('modules.fixture_banner', { count: fixtureTotal })}</p>{/if}
  {#if error && (modules.length > 0 || !loadFailed)}<p role="alert" class="rounded-md border border-destructive p-3 text-sm text-destructive">{error}</p>{/if}
  <div class="sr-only" role="status" aria-live="polite">
    {#if bulk}{t('modules.updating', { done: bulk.done, total: bulk.total })}{:else if pending}{t('modules.working')}{:else if !loading}{t('modules.results', { count: results.length, total: entries.length })}{/if}
  </div>
  {#if bulk}<p class="text-sm text-muted-foreground" aria-hidden="true">{t('modules.updating', { done: bulk.done, total: bulk.total })}</p>{:else if pending}<p class="text-sm text-muted-foreground" aria-hidden="true">{t('modules.working')}</p>{/if}

  <div class="grid min-h-0 flex-1 gap-5 lg:grid-cols-[14rem_minmax(0,1fr)]">
    <aside class="min-h-0 space-y-4 overflow-y-auto lg:pr-1" aria-label={t('modules.filters')}>
      <section>
        <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('modules.filter_status')}</h2>
        <div class="flex flex-wrap gap-1.5" role="group" aria-label={t('modules.filter_status')}>
          {#each [['all', 'modules.filter_all'], ['installed', 'modules.filter_installed'], ['available', 'modules.filter_available'], ['updates', 'modules.filter_updates']] as const as [value, label]}
            <button type="button" class={chip} aria-pressed={filters.status === value} onclick={() => filters.status = value}>{t(label)} <span class="text-muted-foreground">{counts.status[value]}</span></button>
          {/each}
        </div>
      </section>
      <section>
        <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('modules.filter_origin')}</h2>
        <div class="flex flex-wrap gap-1.5" role="group" aria-label={t('modules.filter_origin')}>
          {#each [['all', 'modules.filter_all'], ['official', 'modules.badge_official'], ['community', 'modules.badge_community']] as const as [value, label]}
            <button type="button" class={chip} aria-pressed={filters.origin === value} onclick={() => filters.origin = value}>{t(label)} <span class="text-muted-foreground">{counts.origin[value]}</span></button>
          {/each}
        </div>
      </section>
      <section>
        <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('modules.filter_category')}</h2>
        <div class="flex flex-wrap gap-1.5" role="group" aria-label={t('modules.filter_category')}>
          <button type="button" class={chip} aria-pressed={filters.category === null} onclick={() => filters.category = null}>{t('modules.filter_all')}</button>
          {#each categoryIds as id}
            <button type="button" class={chip} aria-pressed={filters.category === id} onclick={() => filters.category = filters.category === id ? null : id}>{categoryLabel(id)} <span class="text-muted-foreground">{counts.categories.get(id) ?? 0}</span></button>
          {/each}
        </div>
      </section>
      {#if advanced}
        <section>
          <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('modules.filter_state')}</h2>
          <div class="flex flex-wrap gap-1.5" role="group" aria-label={t('modules.filter_state')}>
            {#each [['all', 'modules.filter_all'], ['enabled', 'modules.filter_enabled'], ['disabled', 'modules.disabled']] as const as [value, label]}
              <button type="button" class={chip} aria-pressed={filters.enabled === value} onclick={() => filters.enabled = value}>{t(label)}</button>
            {/each}
          </div>
        </section>
        {#if capabilityIds.length}
          <section>
            <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('modules.filter_capability')}</h2>
            <div class="flex flex-wrap gap-1.5" role="group" aria-label={t('modules.filter_capability')}>
              {#each capabilityIds as id}
                <button type="button" class={chip} aria-pressed={filters.capability === id} onclick={() => filters.capability = filters.capability === id ? null : id}>{capabilityLabel(id)} <span class="text-muted-foreground">{counts.capabilities.get(id) ?? 0}</span></button>
              {/each}
            </div>
          </section>
        {/if}
        <section>
          <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t('modules.filter_platform')}</h2>
          <div class="flex flex-wrap gap-1.5" role="group" aria-label={t('modules.filter_platform')}>
            {#each platformIds as id}
              <button type="button" class={chip} aria-pressed={filters.platform === id} onclick={() => filters.platform = filters.platform === id ? null : id}>{platformLabel(id)} <span class="text-muted-foreground">{counts.platforms.get(id) ?? 0}</span></button>
            {/each}
          </div>
          <label class="mt-3 flex items-center gap-2 text-xs"><input type="checkbox" bind:checked={filters.compatibleOnly} />{t('modules.compatible_only')}</label>
        </section>
      {/if}
      {#if filterCount > 0}<Button variant="ghost" size="sm" onclick={clearFilters}>{t('modules.clear_filters')}</Button>{/if}
    </aside>

    <div class="flex min-h-0 min-w-0 flex-col gap-3">
      <div class="flex flex-wrap items-center gap-2">
        <div class="relative min-w-52 flex-1">
          <Search class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
          <Input
            bind:ref={searchBox}
            type="search"
            class="pl-8"
            bind:value={typed}
            oninput={onType}
            onkeydown={onSearchKey}
            placeholder={t('modules.search_placeholder')}
            aria-label={t('modules.search_label')}
            aria-keyshortcuts="/"
            title={t('modules.search_shortcut')}
          />
        </div>
        <label class="flex items-center gap-2 text-sm">
          <span class="text-muted-foreground">{t('modules.sort')}</span>
          <select class="h-8 rounded-lg border bg-background px-2 text-sm" bind:value={sort}>
            <option value="recommended">{t('modules.sort_recommended')}</option>
            <option value="name">{t('modules.sort_name')}</option>
            <option value="updated">{t('modules.sort_updated')}</option>
            <option value="popular">{t('modules.sort_popular')}</option>
          </select>
        </label>
        <div class="flex" role="group" aria-label={t('modules.catalog_label')}>
          <Button variant={layout === 'grid' ? 'secondary' : 'outline'} size="icon" class="rounded-r-none" aria-pressed={layout === 'grid'} aria-label={t('modules.view_grid')} title={t('modules.view_grid')} onclick={() => layout = 'grid'}><LayoutGrid /></Button>
          <Button variant={layout === 'list' ? 'secondary' : 'outline'} size="icon" class="rounded-l-none" aria-pressed={layout === 'list'} aria-label={t('modules.view_list')} title={t('modules.view_list')} onclick={() => layout = 'list'}><List /></Button>
        </div>
      </div>
      {#if !loading}<p class="text-xs text-muted-foreground" aria-hidden="true">{t('modules.results', { count: results.length, total: entries.length })}</p>{/if}

      <div bind:this={scroller} bind:clientWidth={width} bind:clientHeight={height} onscroll={() => scrollTop = scroller?.scrollTop ?? 0} class="min-h-0 flex-1 overflow-y-auto">
        {#if loading}
          <div class="grid gap-3" style:grid-template-columns="repeat(auto-fill,minmax(min(100%,290px),1fr))" aria-busy="true" role="status" aria-label={t('modules.loading')}>
            {#each Array(6) as _}<Skeleton class="h-44 rounded-lg" />{/each}
          </div>
        {:else if loadFailed && modules.length === 0}
          <div role="alert" class="mx-auto mt-10 max-w-md space-y-3 rounded-lg border border-destructive p-6 text-center">
            <h2 class="font-semibold">{t('modules.error_title')}</h2>
            <p class="text-sm text-destructive">{error}</p>
            <Button onclick={() => { error = ''; loading = true; void refresh(); }}>{t('modules.load_retry')}</Button>
          </div>
        {:else if entries.length === 0}
          <div class="mx-auto mt-10 max-w-md space-y-2 text-center">
            <h2 class="font-semibold">{t('modules.empty_catalog_title')}</h2>
            <p class="text-sm text-muted-foreground">{t('modules.empty_catalog_hint')}</p>
          </div>
        {:else if results.length === 0}
          <div class="mx-auto mt-10 max-w-md space-y-3 text-center">
            <h2 class="font-semibold">{t('modules.empty_title')}</h2>
            <p class="text-sm text-muted-foreground">{t('modules.empty_hint')}</p>
            <Button variant="outline" onclick={clearFilters}>{t('modules.clear_filters')}</Button>
          </div>
        {:else}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            role="list"
            aria-label={t('modules.catalog_label')}
            class="grid"
            style:grid-template-columns="repeat({columns}, minmax(0, 1fr))"
            style:grid-auto-rows="{metrics.row}px"
            style:gap="{metrics.gap}px"
            style:padding-top="{win.before}px"
            style:padding-bottom="{win.after}px"
            onkeydown={onListKey}
          >
            {#each visible as entry, i (entry.id)}
              <ModuleCard
                {entry}
                {layout}
                tabbable={startIndex + i === activeIndex}
                position={startIndex + i + 1}
                total={results.length}
                {busy}
                onopen={() => openDetails(entry.id)}
                onaction={(command) => void action(command, entry.id)}
              />
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<ModuleDetail
  entry={selected}
  bind:open={detailOpen}
  {busy}
  {advanced}
  {simulated}
  {names}
  onaction={(command, id) => void action(command, id)}
  onremove={(id, options) => void action('remove_module_data', id, options)}
  onopenmodule={openModule}
/>
