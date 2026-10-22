<script lang="ts">
  import { fixed } from '#lib/format.js';
  // The window's title bar (the OS frame is off, see tauri.conf.json): brand over the nav column,
  // page title, command search, the one global server indicator, window controls.
  // Empty areas and plain text are drag regions; double-click toggles maximize (Tauri's drag script).
  import type { ServerStatus, SystemHardwareInfo, AppSettings } from '#lib/types.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { panes } from '#lib/stores/panes.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { Square, Search, Cpu } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Kbd from '#lib/components/ui/kbd/index.js';
  import { cn } from '#lib/utils.js';
  import Sparkline from './common/Sparkline.svelte';
  import ServerPicker from './common/ServerPicker.svelte';
  import BrandLogo from './common/BrandLogo.svelte';
  import WindowControls from './common/WindowControls.svelte';
  import TitleBarToolbar from './common/TitleBarToolbar.svelte';
  import type { Tab } from './Sidebar.svelte';

  interface Props {
    currentTab?: Tab;
    serverStatus?: ServerStatus;
    hardware?: SystemHardwareInfo | null;
    settings?: AppSettings | null;
    /** Only brand and window controls (loading, error and first-run screens). */
    minimal?: boolean;
    onStopServer?: () => void;
    onOpenCommand?: () => void;
  }

  let {
    currentTab = 'dashboard',
    serverStatus = 'Stopped',
    hardware = null,
    settings = null,
    minimal = false,
    onStopServer,
    onOpenCommand,
  }: Props = $props();

  const tabTitleKeys: Record<Tab, TKey> = {
    dashboard: 'nav.dashboard',
    playground: 'nav.playground',
    projects: 'nav.projects',
    coding: 'nav.coding',
    servers: 'nav.servers',
    library: 'nav.library',
    hub: 'nav.hub',
    modules: 'modules.title',
    connections: 'nav.connections',
    performance: 'nav.performance',
    analytics: 'nav.analytics',
    settings: 'nav.settings',
    create: 'nav.create',
    training: 'nav.training',
    office: 'office.title',
  };

  let pageTitle = $derived(t(tabTitleKeys[currentTab] || 'nav.dashboard'));

  type Kind = 'running' | 'starting' | 'stopping' | 'stopped' | 'error';
  let kind = $derived<Kind>(
    typeof serverStatus === 'object'
      ? 'error'
      : (({ Running: 'running', Starting: 'starting', Stopping: 'stopping', Stopped: 'stopped' }) as const)[serverStatus]
  );
  // "Stopped" is the dot's grey; only transitions and failures get words.
  const statusKey: Partial<Record<Kind, TKey>> = {
    starting: 'status.starting',
    stopping: 'status.stopping',
    error: 'status.error',
  };

  let gpu = $derived.by(() => {
    const gpus = hardware?.gpus ?? [];
    const dedicated = gpus.find((g) => g.is_discrete) ?? gpus[0];
    return dedicated?.name ?? hardware?.cpu_name ?? t('hw.unknown');
  });
  // The llama.cpp build actually installed (managed runtime); a custom binary's backend is unknown.
  let backend = $derived(settings?.runtime_mode === 'Managed' ? (settings.managed_runtime?.backend ?? '') : '');
  let tps = $derived(app.telemetry?.predicted_tokens_per_sec ?? 0);

  const platform = typeof navigator !== 'undefined' ? navigator.userAgent + ' ' + navigator.platform : '';
  const isMac = /Mac/i.test(platform);

  // The brand block sits exactly over the nav column so the page title starts where the page does.
  // macOS: leave room for the traffic lights (tauri.macos.conf.json places them at x = 16).
  const MAC_LIGHTS = 80;
  let brandWidth = $derived(isMac ? Math.max(panes.nav.dockedWidth, MAC_LIGHTS + 28) : panes.nav.dockedWidth);
  let showBrandText = $derived(brandWidth - (isMac ? MAC_LIGHTS : 16) >= 150);

  let isStopping = $state(false);
  async function handleQuickStop() {
    if (onStopServer) return onStopServer();
    isStopping = true;
    try {
      await invoke('stop_server', app.serverArgs());
    } catch (e) {
      console.error('Failed to stop server from top bar:', e);
    } finally {
      isStopping = false;
    }
  }
</script>

<header
  class="flex h-11 shrink-0 items-stretch bg-sidebar text-sidebar-foreground select-none"
  aria-label={t('nav.aria_toolbar')}
  data-tauri-drag-region
>
  <div
    class={cn('flex shrink-0 items-center overflow-hidden', !panes.nav.dragging && 'transition-[width] duration-150 ease-out motion-reduce:transition-none')}
    style:width="{brandWidth}px"
    style:padding-left="{isMac ? MAC_LIGHTS : 18}px"
    data-tauri-drag-region="deep"
  >
    <BrandLogo size={18} showText={showBrandText} version="" />
  </div>

  {#if minimal}
    <div class="min-w-0 flex-1" data-tauri-drag-region></div>
  {:else}
    <div
      class="grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_auto_minmax(0,auto)] items-center gap-3 pr-2 pl-4"
      data-tauri-drag-region
    >
      <h1 id="page-title" class="min-w-0 truncate text-sm font-semibold tracking-tight text-foreground max-lg:sr-only" data-tauri-drag-region>{pageTitle}</h1>

      <button
        type="button"
        onclick={onOpenCommand}
        class="flex h-7 items-center gap-2 rounded-md border border-border bg-background/60 px-2 text-xs text-muted-foreground transition-colors hover:bg-background hover:text-foreground max-md:w-7 max-md:justify-center max-md:px-0 md:w-[clamp(200px,30vw,400px)]"
        aria-label={t('cmd.open')}
        title={t('cmd.open')}
      >
        <Search class="size-3.5 shrink-0" />
        <span class="flex-1 truncate text-left max-md:hidden">{t('cmd.search')}</span>
        <Kbd.Group class="max-md:hidden">
          <Kbd.Root>{isMac ? '⌘' : 'Ctrl'}</Kbd.Root>
          <Kbd.Root>K</Kbd.Root>
        </Kbd.Group>
      </button>

      <!-- The hardware label gives way first; the speed badge never shrinks under the Stop button. -->
      <div class="flex min-w-0 items-center justify-end gap-2.5 overflow-hidden" data-tauri-drag-region>
        <div class="hidden min-w-0 shrink-100 items-center gap-1.5 text-xs text-muted-foreground xl:flex" title={t('hw.detected')} data-tauri-drag-region="deep">
          <Cpu class="size-3.5 shrink-0" />
          <span class="truncate">{gpu}</span>
          {#if backend}
            <span class="rounded border border-border px-1 font-mono text-xs leading-4" title={t('shell.backend_in_use')}>{backend}</span>
          {/if}
        </div>

        <!-- The one global server indicator: which server the app works with, and its state. -->
        <div class="flex min-w-0 items-center gap-2 text-xs" role="status" aria-live="polite">
          <ServerPicker global />
          {#if kind === 'running'}
            <span class="hidden shrink-0 items-center gap-1.5 rounded-md bg-primary/10 px-1.5 py-0.5 text-primary xl:flex" title={t('header.gen_speed')}>
              <Sparkline values={app.tpsHistory} width={40} height={14} />
              <span class="font-num text-xs">{fixed(tps, 1)}</span>
              <span class="text-xs opacity-70">{t('header.tps_unit')}</span>
            </span>
          {:else if statusKey[kind]}
            <span class={kind === 'error' ? 'text-destructive' : 'text-muted-foreground'}>{t(statusKey[kind]!)}</span>
          {/if}
        </div>

        {#if kind === 'running'}
          <Button variant="destructive" size="sm" class="h-7" onclick={handleQuickStop} disabled={isStopping} aria-label={t('dash.stop_server')} title={t('dash.stop_server')}>
            <Square class="size-3" fill="currentColor" />
            <span class="max-lg:hidden">{t('common.stop')}</span>
          </Button>
        {/if}
      </div>
    </div>
  {/if}

  {#if !minimal}
    <TitleBarToolbar serverKind={kind} {onOpenCommand} />
  {/if}

  {#if !isMac}
    <WindowControls />
  {/if}
</header>
