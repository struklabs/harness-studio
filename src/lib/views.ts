import { loadView, type ViewLoader } from '#lib/components/common/LazyView.svelte';

/**
 * Views loaded on first use (see `LazyView`): each is its own JS chunk, so starting the app
 * parses only the shell and the Dashboard. The loaders are module constants on purpose (the
 * loaded component is cached per loader).
 */
export const views = {
  modules: (() => import('#lib/components/Modules.svelte')) as ViewLoader,
  create: (() => import('#lib/components/Create.svelte')) as ViewLoader,
  playground: (() => import('#lib/components/Playground.svelte')) as ViewLoader,
  servers: (() => import('#lib/components/Servers.svelte')) as ViewLoader,
  library: (() => import('#lib/components/Library.svelte')) as ViewLoader,
  hub: (() => import('#lib/components/ModelHub.svelte')) as ViewLoader,
  connections: (() => import('#lib/components/Connections.svelte')) as ViewLoader,
  performance: (() => import('#lib/components/PerformanceTab.svelte')) as ViewLoader,
  analytics: (() => import('#lib/components/Analytics.svelte')) as ViewLoader,
  settings: (() => import('#lib/components/Settings.svelte')) as ViewLoader,
  onboarding: (() => import('#lib/components/OnboardingWizard.svelte')) as ViewLoader,
  presets: (() => import('#lib/components/presets/PresetManager.svelte')) as ViewLoader,
};

/** Tabs that are a lazy view (the Dashboard is part of the main bundle). */
const tabViews: Record<string, ViewLoader> = {
  modules: views.modules,
  create: views.create,
  playground: views.playground,
  servers: views.servers,
  library: views.library,
  hub: views.hub,
  connections: views.connections,
  performance: views.performance,
  analytics: views.analytics,
  settings: views.settings,
};

/** Fetches (and in dev compiles) a tab's chunk ahead of the click, e.g. on pointer hover. */
export function preloadTab(tab: string): void {
  if (tabViews[tab]) void loadView(tabViews[tab]).catch(() => {});
}

let preloadStarted = false;

/**
 * Loads every tab's chunk one by one while the app is idle, right after the first screen. The
 * gap when opening a tab (an empty pane until its chunk arrived) was exactly that fetch, and in
 * `tauri dev` Vite's first compile of the module; after this a tab opens from memory.
 */
export function preloadAllTabs(): void {
  if (preloadStarted || typeof window === 'undefined') return;
  preloadStarted = true;
  const queue = Object.keys(tabViews);
  const idle = (fn: () => void) =>
    'requestIdleCallback' in window ? window.requestIdleCallback(fn, { timeout: 1500 }) : setTimeout(fn, 200);
  const next = () => {
    const tab = queue.shift();
    if (!tab) return;
    loadView(tabViews[tab])
      .catch(() => {})
      .finally(() => idle(next));
  };
  idle(next);
}
