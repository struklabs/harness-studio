import { getCurrentWebview } from '@tauri-apps/api/webview';

/** Selectable interface sizes, in percent. */
export const UI_SCALES = [90, 100, 115, 130, 150] as const;

const STORAGE_KEY = 'harness_ui_scale';

/** Default when nothing was ever saved: 115% on very wide screens, else 100%. */
export function defaultScale(): number {
  return typeof screen !== 'undefined' && screen.width >= 2560 ? 115 : 100;
}

/** Snaps any stored value to the nearest supported level. */
export function normalizeScale(v: number | null | undefined): number {
  if (v == null || !Number.isFinite(v)) return defaultScale();
  return UI_SCALES.reduce((a, b) => (Math.abs(b - v) < Math.abs(a - v) ? b : a));
}

/** Applies the scale through the webview zoom, so text, icons and spacing scale together. */
export async function applyScale(percent: number): Promise<void> {
  const p = normalizeScale(percent);
  try {
    localStorage.setItem(STORAGE_KEY, String(p));
  } catch {
    // Ignore storage issues
  }
  try {
    await getCurrentWebview().setZoom(p / 100);
  } catch (e) {
    console.error('[UiScale] setZoom failed:', e);
  }
}

/** Runs at startup before settings are loaded, using the last applied scale to avoid a visible jump. */
export function applyCachedScale(): void {
  let cached: number | null = null;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) cached = Number(raw);
  } catch {
    // Ignore storage issues
  }
  const p = normalizeScale(cached);
  if (p !== 100) void applyScale(p);
}

/** Next level after `current` in the given direction (clamped at the ends). */
export function stepScale(current: number, dir: 1 | -1): number {
  const i = UI_SCALES.indexOf(normalizeScale(current) as (typeof UI_SCALES)[number]);
  return UI_SCALES[Math.min(UI_SCALES.length - 1, Math.max(0, i + dir))];
}

/** Maps a Ctrl/Cmd zoom shortcut to an action, or null when the event is not one. */
export function zoomShortcut(e: KeyboardEvent): 'in' | 'out' | 'reset' | null {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return null;
  if (e.key === '=' || e.key === '+') return 'in';
  if (e.key === '-' || e.key === '_') return 'out';
  if (e.key === '0') return 'reset';
  return null;
}
