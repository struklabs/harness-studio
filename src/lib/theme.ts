/*
 * Colour themes. Every theme defines the same semantic tokens in src/app.css; this module only
 * decides which one is active and writes three attributes on <html>:
 *
 *   data-theme  = concrete palette id (never "system")
 *   data-scheme = "dark" | "light"  (drives Tailwind's `dark:` variant, toasts, color-scheme)
 *   data-cvd    = "on" when the colour-vision-safe status/chart palette is enabled
 *
 * The same resolution is inlined in src/app.html so the first paint already has the right theme.
 */

/** A theme the user can pick. `system` resolves to dark/light (or their high-contrast twins). */
export type ThemeId = 'system' | 'dark' | 'light' | 'oled' | 'night' | 'hc-dark' | 'hc-light';
/** Legacy `settings.theme` value, still written so older builds open in the nearest palette. */
export type ThemeMode = 'dark' | 'light' | 'system';
export type Scheme = 'dark' | 'light';
type ConcreteTheme = Exclude<ThemeId, 'system'>;

export const THEME_IDS: readonly ThemeId[] = ['system', 'dark', 'light', 'oled', 'night', 'hc-dark', 'hc-light'];

const SCHEME: Record<ConcreteTheme, Scheme> = {
  dark: 'dark',
  light: 'light',
  oled: 'dark',
  night: 'dark',
  'hc-dark': 'dark',
  'hc-light': 'light',
};

export function isThemeId(v: unknown): v is ThemeId {
  return typeof v === 'string' && (THEME_IDS as readonly string[]).includes(v);
}

export function schemeOf(id: ThemeId): Scheme | null {
  return id === 'system' ? null : SCHEME[id];
}

/** Value for the legacy `theme` field. */
export function baseMode(id: ThemeId): ThemeMode {
  return id === 'system' ? 'system' : SCHEME[id];
}

/** The theme stored in settings: `color_theme` when set and known, else the legacy `theme`. */
export function themeFromSettings(s: { theme?: string; color_theme?: string } | null | undefined): ThemeId {
  if (isThemeId(s?.color_theme)) return s.color_theme;
  if (isThemeId(s?.theme)) return s.theme;
  return 'dark';
}

function mq(query: string): boolean {
  return typeof window !== 'undefined' && !!window.matchMedia && window.matchMedia(query).matches;
}

/** `system` follows the OS: light/dark, and the high-contrast pair when the OS asks for more contrast. */
export function resolveTheme(id: ThemeId): ConcreteTheme {
  if (id !== 'system') return id;
  const dark = !mq('(prefers-color-scheme: light)');
  if (mq('(prefers-contrast: more)')) return dark ? 'hc-dark' : 'hc-light';
  return dark ? 'dark' : 'light';
}

/** Scheme currently applied on <html> (for toasts, charts, etc.). */
export function currentScheme(): Scheme {
  if (typeof document === 'undefined') return 'dark';
  return document.documentElement.getAttribute('data-scheme') === 'light' ? 'light' : 'dark';
}

let systemCleanup: (() => void) | null = null;
const listeners = new Set<(scheme: Scheme) => void>();

/** Called whenever the applied scheme may have changed (theme pick or OS change under `system`). */
export function onSchemeChange(fn: (scheme: Scheme) => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function write(id: ThemeId, cvd: boolean) {
  const html = document.documentElement;
  const concrete = resolveTheme(id);
  html.setAttribute('data-theme', concrete);
  html.setAttribute('data-scheme', SCHEME[concrete]);
  if (cvd) html.setAttribute('data-cvd', 'on');
  else html.removeAttribute('data-cvd');
  for (const fn of listeners) fn(SCHEME[concrete]);
}

export function applyTheme(id: ThemeId, cvd = false) {
  if (typeof document === 'undefined') return;
  try {
    localStorage.setItem('harness_theme', id);
    localStorage.setItem('harness_cvd', cvd ? '1' : '0');
  } catch {
    // Storage may be unavailable; the theme still applies for this session.
  }

  systemCleanup?.();
  systemCleanup = null;

  write(id, cvd);

  if (id === 'system' && window.matchMedia) {
    const queries = ['(prefers-color-scheme: light)', '(prefers-contrast: more)'].map((q) => window.matchMedia(q));
    const onChange = () => write('system', cvd);
    for (const q of queries) q.addEventListener('change', onChange);
    systemCleanup = () => {
      for (const q of queries) q.removeEventListener('change', onChange);
    };
  }
}

/** Applies the theme saved in app settings. */
export function applyThemeSettings(s: { theme?: string; color_theme?: string; cvd_safe?: boolean }) {
  applyTheme(themeFromSettings(s), !!s.cvd_safe);
}
