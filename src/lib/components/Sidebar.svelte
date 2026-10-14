<script lang="ts" module>
  export type Tab = 'dashboard' | 'playground' | 'servers' | 'library' | 'hub' | 'connections' | 'performance' | 'analytics' | 'settings' | 'create';
</script>

<script lang="ts">
  import {
    LayoutDashboard,
    MessageSquare,
    Server,
    Compass,
    Library,
    Cable,
    Activity,
    ChartColumn,
    Settings,
    Hammer,
    SlidersHorizontal,
    PanelLeftClose,
    PanelLeftOpen,
  } from '@lucide/svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { preloadTab } from '#lib/views.js';
  import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { isVisible, tabLevel } from '#lib/uiLevel.js';
  import UiLevelToggle from './common/UiLevelToggle.svelte';
  import { cn } from '#lib/utils.js';
  import { servedModelName } from '#lib/libraryDefinitions.js';
  import { panes } from '#lib/stores/panes.svelte.js';
  import ResizablePane from './common/ResizablePane.svelte';

  interface Props {
    currentTab: Tab;
    onTabChange: (tab: Tab) => void;
  }

  let { currentTab, onTabChange }: Props = $props();

  const pane = panes.nav;
  /** Icon rail: collapsed by the user, or the window is narrow and the panel is not floated open. */
  let isCollapsed = $derived(!pane.expanded);
  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.userAgent + navigator.platform);

  type NavItem = { level?: 'advanced'; id: Tab; labelKey: TKey; icon: typeof LayoutDashboard };
  const groups: { labelKey: TKey; items: NavItem[] }[] = [
    {
      labelKey: 'nav.group_run',
      items: [
        { id: 'dashboard', labelKey: 'nav.dashboard', icon: LayoutDashboard },
        { level: tabLevel('servers'), id: 'servers', labelKey: 'nav.servers', icon: Server },
      ],
    },
    {
      labelKey: 'nav.group_discover',
      items: [
        { id: 'library', labelKey: 'nav.library', icon: Library },
        { id: 'hub', labelKey: 'nav.hub', icon: Compass },
      ],
    },
    {
      labelKey: 'nav.group_use',
      items: [
        { id: 'playground', labelKey: 'nav.playground', icon: MessageSquare },
        { id: 'connections', labelKey: 'nav.connections', icon: Cable },
      ],
    },
    { labelKey: 'nav.create', items: [{ level: tabLevel('create'), id: 'create', labelKey: 'create.tools', icon: Hammer }] },
    {
      labelKey: 'nav.group_monitor',
      items: [
        { level: tabLevel('performance'), id: 'performance', labelKey: 'nav.performance', icon: Activity },
        { level: tabLevel('analytics'), id: 'analytics', labelKey: 'nav.analytics', icon: ChartColumn },
      ],
    },
  ];

  let visible = $derived(groups.map(group => ({ ...group, items: group.items.filter(item => isVisible(item.level, uiLevel.level)) })).filter(group => group.items.length));

  /** Servers that are up or coming up, shown as live entries like containers in OrbStack. */
  let liveServers = $derived(app.servers.filter((s) => s.status === 'Running' || s.status === 'Starting'));

  /** Navigating from the floating (narrow window) panel closes it. */
  function go(tab: Tab) {
    onTabChange(tab);
    if (pane.floating) pane.close();
  }

  function openServer(id: string) {
    app.selectServer(id);
    go('dashboard');
  }

  /** A GGUF by its file name, an ONNX export (`onnx/model_quantized.onnx`) by its repo folder. */
  function modelName(path: string | undefined): string {
    return path ? servedModelName(path, []) : '';
  }

  const itemBase =
    'group/nav flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-sm font-medium text-sidebar-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground';
</script>

{#snippet navButton(item: NavItem)}
  {@const Icon = item.icon}
  {@const active = currentTab === item.id}
  {@const label = t(item.labelKey)}
  <button
    type="button"
    data-tab={item.id}
    aria-current={active ? 'page' : undefined}
    aria-label={isCollapsed ? label : undefined}
    class={cn(
      itemBase,
      isCollapsed && 'justify-center px-0',
      active && 'bg-sidebar-accent text-sidebar-accent-foreground shadow-[inset_2px_0_0_var(--sidebar-primary)]'
    )}
    onclick={() => go(item.id)}
    onpointerenter={() => preloadTab(item.id)}
    onfocus={() => preloadTab(item.id)}
  >
    <Icon
      class={cn('size-4 shrink-0', active ? 'text-sidebar-primary' : 'text-muted-foreground group-hover/nav:text-sidebar-accent-foreground')}
      strokeWidth={active ? 2.1 : 1.8}
    />
    {#if !isCollapsed}<span class="truncate">{label}</span>{/if}
  </button>
{/snippet}

{#snippet withTooltip(label: string, content: import('svelte').Snippet)}
  {#if isCollapsed}
    <Tooltip.Root>
      <Tooltip.Trigger class="w-full">
        {#snippet child({ props })}
          <div {...props} class="w-full">{@render content()}</div>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content side="right" sideOffset={8}>{label}</Tooltip.Content>
    </Tooltip.Root>
  {:else}
    {@render content()}
  {/if}
{/snippet}

<ResizablePane {pane} side="left" label={t('pane.nav')} class="border-transparent bg-sidebar">
  <nav class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-2 py-3" aria-label={t('nav.aria_nav')}>
    {#each visible as group}
      <div class="flex flex-col gap-0.5">
        {#if !isCollapsed}
          <div class="px-2 pb-1 text-xs font-medium text-muted-foreground">
            {t(group.labelKey)}
          </div>
        {:else}
          <div class="mx-auto mb-1 h-px w-5 bg-sidebar-border" aria-hidden="true"></div>
        {/if}
        {#each group.items as item}
          {#snippet btn()}{@render navButton(item)}{/snippet}
          {@render withTooltip(t(item.labelKey), btn)}
        {/each}
      </div>
    {/each}

    <!-- Live servers -->
    <div class="flex flex-col gap-0.5">
      {#if !isCollapsed}
        <div class="flex items-center justify-between px-2 pb-1 text-xs font-medium text-muted-foreground">
          <span>{t('nav.running')}</span>
          {#if liveServers.length > 0}
            <span class="font-num text-xs text-success">{liveServers.length}</span>
          {/if}
        </div>
      {:else}
        <div class="mx-auto mb-1 h-px w-5 bg-sidebar-border" aria-hidden="true"></div>
      {/if}

      {#if liveServers.length === 0}
        {#if !isCollapsed}
          <p class="px-2 text-xs text-muted-foreground/70">{t('nav.no_running')}</p>
        {/if}
      {:else}
        {#each liveServers as srv (srv.id)}
          {@const selected = srv.id === app.selectedServerId && currentTab === 'dashboard'}
          {@const starting = srv.status === 'Starting'}
          {@const label = srv.id === DEFAULT_SERVER_ID ? t('srv.default_label') : app.serverLabel(srv.id)}
          {#snippet serverBtn()}
            <button
              type="button"
              class={cn(itemBase, 'h-auto py-1.5', isCollapsed && 'justify-center px-0', selected && 'bg-sidebar-accent text-sidebar-accent-foreground')}
              onclick={() => openServer(srv.id)}
              aria-label={isCollapsed ? label : undefined}
            >
              <span class="relative flex size-4 shrink-0 items-center justify-center" aria-hidden="true">
                <span class={cn('size-2 rounded-full', starting ? 'border-[1.5px] border-warning' : 'bg-success')}></span>
                {#if !starting}
                  <!-- Static halo: an infinite ping animation kept the GPU process at ~15% of a core. -->
                  <span class="absolute size-3.5 rounded-full bg-success/25"></span>
                {/if}
              </span>
              {#if !isCollapsed}
                <span class="flex min-w-0 flex-col items-start leading-tight">
                  <span class="w-full truncate text-left text-sm">{label}</span>
                  <span class="w-full truncate text-left font-num text-xs text-muted-foreground">
                    :{srv.active?.port ?? '—'} · {modelName(srv.active?.model_path)}
                  </span>
                </span>
              {/if}
            </button>
          {/snippet}
          {@render withTooltip([label, app.serverDescription(srv.id)].filter(Boolean).join(' — ') + ` · :${srv.active?.port ?? ''}`, serverBtn)}
        {/each}
      {/if}
    </div>
  </nav>

  <div class="flex flex-col gap-0.5 border-t border-sidebar-border px-2 py-2">
    {#if isCollapsed}
      {#snippet levelBtn()}
        <button type="button" class={cn(itemBase, 'justify-center px-0 text-muted-foreground')}
          aria-label={t('uiLevel.tooltip', { level: t(uiLevel.advanced ? 'uiLevel.advanced' : 'uiLevel.simple') })}
          disabled={uiLevel.busy} onclick={() => uiLevel.set(uiLevel.advanced ? 'simple' : 'advanced')}>
          <SlidersHorizontal class="size-4" />
        </button>
      {/snippet}
      {@render withTooltip(t('uiLevel.tooltip', { level: t(uiLevel.advanced ? 'uiLevel.advanced' : 'uiLevel.simple') }), levelBtn)}
    {:else}
      <div class="min-w-0 px-1 pb-2"><UiLevelToggle /></div>
    {/if}
    {#snippet settingsBtn()}{@render navButton({ id: 'settings', labelKey: 'nav.settings', icon: Settings })}{/snippet}
    {@render withTooltip(t('nav.settings'), settingsBtn)}
    <button
      type="button"
      class={cn(itemBase, 'text-muted-foreground', isCollapsed && 'justify-center px-0')}
      onclick={() => pane.toggle()}
      aria-label={`${isCollapsed ? t('sidebar.expand') : t('sidebar.collapse')} (${isMac ? '⌘' : 'Ctrl'}+B)`}
      title={`${t('shell.toggle_sidebar')} (${isMac ? '⌘' : 'Ctrl'}+B)`}
      aria-expanded={!isCollapsed}
      aria-controls={pane.elementId}
    >
      {#if isCollapsed}
        <PanelLeftOpen class="size-4" strokeWidth={1.8} />
      {:else}
        <PanelLeftClose class="size-4" strokeWidth={1.8} />
        <span>{t('sidebar.collapse')}</span>
      {/if}
    </button>
  </div>
</ResizablePane>
