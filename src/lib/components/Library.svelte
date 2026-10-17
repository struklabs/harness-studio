<script lang="ts">
  import AdvancedPageNotice from './common/AdvancedPageNotice.svelte';
  import { pageLevel } from '#lib/uiLevel.js';
  import SectionSidebar from './common/SectionSidebar.svelte';
  import { panes } from '#lib/stores/panes.svelte.js';
  import { SectionSelection } from '#lib/stores/sectionPage.svelte.js';
  import { LIBRARY_GROUPS } from './common/screenSections';
  import ImportModal from './ImportModal.svelte';
  const section = new SectionSelection('library', LIBRARY_GROUPS.flatMap(g => g.pages.map(p => p.id)), undefined, undefined, LIBRARY_GROUPS);
  let importVisited = $state(false);
  $effect(() => { if (section.active === 'import') importVisited = true; });
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { localizeError } from '#lib/network.js';
  // My Models (Kütüphanem): local model files by kind with their definitions, run state, user
  // state (favorite, last used, arrival) and file actions. Separate from the Model Hub.
  import { invoke, invokeShared } from '#lib/ipc.js';
  import { onMount, untrack } from 'svelte';
  import { toast } from 'svelte-sonner';
  import type { FitInput, LibraryEntry, LibraryView, ModelDefinition, StorageDirInfo } from '#lib/types.js';
  import { t, locale, type TKey } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as Empty from '#lib/components/ui/empty/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';
  import { filterRemoteModels, remoteGroupState, type RemoteModel } from './playground/remoteModels';
  import { security } from '#lib/stores/security.svelte.js';
  import { Search, Star, LayoutGrid, List, RefreshCw, Copy as CopyIcon, Library as LibraryIcon, Compass, CircleAlert } from '@lucide/svelte';
  import FilterSelect from './hub/FilterSelect.svelte';
  import { listAgentModels, type AgentModel } from './connections/agentModels';
  import ModelEntry from './library/ModelEntry.svelte';
  import { createCardState, FitCache, type CardState } from './library/cardState';
  import { hardwareFit } from '#lib/fit.js';
  import WindowedModels from './library/WindowedModels.svelte';
  import DeleteModelDialog from './library/DeleteModelDialog.svelte';
  import DuplicatesDialog from './library/DuplicatesDialog.svelte';
  import DefinitionDialog from './dashboard/DefinitionDialog.svelte';
  import MigrationModal from './dashboard/MigrationModal.svelte';
  import {
    TABS,
    tabCounts,
    inTab,
    matchesQuery,
    sortEntries,
    definitionsOf,
    suggestAlias,
    type LibraryTab,
    type LibrarySort,
  } from './library/filters';

  const PREFS_KEY = 'harness_library_prefs';

  let view = $state<LibraryView | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let remoteModels = $state<RemoteModel[]>([]);
  let remoteLinkCount = $state(0);
  let agentModels = $state<AgentModel[]>([]);
  let storageDirs = $state<StorageDirInfo[]>([]);

  let tab = $derived(TABS.includes(section.active as LibraryTab) ? section.active as LibraryTab : 'all');
  let query = $state('');
  let sort = $state<LibrarySort>('name');
  let favoritesOnly = $state(false);
  let layout = $state<'grid' | 'list'>('grid');
  let modelsViewport = $state<HTMLDivElement>();
  // State belongs to the library, not the currently mounted window of cards.
  const cardStates = $state<Record<string, CardState>>({});
  const fits = new FitCache();
  const openMenus = $state<Record<string, boolean>>({});
  let fitScanVersion = $state(0);
  let fitEnvironment = $derived(JSON.stringify([app.hardware, app.settings, locale(), library.version, fitScanVersion]));
  function cardState(id: string) {
    return cardStates[id] ??= createCardState();
  }
  function fitFor(entry: LibraryEntry, input: FitInput, environment: string) {
    // Include file metadata; each rescan also invalidates same-size replacements (no mtime in the scan).
    const key = JSON.stringify([environment, entry.model, input]);
    return fits.get(entry.model.path, key, () => hardwareFit(input));
  }

  let editDef = $state<ModelDefinition | null>(null);
  let deleteId = $state<string | null>(null);
  let moveEntry = $state<LibraryEntry | null>(null);
  let showDupes = $state(false);
  let dupesVersion = $state(0);

  onMount(() => {
    try {
      const p = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}');
      if (!localStorage.getItem('harness_library_page') && TABS.includes(p.tab)) {
        section.restore(p.tab);
        localStorage.setItem('harness_library_page', p.tab);
      }
      if (['name', 'size', 'last_used', 'added'].includes(p.sort)) sort = p.sort;
      if (p.layout === 'grid' || p.layout === 'list') layout = p.layout;
      favoritesOnly = !!p.favoritesOnly;
    } catch {
      // storage unavailable
    }
    void domain.refreshDefinitions();
    void domain.refreshPresets();
    void domain.refreshStates();
    void refresh();
  });

  $effect(() => {
    const prefs = JSON.stringify({ tab, sort, layout, favoritesOnly });
    try {
      localStorage.setItem(PREFS_KEY, prefs);
    } catch {
      // storage unavailable
    }
  });

  // A download finished / files changed elsewhere: rescan.
  let seenVersion = library.version;
  $effect(() => {
    const v = library.version;
    if (v !== seenVersion) {
      seenVersion = v;
      untrack(() => void refresh());
    }
  });

  // A definition made outside the window (CLI, Control API) can add a file; the pushed
  // definition list changes the path set, which rescans the library (outside-directory flags too).
  let seenDefinitionPaths: string | null = null;
  $effect(() => {
    if (!domain.definitionsLoaded) return;
    const key = domain.definitions.flatMap((d) => (d.base.kind === 'local' ? [d.base.path] : [])).sort().join('\n');
    const seen = seenDefinitionPaths;
    seenDefinitionPaths = key;
    if (seen !== null && seen !== key) untrack(() => void refresh());
  });

  // Run states follow the servers (polled once a second by the shell).
  let serversSig = $derived(app.servers.map((s) => `${s.id}:${JSON.stringify(s.status)}`).join('|'));
  $effect(() => {
    void serversSig;
    untrack(() => void domain.refreshStates());
  });

  // Remote group: read-only rows from the link cache, refreshed while the page is open.
  $effect(() => {
    let alive = true;
    async function loadRemote() {
      try {
        const [models, links] = await Promise.all([invoke<RemoteModel[]>('link_models'), invoke<unknown[]>('link_list')]);
        if (alive) { remoteModels = models; remoteLinkCount = links.length; }
      } catch {
        // links unavailable: the group stays hidden
      }
    }
    void loadRemote();
    const timer = setInterval(() => void loadRemote(), 10000);
    return () => { alive = false; clearInterval(timer); };
  });
  let remoteState = $derived(remoteGroupState(remoteLinkCount, app.gatewayRunning));
  let remoteRows = $derived(filterRemoteModels(remoteModels, query));
  function openRemote(model: RemoteModel) {
    app.requestedPlaygroundModel = model.id;
    app.openPlaygroundTab('chat');
  }

  let refreshToken = 0;
  async function refresh() {
    const token = ++refreshToken;
    loading = true;
    try {
      const [v, dirs, agents] = await Promise.all([
        invoke<LibraryView>('list_library'),
        invokeShared<StorageDirInfo[]>('get_storage_directories').catch(() => storageDirs),
        listAgentModels().catch(() => []),
      ]);
      if (token !== refreshToken) return;
      for (const entry of v.entries) cardState(entry.model.id);
      fitScanVersion++;
      view = v;
      agentModels = agents;
      storageDirs = dirs;
      loadError = null;
      void library.refresh();
    } catch (e) {
      if (token === refreshToken) loadError = String(e);
    } finally {
      if (token === refreshToken) loading = false;
    }
  }

  let entries = $derived(view?.entries ?? []);
  let counts = $derived(tabCounts(entries));
  let aliasesById = $derived(
    Object.fromEntries(entries.map((e) => [e.model.id, definitionsOf(e, domain.definitions).map((d) => d.alias)]))
  );
  let visible = $derived(
    sortEntries(
      entries.filter(
        (e) => (section.active !== 'definitions' || definitionsOf(e, domain.definitions).length > 0) && inTab(e, tab) && (!favoritesOnly || e.favorite) && matchesQuery(e, query, aliasesById[e.model.id] ?? [])
      ),
      sort
    )
  );
  let models = $derived(entries.map((e) => e.model));

  async function toggleFavorite(e: LibraryEntry) {
    const next = !e.favorite;
    try {
      await invoke('set_model_favorite', { modelId: e.model.id, favorite: next });
      e.favorite = next;
    } catch (err) {
      toast.error(t('lib.favorite_failed', { error: String(err) }));
    }
  }

  function newDefinition(e: LibraryEntry) {
    editDef = {
      id: '',
      alias: suggestAlias(e.model.name, domain.definitions),
      base: { kind: 'local', path: e.model.path },
      load: {},
      created_at: 0,
      updated_at: 0,
    };
  }

  function afterDelete(message: string) {
    deleteId = null;
    toast.success(message);
    dupesVersion++;
    library.markChanged();
    void domain.refreshDefinitions();
  }

  const SORTS: { value: LibrarySort; key: TKey }[] = [
    { value: 'name', key: 'lib.sort_name' },
    { value: 'size', key: 'lib.sort_size' },
    { value: 'last_used', key: 'lib.sort_last_used' },
    { value: 'added', key: 'lib.sort_added' },
  ];
</script>

<div class="flex h-full min-h-0 overflow-hidden">
  <SectionSidebar pane={panes.libraryNav} label={t('nav.library')} groups={LIBRARY_GROUPS} active={section.active} onSelect={section.select} />
  <main class="flex min-w-0 flex-1 flex-col gap-4 overflow-y-auto p-4 sm:p-6">
    <AdvancedPageNotice level={pageLevel(LIBRARY_GROUPS, section.active)} />
    <header><h1 class="text-lg font-semibold">{t(LIBRARY_GROUPS.flatMap(g => g.pages).find(p => p.id === section.active)!.label)}{#if TABS.includes(section.active as LibraryTab)} <span class="ml-1.5 font-num text-sm text-muted-foreground">{counts[tab]}</span>{/if}</h1></header>
    <div hidden={section.active !== 'import'} class="min-h-0 flex-1 overflow-y-auto">
      {#if importVisited}<ImportModal inline onClose={() => section.select('all')} onImported={refresh} />{/if}
    </div>
    <div hidden={section.active === 'import'} class="flex min-h-64 flex-1 flex-col gap-4">
  <!-- Toolbar -->
  <div class="flex shrink-0 flex-wrap items-center gap-2">
    <div class="relative min-w-0 max-w-3xl flex-[1_1_18rem]">
      <Search class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
      <FieldHelp text={t('help.library_search')} class="w-full"><Input type="search" class="h-8 pl-8" placeholder={t('lib.search_placeholder')} aria-label={t('hub.search_input_label')} bind:value={query} /></FieldHelp>
    </div>
    <div class="flex max-w-full flex-wrap items-center gap-2">
    <FilterSelect help={t('help.library_sort')}
      class="w-48 max-w-full"
      label={t('lib.sort_label')}
      showLabel={false}
      value={sort}
      options={SORTS.map((s) => ({ value: s.value, label: t(s.key) }))}
      onchange={(v) => (sort = v as LibrarySort)}
    />
    <FieldHelp text={t('help.favorites')}><Button
      variant="outline"
      size="sm"
      class={cn(favoritesOnly && 'border-border bg-muted text-foreground')}
      aria-pressed={favoritesOnly}
      onclick={() => (favoritesOnly = !favoritesOnly)}
    >
      <Star fill={favoritesOnly ? 'currentColor' : 'none'} />{t('lib.favorites_only')}
    </Button></FieldHelp>
    <Button variant="outline" size="sm" onclick={() => (showDupes = true)}><CopyIcon />{t('lib.find_dupes')}</Button>
    <Button variant="ghost" size="icon-sm" aria-label={t('lib.refresh')} title={t('lib.refresh')} onclick={refresh} disabled={loading}>
      <RefreshCw class={cn(loading && 'animate-spin')} />
    </Button>
    <FieldHelp text={t('help.layout')}><ToggleGroup.Root type="single" variant="outline" size="sm" value={layout} onValueChange={(v) => v && (layout = v as 'grid' | 'list')} aria-label={t('lib.view_label')}>
      <ToggleGroup.Item value="grid" aria-label={t('lib.view_grid')}><LayoutGrid /></ToggleGroup.Item>
      <ToggleGroup.Item value="list" aria-label={t('lib.view_list')}><List /></ToggleGroup.Item>
    </ToggleGroup.Root></FieldHelp>
    </div>
  </div>

  {#if view?.state_error}
    <div class="flex items-center gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-1.5 text-xs text-warning" role="status">
      <CircleAlert class="size-3.5 shrink-0" /><span class="min-w-0 break-words">{t('lib.state_error', { error: localizeError(view.state_error) })}</span>
    </div>
  {/if}

  <div hidden={section.active !== 'definitions'} class="min-h-0 flex-1 overflow-y-auto">
    {#each domain.definitions as definition (definition.id)}
      <div class="flex items-center justify-between gap-3 border-b border-border py-3">
        <div class="min-w-0"><p class="break-words font-mono text-sm [overflow-wrap:anywhere]">{definition.alias}</p><p class="break-words text-xs text-muted-foreground [overflow-wrap:anywhere]">{definition.base.kind === 'local' ? definition.base.path : definition.base.kind}</p></div>
        <Button variant="outline" size="sm" onclick={() => editDef = definition}>{t('lib.edit_definition')}</Button>
      </div>
    {:else}<p class="text-sm text-muted-foreground">{t('def.no_definition')}</p>{/each}
  </div>
  <div bind:this={modelsViewport} hidden={section.active === 'definitions'} class="min-h-40 flex-1 overflow-y-auto p-1 -m-1" aria-busy={loading}>
    {#if loadError}
      <div class="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">
        <CircleAlert class="size-3.5 shrink-0" /><span class="min-w-0 break-words">{t('lib.load_error', { error: loadError })}</span>
        <Button size="xs" variant="outline" class="ml-auto" onclick={refresh}>{t('lib.retry')}</Button>
      </div>
    {:else if view === null}
      <SectionLoading shape="rows" rows={6} label={t('sl.lib_label')} phases={[t('sl.lib_1'), t('sl.lib_2')]} />
    {:else if entries.length === 0}
      <Empty.Root class="min-h-48">
        <Empty.Header>
          <Empty.Media variant="icon"><LibraryIcon /></Empty.Media>
          <Empty.Title>{t('lib.empty_title')}</Empty.Title>
          <Empty.Description>{t('lib.empty_desc')}</Empty.Description>
        </Empty.Header>
        <Empty.Content>
          <Button size="sm" onclick={() => (app.requestedTab = 'hub')}><Compass />{t('lib.go_hub')}</Button>
        </Empty.Content>
      </Empty.Root>
    {:else if visible.length === 0}
      <Empty.Root class="min-h-48">
        <Empty.Header>
          <Empty.Title>{t('lib.no_match')}</Empty.Title>
          <Empty.Description>{t('lib.no_match_hint')}</Empty.Description>
        </Empty.Header>
        <Empty.Content>
          <FieldHelp text={t('help.favorites')}><Button
            variant="outline"
            size="sm"
            onclick={() => {
              section.select('all');
              query = '';
              favoritesOnly = false;
            }}>{t('lib.clear_filters')}</Button
          ></FieldHelp>
        </Empty.Content>
      </Empty.Root>
    {:else}
      <WindowedModels pinnedIds={Object.keys(openMenus).filter(id => openMenus[id])} entries={visible} {layout} viewport={modelsViewport} resetKey={query}>
        {#snippet children(entry)}
          <ModelEntry
            {entry}
            bind:state={cardStates[entry.model.id]}
            {fitFor}
            {fitEnvironment}
            onMenuChange={(open) => openMenus[entry.model.id] = open}
            {entries}
            {agentModels}
            {layout}
            definitions={definitionsOf(entry, domain.definitions)}
            onEditDefinition={(d) => (editDef = d)}
            onNewDefinition={newDefinition}
            onMove={(e) => (moveEntry = e)}
            onDelete={(e) => (deleteId = e.model.id)}
            onToggleFavorite={toggleFavorite}
            onChanged={() => void refresh()}
          />
        {/snippet}
      </WindowedModels>
    {/if}
  </div>
  {#if (tab === 'all' || tab === 'llm') && section.active !== 'definitions' && remoteState !== 'hidden'}
    <section class="max-h-48 shrink-0 overflow-y-auto rounded-lg border border-border" aria-label={t('link.remote')}>
      <h2 class="px-3 py-1.5 text-xs font-semibold">{t('link.remote')}</h2>
      {#if remoteState === 'gateway_off'}
        <div class="flex items-center gap-2 px-3 pb-2">
          <p class="text-xs text-muted-foreground">{t('link.gateway_off')}</p>
          <Button variant="outline" size="xs" onclick={() => security.openSettings('gateway')}>{t('gw.ep_open_settings')}</Button>
        </div>
      {:else}
        {#each remoteRows as model (model.id)}
          <div class="flex items-center justify-between gap-3 border-t border-border px-3 py-1.5">
            <div class="min-w-0">
              <p class="truncate font-mono text-xs">{model.model}</p>
              <p class={cn('truncate text-xs', model.state === 'online' ? 'text-success' : 'text-muted-foreground')}>{model.link} · {t(model.state === 'online' ? 'link.online' : model.state === 'unauthorized' ? 'link.token_rejected' : 'link.offline')}</p>
            </div>
            <Button variant="outline" size="xs" disabled={model.state !== 'online'} onclick={() => openRemote(model)}>{t('lib.open_in_playground')}</Button>
          </div>
        {:else}
          <p class="border-t border-border px-3 py-1.5 text-xs text-muted-foreground">{t('link.models_empty')}</p>
        {/each}
      {/if}
    </section>
  {/if}
    </div>
  </main>
</div>

{#if editDef}
  <DefinitionDialog
    definition={editDef}
    {models}
    onSaved={(def) => {
      const before = editDef;
      editDef = null;
      toast.success(t('def.saved', { alias: def.alias }));
      if (before?.id && before.id === def.id && before.alias !== def.alias) {
        toast.warning(t('def.alias_changed', { old: before.alias, alias: def.alias }), { duration: 10000 });
      }
      void domain.refreshStates();
    }}
    onClose={() => (editDef = null)}
  />
{/if}

{#if deleteId}
  <DeleteModelDialog modelId={deleteId} onDone={afterDelete} onClose={() => (deleteId = null)} />
{/if}

{#if moveEntry}
  <MigrationModal
    model={moveEntry.model}
    {storageDirs}
    onMigrated={async () => {
      library.markChanged();
      await domain.refreshDefinitions();
    }}
    onClose={() => (moveEntry = null)}
  />
{/if}

{#if showDupes}
  <DuplicatesDialog version={dupesVersion} onDelete={(id) => (deleteId = id)} onClose={() => (showDupes = false)} />
{/if}
