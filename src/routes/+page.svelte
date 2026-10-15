<script lang="ts">
  import AdvancedPageNotice from '#lib/components/common/AdvancedPageNotice.svelte';
  import { tabLevel } from '#lib/uiLevel.js';
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import type { SystemHardwareInfo, AppSettings } from '#lib/types.js';
  import Sidebar, { type Tab } from '#lib/components/Sidebar.svelte';
  import Header from '#lib/components/Header.svelte';
  import Dashboard from '#lib/components/Dashboard.svelte';
  import LazyView from '#lib/components/common/LazyView.svelte';
  import { views, preloadAllTabs } from '#lib/views.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import CommandPalette from '#lib/components/CommandPalette.svelte';
  import SecurityDialogs from '#lib/components/common/SecurityDialogs.svelte';
  import CloseDialog from '#lib/components/common/CloseDialog.svelte';
  import SaveLoadSettingsDialog from '#lib/components/common/SaveLoadSettingsDialog.svelte';
  import { unsavedLoad } from '#lib/stores/unsavedLoad.svelte.js';
  import ExposureBanner from '#lib/components/common/ExposureBanner.svelte';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { Toaster } from '#lib/components/ui/sonner/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { applyThemeSettings, currentScheme } from '#lib/theme.js';
  import { applyCachedScale, applyScale, normalizeScale, stepScale, zoomShortcut } from '#lib/uiScale.js';
  import { setLanguage, t, type Language } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { initWindowActivity, onBeforeRelease, onWindowActive, windowActive } from '#lib/windowActivity.js';
  import { hideSplash } from '#lib/splash.js';
  import { installOverlayGuard } from '#lib/actions/overlayGuard.js';
  import { connectionPageFromUrl } from '#lib/components/connections/navigationResolver.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { panes, sidebarContext } from '#lib/stores/panes.svelte.js';

  /** Tab to reopen after the window was released to the tray and built again. Forgotten once
   *  the window is seen: a page built in the background (tray hover) and released again unseen
   *  leaves it for the next one. */
  const RESTORE_TAB_KEY = 'harness_restore_tab';
  let currentTab = $state<Tab>(takeRestoreTab());
  // Leaving a screen with manual load settings that were used but not saved: ask once.
  let lastTab: Tab | null = null;
  $effect(() => {
    const tab = currentTab;
    if (lastTab !== null && tab !== lastTab) void unsavedLoad.flush();
    lastTab = tab;
  });
  $effect(() => {
    unsavedLoad.disabled = !!app.settings?.no_save_load_prompt;
  });
  $effect(() => { sidebarContext.section = currentTab === 'connections' || currentTab === 'create' || currentTab === 'settings' || currentTab === 'library' || currentTab === 'performance' || currentTab === 'analytics' || currentTab === 'dashboard' ? currentTab : null; });
  function forgetRestoreTab() {
    try {
      localStorage.removeItem(RESTORE_TAB_KEY);
    } catch {
      // storage unavailable
    }
  }
  function takeRestoreTab(): Tab {
    try {
      const tab = localStorage.getItem(RESTORE_TAB_KEY);
      const tabs: Tab[] = ['settings', 'dashboard', 'playground', 'analytics', 'performance', 'library', 'servers', 'library', 'hub', 'connections', 'performance', 'analytics', 'settings', 'create'];
      if (tabs.includes(tab as Tab)) return tab as Tab;
    } catch {
      // storage unavailable
    }
    return 'dashboard';
  }
  let playgroundOpened = $state(false);
  let performanceOpened = $state(false);
  let settingsOpened = $state(false);
  let libraryOpened = $state(false);
  let analyticsOpened = $state(false);
  $effect(() => { if (currentTab === 'settings') settingsOpened = true; if (currentTab === 'library') libraryOpened = true; if (currentTab === 'analytics') analyticsOpened = true; });
  $effect(() => { if (currentTab === 'performance') performanceOpened = true; });
  // "New server" from the header picker / Servers tab: the Dashboard is where it gets configured.
  $effect(() => {
    if (app.newServerPending) currentTab = 'dashboard';
  });

  // A component (e.g. the "open in Dashboard" toast action) asked for a tab.
  $effect(() => {
    const tab = app.requestedTab;
    if (tab) {
      currentTab = tab as Tab;
      app.requestedTab = null;
    }
  });

  $effect(() => {
    if (currentTab === 'playground') playgroundOpened = true;
  });
  // The preset manager is loaded the first time it is opened, then stays.
  let presetsOpened = $state(false);
  $effect(() => {
    if (domain.presetManagerOpen) presetsOpened = true;
  });
  let isInitializing = $state<boolean>(true);
  let initError = $state<string | null>(null);
  let showWizardManually = $state<boolean>(false);
  let commandOpen = $state(false);
  let toastTheme = $state<'light' | 'dark'>('dark');

  // Once the first real screen is up, fetch the other tabs' code in idle time (no empty pane on open).
  $effect(() => {
    if (!isInitializing && app.settings) preloadAllTabs();
  });

  // The start screen (app.html) stays until the first real screen or the error screen is up.
  $effect(() => {
    if (initError || (!isInitializing && app.settings)) hideSplash();
  });

  onMount(() => {
    const removeOverlayGuard = installOverlayGuard();

    const navigateConnections = () => {
      const id = connectionPageFromUrl(location.href);
      if (id) { app.requestedConnectionPage = id; currentTab = 'connections'; }
    };
    const navigateCreate = () => { const match = location.hash.match(/^#create\/(generator|auto|mine)$/); if (match) { app.requestedCreatePage = match[1]; currentTab = 'create'; } };
    const navigateSections = () => {
      const url = new URL(location.href);
      const screen = url.hash.match(/^#([^/]+)(?:\/|$)/)?.[1] ?? url.searchParams.get('screen') ?? url.searchParams.get('tab');
      if (['settings', 'dashboard', 'playground', 'analytics', 'performance', 'library', 'servers', 'create', 'connections', 'hub'].includes(screen ?? '')) currentTab = screen as Tab;
    };
    navigateSections();
    window.addEventListener('hashchange', navigateSections);
    window.addEventListener('popstate', navigateSections);
    navigateCreate();
    window.addEventListener('hashchange', navigateCreate);
    navigateConnections();
    window.addEventListener('hashchange', navigateConnections);
    window.addEventListener('popstate', navigateConnections);
    initApp();
    // No polling: the backend pushes server, telemetry, log and download changes (and only
    // what a visible view shows). Hidden to the tray, the page may be released; remember the
    // tab so the rebuilt window opens where it was.
    const stopApp = app.start();
    const stopDomain = domain.start();
    let offActivity: (() => void) | undefined;
    let offSeen: (() => void) | undefined;
    void initWindowActivity().then((fn) => {
      if (destroyed) return fn();
      offActivity = fn;
      if (windowActive()) forgetRestoreTab();
      else
        offSeen = onWindowActive(() => {
          forgetRestoreTab();
          offSeen?.();
        });
    });
    onBeforeRelease(() => {
      try {
        localStorage.setItem(RESTORE_TAB_KEY, currentTab);
      } catch {
        // storage unavailable
      }
    });

    // Frontend errors become local diagnostic events (only if the user opted in).
    let lastReport = 0;
    const report = (message: string, stack?: string) => {
      if (!app.settings?.privacy.app_errors) return;
      const now = Date.now();
      if (now - lastReport < 5000) return; // throttle error storms
      lastReport = now;
      invoke('save_crash_event', {
        event: { event_type: 'app_error', message, stack_trace: stack ?? null, metadata: null },
      }).catch(() => {});
    };
    const onError = (e: ErrorEvent) => report(e.message || 'Unknown error', e.error?.stack);
    const onRejection = (e: PromiseRejectionEvent) =>
      report(`Unhandled rejection: ${e.reason?.message ?? String(e.reason)}`, e.reason?.stack);
    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onRejection);

    // The background runtime auto-updater activated a new llama.cpp build.
    let unlistenRuntime: UnlistenFn | null = null;
    let destroyed = false;
    listen<AppSettings>('runtime-updated', (event) => app.setSettings(event.payload))
      .then((fn) => {
        if (destroyed) fn();
        else unlistenRuntime = fn;
      })
      .catch(() => {});

    return () => {
      window.removeEventListener('hashchange', navigateSections);
      window.removeEventListener('popstate', navigateSections);
      window.removeEventListener('hashchange', navigateCreate);
      window.removeEventListener('hashchange', navigateConnections);
      window.removeEventListener('popstate', navigateConnections);
      destroyed = true;
      removeOverlayGuard();
      unlistenRuntime?.();
      stopApp();
      stopDomain();
      offActivity?.();
      offSeen?.();
      window.removeEventListener('error', onError);
      window.removeEventListener('unhandledrejection', onRejection);
    };
  });

  async function initApp() {
    isInitializing = true;
    initError = null;
    try {
      // 1. Load app preferences first and apply theme/language immediately
      const appSettings = await invoke<AppSettings>('get_app_settings');
      app.setSettings(appSettings);
      applyPreferences(appSettings);

      // 2. Servers and profiles are what the first screen shows. Hardware (GPU name, fit
      //    estimates) fills in when detected; it does not hold the first screen back.
      const hardware = invoke<SystemHardwareInfo>('detect_system_hardware').then((hw) => {
        app.hardware = hw;
        return hw;
      });
      await Promise.all([app.refreshServer(), app.refreshProfiles()]);

      // Offline crash dumps (strictly governed by user privacy choices): after the first
      // screen, off the start path.
      void hardware
        .catch((err) => console.error('[Hardware] detection failed:', err))
        .then(() => invoke<unknown[]>('check_offline_crash'))
        .then((processed) => {
          if (processed.length > 0) {
            console.info(`[Diagnostics] ${processed.length} local report(s) created under your privacy settings.`);
          }
        })
        .catch((err) => console.error('[Diagnostics] Offline crash check failed:', err));
    } catch (e) {
      console.error('Failed to initialize app state:', e);
      initError = String(e);
    } finally {
      isInitializing = false;
    }
  }

  function applyPreferences(s: AppSettings) {
    applyThemeSettings(s);
    void applyScale(normalizeScale(s.ui_scale));
    toastTheme = currentScheme();
    if (s.language === 'tr' || s.language === 'en') setLanguage(s.language as Language);
  }

  applyCachedScale();

  function onGlobalKey(e: KeyboardEvent) {
    // Ctrl/Cmd+B: show or hide the navigation (rail <-> full width; floats open on narrow windows).
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'b') {
      if (!app.settings?.privacy.onboarding_completed) return;
      e.preventDefault();
      panes.nav.toggle();
      return;
    }
    void onZoomKey(e);
  }

  async function onZoomKey(e: KeyboardEvent) {
    const action = zoomShortcut(e);
    if (!action || !app.settings) return;
    e.preventDefault();
    const cur = normalizeScale(app.settings.ui_scale);
    const next = action === 'reset' ? 100 : stepScale(cur, action === 'in' ? 1 : -1);
    if (next === cur) return;
    void applyScale(next);
    const merged = { ...$state.snapshot(app.settings), ui_scale: next } as AppSettings;
    try {
      const saved = await invoke<AppSettings>('save_app_settings', { settings: merged });
      app.setSettings(saved ?? merged);
    } catch (err) {
      console.error('[UiScale] save failed:', err);
    }
  }

  /** After the first-run wizard: the Model Hub when no model exists yet, else the Playground. A page remembered from an earlier session is dropped. */
  async function landAfterWizard() {
    forgetRestoreTab();
    const models = await library.refresh();
    const tab: Tab = models.length === 0 ? 'hub' : 'playground';
    currentTab = tab;
  }

  function handleSettingsUpdated(updated: AppSettings) {
    app.setSettings(updated);
    applyPreferences(updated);
  }
</script>

<svelte:window onkeydown={onGlobalKey} />

{#if initError}
  <div class="flex h-screen w-screen flex-col bg-background">
  <Header minimal />
  <div class="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 p-6 text-center" role="alert">
    <div class="flex size-9 items-center justify-center rounded-full border border-destructive/40 bg-destructive/10 font-semibold text-destructive">!</div>
    <h2 class="text-base font-semibold">{t('onboarding.init_error_title')}</h2>
    <p class="selectable max-w-xl text-xs text-muted-foreground">{initError}</p>
    <Button onclick={initApp}>{t('onboarding.retry')}</Button>
  </div>
  </div>
{:else if isInitializing || !app.settings}
  <div class="flex h-screen w-screen flex-col bg-background">
    <Header minimal />
    <div class="flex min-h-0 flex-1 flex-col items-center justify-center gap-3">
      <div class="size-7 animate-spin rounded-full border-2 border-border border-t-primary" aria-hidden="true"></div>
      <h2 class="text-sm font-semibold">Harness Studio</h2>
      <p class="text-xs text-muted-foreground">{t('onboarding.loading_subtitle')}</p>
    </div>
  </div>
{:else}
  {@const settings = app.settings}
  {@const serverStatus = app.serverStatus}
  {@const hardware = app.hardware}

  <Tooltip.Provider delayDuration={250}>
    {#if !settings.privacy.onboarding_completed || showWizardManually}
      <LazyView
        load={views.onboarding}
        {settings}
        {hardware}
        onComplete={(updated: AppSettings) => {
          const firstRun = !settings.privacy.onboarding_completed;
          showWizardManually = false;
          handleSettingsUpdated(updated);
          if (firstRun) void landAfterWizard();
        }}
        onClose={settings.privacy.onboarding_completed
          ? () => {
              showWizardManually = false;
              // A step may have saved settings (e.g. installed the runtime) before the wizard was closed.
              invoke<AppSettings>('get_app_settings').then(handleSettingsUpdated).catch(() => {});
            }
          : undefined}
      />
    {/if}

    <!-- Frame: title bar and navigation share the chrome tone; pages sit on an inset sheet. -->
    <div class="flex h-screen w-screen flex-col overflow-hidden bg-sidebar text-foreground">
      <Header
        {currentTab}
        {serverStatus}
        {hardware}
        {settings}
        minimal={!settings.privacy.onboarding_completed || showWizardManually}
        onOpenCommand={() => (commandOpen = true)}
      />
      <div class="flex min-h-0 flex-1">
      <Sidebar {currentTab} onTabChange={(tab) => (currentTab = tab)} />

      <div class="relative flex min-w-0 flex-1 flex-col overflow-clip rounded-tl-[10px] border-t border-l border-border bg-background">
        <ExposureBanner />
        <AdvancedPageNotice level={tabLevel(currentTab)} />

        <!-- overflow-clip, not hidden: a hidden box is still a scroll container, so focusing or scrolling to
             an element inside a page could scroll the whole shell up and leave a blank band. -->
        <main class="relative min-h-0 flex-1 overflow-clip">
          <div class="h-full" hidden={currentTab !== 'dashboard'}>
            <Dashboard
              {settings}
              {serverStatus}
              {hardware}
              onGoToHub={() => (currentTab = 'hub')}
              onGoToServers={() => (currentTab = 'servers')}
              onGoToConnections={() => (currentTab = 'connections')}
            />
          </div>
          {#if currentTab === 'servers'}
            <LazyView
              load={views.servers}
              fallback
              onOpenDashboard={(id: string) => {
                app.selectServer(id);
                currentTab = 'dashboard';
              }}
              onOpenPlayground={(id: string) => {
                app.selectServer(id);
                currentTab = 'playground';
              }}
            />
          {:else if currentTab === 'hub'}
            <LazyView load={views.hub} fallback {hardware} />
          {:else if currentTab === 'create'}
            <LazyView load={views.create} fallback />
          {:else if currentTab === 'connections'}
            <LazyView load={views.connections} fallback {settings} {serverStatus} />
          {/if}
          {#if libraryOpened}<div class="h-full" hidden={currentTab !== 'library'}><LazyView load={views.library} fallback /></div>{/if}
          {#if analyticsOpened}<div class="h-full" hidden={currentTab !== 'analytics'}><LazyView load={views.analytics} fallback /></div>{/if}
          {#if settingsOpened}<div class="h-full" hidden={currentTab !== 'settings'}>
            <LazyView
              load={views.settings}
              fallback
              {settings}
              {hardware}
              onSettingsUpdated={handleSettingsUpdated}
              onReopenWizard={() => (showWizardManually = true)}
            />
          </div>{/if}
          {#if performanceOpened}
            <div class="h-full" hidden={currentTab !== 'performance'}>
              <LazyView load={views.performance} visible={currentTab === 'performance'} fallback {settings} {serverStatus} onGoToSettings={() => (currentTab = 'settings')} />
            </div>
          {/if}
          <!-- Kept mounted once opened so a reply keeps streaming while the user looks at other tabs. -->
          {#if playgroundOpened}
            <div class="h-full" hidden={currentTab !== 'playground'}>
              <LazyView load={views.playground} fallback onGoToDashboard={() => (currentTab = 'dashboard')} onGoToHub={() => (currentTab = 'hub')} />
            </div>
          {/if}
        </main>
      </div>
      </div>
    </div>

    <CommandPalette
      bind:open={commandOpen}
      onNavigate={(tab) => (currentTab = tab)}
      onSettingsUpdated={handleSettingsUpdated}
    />
    <SecurityDialogs />
    <CloseDialog />
    <SaveLoadSettingsDialog />
    {#if presetsOpened}
      <LazyView load={views.presets} />
    {/if}
    <Toaster theme={toastTheme} position="bottom-right" offset={{ bottom: 'calc(var(--toast-clearance, 0px) + 24px)' }} mobileOffset={{ bottom: 'calc(var(--toast-clearance, 0px) + 16px)' }} closeButtonAriaLabel={t('ui.toast_close')} />
  </Tooltip.Provider>
{/if}
