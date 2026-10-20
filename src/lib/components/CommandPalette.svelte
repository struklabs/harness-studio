<script lang="ts">
  import { Command as CommandPrimitive } from 'bits-ui';
  import * as Command from '#lib/components/ui/command/index.js';
  import {
    LayoutDashboard,
    MessageSquare,
    FolderKanban,
    Server,
    Compass,
    Library,
    Cable,
    Activity,
    ChartColumn,
    Settings,
    Moon,
    Sun,
    ZoomIn,
    Languages,
    Square,
    SlidersHorizontal,
    MessageSquarePlus,
    Boxes,
    Rocket,
  } from '@lucide/svelte';
  import { invoke } from '#lib/ipc.js';
  import { presetName } from '#lib/components/presets/presetText.js';
  import { t, getLanguage, inAllLanguages, type TKey } from '#lib/i18n.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { pageLevel, tabLevel } from '#lib/uiLevel.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { PLAYGROUND_MODES } from './playground/navigation';
  import { CREATE_PAGES } from './create/navigation';
  import { CONNECTION_PAGES } from './connections/navigation';
  import { requestSection } from '#lib/stores/sectionPage.svelte.js';
  import { getStarted } from '#lib/stores/getStarted.svelte.js';
  import { panes } from '#lib/stores/panes.svelte.js';
  import { DASHBOARD_GROUPS, PLAYGROUND_GROUPS, LIBRARY_GROUPS, PERFORMANCE_GROUPS, ANALYTICS_GROUPS } from './common/screenSections';
  import { SETTINGS_SECTIONS } from './settings/sections';
  import { SETTINGS_SEARCH_KEYS } from './settings/searchFields';
  import { paletteFilter } from '#lib/search.js';
  import type { AppSettings } from '#lib/types.js';
  import { UI_SCALES, normalizeScale } from '#lib/uiScale.js';
  import type { Tab } from './Sidebar.svelte';

  interface Props {
    open?: boolean;
    onNavigate: (tab: Tab) => void;
    onSettingsUpdated: (s: AppSettings) => void;
  }

  let { open = $bindable(false), onNavigate, onSettingsUpdated }: Props = $props();
  let query = $state('');

  $effect(() => {
    if (open) query = '';
  });

  const pages: { id: Tab; key: TKey; icon: typeof LayoutDashboard }[] = [
    { id: 'dashboard', key: 'nav.dashboard', icon: LayoutDashboard },
    { id: 'servers', key: 'nav.servers', icon: Server },
    { id: 'library', key: 'nav.library', icon: Library },
    { id: 'hub', key: 'nav.hub', icon: Compass },
    { id: 'modules', key: 'modules.title', icon: Compass },
    { id: 'playground', key: 'nav.playground', icon: MessageSquare },
    { id: 'projects', key: 'nav.projects', icon: FolderKanban },
    { id: 'connections', key: 'nav.connections', icon: Cable },
    { id: 'performance', key: 'nav.performance', icon: Activity },
    { id: 'analytics', key: 'nav.analytics', icon: ChartColumn },
    { id: 'settings', key: 'nav.settings', icon: Settings },
  ];

  /** Search terms of an entry in both languages: its label plus the keyword list (`cmd.kw_*`). */
  function keywordsOf(...keys: TKey[]): string[] {
    return keys.flatMap((k) => inAllLanguages(k));
  }

  function openSection(id: string) {
    security.openSettings(id, query);
  }

  // Re-read the applied theme whenever the palette opens (`system` resolves at runtime).
  let isDark = $derived.by(() => {
    void open;
    return typeof document !== 'undefined' && document.documentElement.getAttribute('data-scheme') === 'dark';
  });

  // Presets for "New chat with preset…" (refreshed whenever the palette opens).
  $effect(() => {
    if (open) void domain.refreshPresets();
  });

  function newChatWith(presetId: string) {
    domain.pendingNewChatPreset = presetId;
    onNavigate('playground');
  }

  function startModel() {
    domain.pendingModelPicker = true;
    onNavigate('playground');
  }

  function run(fn: () => void | Promise<void>) {
    open = false;
    void fn();
  }

  async function saveSettings(patch: Partial<AppSettings>) {
    if (!app.settings) return;
    const next = { ...$state.snapshot(app.settings), ...patch } as AppSettings;
    try {
      const saved = await invoke<AppSettings>('save_app_settings', { settings: next });
      onSettingsUpdated(saved ?? next);
    } catch {
      // Still apply locally so the palette never feels dead.
      onSettingsUpdated(next);
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      open = !open;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<Command.Dialog bind:open filter={paletteFilter} title={t('cmd.open')} description={t('cmd.placeholder')}>
  <Command.Input bind:value={query} placeholder={t('cmd.placeholder')} />
  <Command.List aria-label={t('cmd.results')}>
    <!-- bits-ui sorts groups by their best match only inside a Viewport. -->
    <CommandPrimitive.Viewport>
    <Command.Empty>{t('cmd.empty')}</Command.Empty>
    <Command.Group heading={t('cmd.group_nav')}>
      {#each pages as p}
        {@const Icon = p.icon}
        <Command.Item value={t(p.key)} keywords={keywordsOf(p.key)} onSelect={() => run(() => onNavigate(p.id))}>
          <Icon />
          <span>{t(p.key)}</span>
          {#if tabLevel(p.id)}<span class="ml-auto text-xs text-muted-foreground">{t('uiLevel.advanced')}</span>{/if}
        </Command.Item>
      {/each}
    </Command.Group>
    <Command.Separator />
    <Command.Group heading={t('nav.create')}>
      {#each CREATE_PAGES as page (page.id)}
        {@const Icon = page.icon}
        <Command.Item value={`${t('nav.create')}: ${t(page.label)}`} keywords={keywordsOf(page.label, 'nav.create')} onSelect={() => run(() => { app.requestedCreatePage = page.id; onNavigate('create'); })}>
          <Icon /><span>{t(page.label)}</span>
          <span class="ml-auto text-xs text-muted-foreground">{t('uiLevel.advanced')}</span>
        </Command.Item>
      {/each}
    </Command.Group>
    <Command.Separator />
    <Command.Group heading={t('nav.connections')}>
      {#each CONNECTION_PAGES as page (page.id)}
        {@const Icon = page.icon}
        <Command.Item value={`${t('nav.connections')}: ${t(page.label)}`} keywords={keywordsOf(page.label, 'nav.connections')}
          onSelect={() => run(() => { app.requestedConnectionPage = page.id; onNavigate('connections'); })}>
          <Icon /><span>{t(page.label)}</span>
          {#if page.level}<span class="ml-auto text-xs text-muted-foreground">{t('uiLevel.advanced')}</span>{/if}
        </Command.Item>
      {/each}
    </Command.Group>
    <Command.Separator />
    {#each [
      { screen: 'dashboard' as const, label: 'nav.dashboard' as const, groups: DASHBOARD_GROUPS },
      { screen: 'playground' as const, label: 'nav.playground' as const, groups: PLAYGROUND_GROUPS },
      { screen: 'library' as const, label: 'nav.library' as const, groups: LIBRARY_GROUPS },
      { screen: 'performance' as const, label: 'nav.performance' as const, groups: PERFORMANCE_GROUPS },
      { screen: 'analytics' as const, label: 'nav.analytics' as const, groups: ANALYTICS_GROUPS },
    ] as screen (screen.screen)}
      <Command.Group heading={t(screen.label)}>
        {#each screen.groups.flatMap(g => g.pages) as page (page.id)}
          {@const Icon = page.icon}
          {@const analyticsKeywords: TKey[] = screen.screen === 'analytics'
            ? page.id === 'overview' ? ['cmd.analytics_overview', 'cmd.kw_analytics_overview']
              : page.id === 'requests' ? ['cmd.analytics_requests', 'cmd.kw_analytics_requests'] : []
            : []}
          <Command.Item value={`${t(screen.label)}: ${t(page.label)}`} keywords={keywordsOf(page.label, screen.label, ...analyticsKeywords)} onSelect={() => run(() => { requestSection(screen.screen, page.id); if (screen.screen === 'playground') panes.pgSettings.open(); onNavigate(screen.screen); })}>
            <Icon /><span>{t(page.label)}</span>
            {#if tabLevel(screen.screen) || pageLevel(screen.groups, page.id)}<span class="ml-auto text-xs text-muted-foreground">{t('uiLevel.advanced')}</span>{/if}
          </Command.Item>
        {/each}
      </Command.Group>
      <Command.Separator />
    {/each}
    <Command.Group heading={t('nav.playground')}>
      {#each PLAYGROUND_MODES as mode (mode.id)}
        <Command.Item value={`${t('nav.playground')}: ${t(mode.label)}`} keywords={keywordsOf(mode.label, 'nav.playground')}
          onSelect={() => run(() => { app.requestedPlaygroundTab = mode.id; onNavigate('playground'); })}>
          <mode.icon /><span>{t(mode.label)}</span>
          {#if mode.level}<span class="ml-auto text-xs text-muted-foreground">{t('uiLevel.advanced')}</span>{/if}
        </Command.Item>
      {/each}
    </Command.Group>
    <Command.Separator />
    <Command.Group heading={t('cmd.group_settings')}>
      {#each SETTINGS_SECTIONS as sec (sec.id)}
        {@const Icon = sec.icon}
        <Command.Item
          value={`${t('nav.settings')}: ${t(sec.label)}`}
          keywords={keywordsOf(sec.label, sec.desc, sec.keywords, 'nav.settings', ...(SETTINGS_SEARCH_KEYS[sec.id] ?? []))}
          onSelect={() => run(() => openSection(sec.id))}
        >
          <Icon />
          <span>{t(sec.label)}</span>
          {#if sec.level}<span class="ml-auto text-xs text-muted-foreground">{t('uiLevel.advanced')}</span>{/if}
        </Command.Item>
      {/each}
    </Command.Group>
    <Command.Separator />
    <Command.Group heading={t('cmd.group_actions')}>
      <Command.Item value={t(uiLevel.advanced ? 'uiLevel.switch_simple' : 'uiLevel.switch_advanced')}
        onSelect={() => run(() => uiLevel.set(uiLevel.advanced ? 'simple' : 'advanced'))}>
        <SlidersHorizontal /><span>{t(uiLevel.advanced ? 'uiLevel.switch_simple' : 'uiLevel.switch_advanced')}</span>
      </Command.Item>
      <Command.Item value={t('cmd.get_started')} keywords={keywordsOf('cmd.get_started')} onSelect={() => run(() => { getStarted.show(); requestSection('dashboard', 'overview'); onNavigate('dashboard'); })}>
        <Rocket /><span>{t('cmd.get_started')}</span>
      </Command.Item>
      {#if app.isRunning}
        <Command.Item value={t('cmd.stop_server')} keywords={keywordsOf('cmd.stop_server')} onSelect={() => run(() => invoke('stop_server', app.serverArgs()).then(() => {}))}>
          <Square />
          <span>{t('cmd.stop_server')}</span>
        </Command.Item>
      {/if}
      {#if isDark}
        <Command.Item value={t('cmd.theme_light')} keywords={[...keywordsOf('cmd.theme_light'), 'açık']} onSelect={() => run(() => saveSettings({ theme: 'light', color_theme: 'light' }))}>
          <Sun />
          <span>{t('cmd.theme_light')}</span>
        </Command.Item>
      {:else}
        <Command.Item value={t('cmd.theme_dark')} keywords={keywordsOf('cmd.theme_dark')} onSelect={() => run(() => saveSettings({ theme: 'dark', color_theme: 'dark' }))}>
          <Moon />
          <span>{t('cmd.theme_dark')}</span>
        </Command.Item>
      {/if}
      {#each UI_SCALES as pct (pct)}
        {#if pct !== normalizeScale(app.settings?.ui_scale)}
          <Command.Item value={t('cmd.ui_scale', { pct })} onSelect={() => run(() => saveSettings({ ui_scale: pct }))}>
            <ZoomIn />
            <span>{t('cmd.ui_scale', { pct })}</span>
          </Command.Item>
        {/if}
      {/each}
      <Command.Item
        value={t('cmd.lang_switch')}
        keywords={[...keywordsOf('cmd.lang_switch'), 'dil', 'language', 'türkçe', 'english']}
        onSelect={() => run(() => saveSettings({ language: getLanguage() === 'tr' ? 'en' : 'tr' }))}
      >
        <Languages />
        <span>{t('cmd.lang_switch')}</span>
      </Command.Item>
    </Command.Group>
    <Command.Separator />
    <Command.Group heading={t('ip.cmd_group')}>
      <Command.Item value={t('ip.cmd_manage')} onSelect={() => run(() => domain.openPresetManager())}>
        <SlidersHorizontal />
        <span>{t('ip.cmd_manage')}</span>
      </Command.Item>
      <Command.Item value={t('ip.cmd_start_model')} onSelect={() => run(startModel)}>
        <Boxes />
        <span>{t('ip.cmd_start_model')}</span>
      </Command.Item>
      {#each domain.presets as p (p.id)}
        <Command.Item value={t('ip.cmd_new_chat', { name: presetName(p) })} onSelect={() => run(() => newChatWith(p.id))}>
          <MessageSquarePlus />
          <span>{t('ip.cmd_new_chat', { name: presetName(p) })}</span>
        </Command.Item>
      {/each}
    </Command.Group>
  </CommandPrimitive.Viewport>
  </Command.List>
</Command.Dialog>
