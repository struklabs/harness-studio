// Size and open state of the app's side panels (main nav, Playground lists, Dashboard controls,
// Settings sections). One `PaneState` per panel, rendered by `common/ResizablePane.svelte`.
//
// Three modes, decided by the window width (CSS px, so the UI scale / webview zoom counts):
// - docked:   the panel takes `size` px next to the content; the user can resize or collapse it.
// - rail:     panels with `rail > 0` collapse to an icon rail instead of disappearing.
// - floating: below `narrowBelow` the panel no longer takes space. It is collapsed (or a rail)
//             and opens over the content; outside click / Escape closes it. The docked
//             preference is kept untouched, so widening the window restores the user's layout.
// Left navigation uses a combined width budget instead: main nav always stays in flow;
// sections preserve their docked preference while drawers use transient floatingOpen.
import { MediaQuery } from 'svelte/reactivity';
import { innerWidth } from 'svelte/reactivity/window';
import { sidebarLayout } from './sidebarLayout.js';

export const sidebarContext = $state<{ section: 'connections' | 'create' | 'settings' | 'dashboard' | 'library' | 'performance' | 'analytics' | null }>({ section: null });

export interface PaneOptions {
  /** Storage key suffix; also used for the element id (`pane-<id>`). */
  id: string;
  defaultSize: number;
  min: number;
  max: number;
  /** Width of the collapsed icon rail; 0 = the panel disappears when collapsed. */
  rail?: number;
  /** Window widths below this float the panel over the content (0 = never). */
  narrowBelow?: number;
  /** Share of the window one docked panel may take at most. */
  maxShare?: number;
  defaultCollapsed?: boolean;
  /** Pre-existing boolean key to migrate from (`collapsedValue` = the stored value meaning collapsed). */
  legacy?: { key: string; collapsedValue: string };
}

const PREFIX = 'harness_pane_';

/** Window width in CSS px; a desktop default until the first measurement. */
export function viewportWidth(): number {
  return innerWidth.current ?? 1280;
}

/**
 * Short window (CSS px height): a 900×600 window, or 1080p maximised at 150 %. Pinned footers
 * (Dashboard start area) switch to a compact layout. Same threshold as the `short:` CSS variant
 * in app.css.
 */
export const shortWindow = new MediaQuery('(max-height: 760px)');

export class PaneState {
  readonly id: string;
  readonly defaultSize: number;
  readonly min: number;
  readonly max: number;
  readonly rail: number;
  readonly narrowBelow: number;
  readonly maxShare: number;
  readonly defaultCollapsed: boolean;

  /** Preferred width while expanded (persisted). */
  size = $state(0);
  /** Docked preference (persisted). */
  collapsed = $state(false);
  /** Floating mode only: whether the panel is shown over the content (not persisted). */
  floatingOpen = $state(false);
  /** A resize drag is in progress (disables width transitions). */
  dragging = $state(false);

  constructor(o: PaneOptions) {
    this.id = o.id;
    this.defaultSize = o.defaultSize;
    this.min = o.min;
    this.max = o.max;
    this.rail = o.rail ?? 0;
    this.narrowBelow = o.narrowBelow ?? 0;
    this.maxShare = o.maxShare ?? 0.34;
    this.defaultCollapsed = o.defaultCollapsed ?? false;
    this.size = o.defaultSize;
    this.collapsed = this.defaultCollapsed;
    this.load(o.legacy);
  }

  get elementId(): string {
    return `pane-${this.id}`;
  }

  get isSection(): boolean { return ['connections-nav', 'create-nav', 'settings-nav', 'dashboard-nav', 'library-nav', 'performance-nav', 'analytics-nav'].includes(this.id); }

  get leftLayout(): ReturnType<typeof sidebarLayout> {
    const section = sidebarContext.section ? panes[`${sidebarContext.section}Nav`] : null;
    return sidebarLayout({ viewport: viewportWidth(), mainWidth: panes.nav.width,
      mainRail: panes.nav.rail, mainCollapsed: panes.nav.collapsed,
      sectionWidth: section?.width ?? 0, sectionRail: section?.rail ?? 0 });
  }

  /** The window is too narrow to dock this panel open. */
  get narrow(): boolean {
    if (this.id === 'nav') return false;
    if (this.isSection) return this.leftLayout.sectionDrawer;
    return this.narrowBelow > 0 && viewportWidth() < this.narrowBelow;
  }

  /** The panel shows its full content (docked open, or floating open). */
  get expanded(): boolean {
    if (this.id === 'nav' && this.leftLayout.forcedMainRail) return false;
    return this.narrow ? this.floatingOpen : !this.collapsed;
  }

  /** Shown over the content rather than beside it. */
  get floating(): boolean {
    return this.narrow && this.floatingOpen;
  }

  /** Largest width allowed right now (never below `min`). */
  get maxNow(): number {
    const vw = viewportWidth();
    // Left sidebar widths must not depend on narrow/leftLayout (which uses these widths).
    const share = this.isSection ? Math.max(this.min, vw - 56) : this.id === 'nav' ? vw * this.maxShare : this.narrow ? vw - 48 : vw * this.maxShare;
    return Math.max(this.min, Math.min(this.max, Math.floor(share)));
  }

  /** Width of the expanded panel. */
  get width(): number {
    return clamp(this.size, this.min, this.maxNow);
  }

  /** Space the panel takes in the layout. */
  get dockedWidth(): number {
    if (this.id === 'nav') return this.leftLayout.navWidth;
    if (this.narrow || this.collapsed) return this.rail;
    return this.width;
  }

  toggle(): void {
    if (this.narrow) this.floatingOpen = !this.floatingOpen;
    else this.setCollapsed(!this.collapsed);
  }

  open(): void {
    if (this.narrow) this.floatingOpen = true;
    else this.setCollapsed(false);
  }

  /** Closes the floating panel; collapses a docked one. */
  close(): void {
    if (this.narrow) this.floatingOpen = false;
    else this.setCollapsed(true);
  }

  setCollapsed(v: boolean): void {
    this.collapsed = v;
    this.save();
  }

  /**
   * Live drag: `raw` is the width under the pointer. Dragging well past `min` collapses a docked
   * panel (to its rail, if any); dragging back out re-opens it.
   */
  dragTo(raw: number): void {
    if (!this.narrow) {
      const threshold = Math.max(this.rail, this.min * 0.6);
      if (raw < threshold) {
        this.collapsed = true;
        return;
      }
      this.collapsed = false;
    }
    this.size = clamp(Math.round(raw), this.min, this.maxNow);
  }

  /** Keyboard / programmatic resize (always expanded afterwards). */
  resizeBy(delta: number): void {
    this.setSize(this.width + delta);
  }

  setSize(px: number): void {
    this.size = clamp(Math.round(px), this.min, this.maxNow);
    if (!this.narrow) this.collapsed = false;
    this.save();
  }

  /** Double-click on the handle: back to the default width, expanded. */
  reset(): void {
    this.size = this.defaultSize;
    if (!this.narrow) this.collapsed = false;
    this.save();
  }

  save(): void {
    try {
      localStorage.setItem(PREFIX + this.id, JSON.stringify({ size: this.size, collapsed: this.collapsed }));
    } catch {
      // Storage unavailable: the layout just is not remembered.
    }
  }

  private load(legacy?: PaneOptions['legacy']): void {
    try {
      const raw = localStorage.getItem(PREFIX + this.id);
      if (raw) {
        const v = JSON.parse(raw) as { size?: unknown; collapsed?: unknown };
        if (typeof v.size === 'number' && Number.isFinite(v.size)) this.size = clamp(v.size, this.min, this.max);
        if (typeof v.collapsed === 'boolean') this.collapsed = v.collapsed;
        return;
      }
      if (legacy) {
        const old = localStorage.getItem(legacy.key);
        if (old !== null) this.collapsed = old === legacy.collapsedValue;
      }
    } catch {
      // Unreadable value: keep the defaults.
    }
  }
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

// The app shell's panels. Module-level so every page shares one instance per panel.
export const panes = {
  /** Main navigation: icon rail when collapsed or when the window is narrow. */
  nav: new PaneState({
    id: 'nav',
    defaultSize: 216,
    min: 184,
    max: 320,
    rail: 56,
    maxShare: 0.26,
    legacy: { key: 'harness_sidebar_collapsed', collapsedValue: '1' },
  }),
  pgConversations: new PaneState({
    id: 'pg-conversations',
    defaultSize: 240,
    min: 184,
    max: 400,
    narrowBelow: 900,
    maxShare: 0.3,
    legacy: { key: 'harness_playground_sidebar', collapsedValue: '0' },
  }),
  pgSettings: new PaneState({ id: 'pg-settings', defaultSize: 320, min: 264, max: 520, narrowBelow: 1100, maxShare: 0.34 }),
  dashboardControls: new PaneState({ id: 'dash-controls', defaultSize: 360, min: 304, max: 560, maxShare: 0.45 }),
  libraryNav: new PaneState({ id: 'library-nav', defaultSize: 224, min: 184, max: 300, rail: 48 }),
  performanceNav: new PaneState({ id: 'performance-nav', defaultSize: 224, min: 184, max: 300, rail: 48 }),
  analyticsNav: new PaneState({ id: 'analytics-nav', defaultSize: 224, min: 184, max: 300, rail: 48 }),
  dashboardNav: new PaneState({ id: 'dashboard-nav', defaultSize: 224, min: 184, max: 300, rail: 48 }),
  /** Sections float only when their combined width would squeeze the content below 480px. */
  connectionsNav: new PaneState({ id: 'connections-nav', defaultSize: 224, min: 184, max: 300, rail: 48, maxShare: 0.28 }),
  createNav: new PaneState({ id: 'create-nav', defaultSize: 224, min: 184, max: 300, rail: 48, maxShare: 0.28 }),
  settingsNav: new PaneState({ id: 'settings-nav', defaultSize: 224, min: 184, max: 300, rail: 48, maxShare: 0.28 }),
};
