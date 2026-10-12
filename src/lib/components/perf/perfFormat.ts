import { t } from '$lib/i18n.svelte';

/** Placeholder for a value the server or OS doesn't report (never a fake zero). */
export const NA = '—';

export function fmtPercent(v: number | null | undefined, decimals = 0): string {
  return v == null || !Number.isFinite(v) ? NA : `${v.toFixed(decimals)}%`;
}

export function fmtCount(v: number | null | undefined): string {
  return v == null || !Number.isFinite(v) ? NA : Math.round(v).toLocaleString();
}

export function fmtSeconds(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return NA;
  return v < 10 ? `${v.toFixed(2)} s` : `${v.toFixed(1)} s`;
}

/** Elapsed time, e.g. "2 sa 5 dk", "3 dk 12 sn", "40 sn". */
export function fmtDuration(secs: number | null | undefined): string {
  if (secs == null || !Number.isFinite(secs)) return NA;
  const s = Math.max(0, Math.floor(secs));
  if (s >= 3600) return t('perf.dur_hm', { h: Math.floor(s / 3600), m: Math.floor((s % 3600) / 60) });
  if (s >= 60) return t('perf.dur_ms', { m: Math.floor(s / 60), s: s % 60 });
  return t('perf.dur_s', { s });
}

/** Tokens per second from a token count and the seconds it took. */
export function rate(tokens: number, seconds: number): number | null {
  return seconds > 0 ? tokens / seconds : null;
}
