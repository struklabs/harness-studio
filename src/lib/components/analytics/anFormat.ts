// Formatting for the Analytics screen. A value the store does not have renders as "—", never 0.
import { t, locale } from '#lib/i18n.svelte.js';
import { formatBytes } from '#lib/format.js';
import { NA, fmtDuration } from '#lib/components/perf/perfFormat.js';

export { NA };

export function fmtMs(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return NA;
  if (v >= 60_000) return fmtDuration(v / 1000);
  const n = (x: number, d: number) => x.toLocaleString(locale(), { minimumFractionDigits: d, maximumFractionDigits: d });
  if (v >= 10_000) return `${n(v / 1000, 1)} s`;
  if (v >= 1000) return `${n(v / 1000, 2)} s`;
  return `${Math.round(v)} ms`;
}

export function fmtTps(v: number | null | undefined): string {
  return v == null || !Number.isFinite(v) ? NA : `${v.toLocaleString(locale(), { maximumFractionDigits: 1 })} t/s`;
}

export function fmtInt(v: number | null | undefined): string {
  return v == null || !Number.isFinite(v) ? NA : Math.round(v).toLocaleString(locale());
}

/** Compact count: 1.2k, 3.4M. */
export function fmtCompact(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return NA;
  return new Intl.NumberFormat(locale(), { notation: 'compact', maximumFractionDigits: 1 }).format(v);
}

/** 0–1 ratio as a percentage. */
export function fmtRate(v: number | null | undefined, decimals = 1): string {
  if (v == null || !Number.isFinite(v)) return NA;
  // Locale order and separator: "98.6%" in English, "%98,6" in Turkish.
  return new Intl.NumberFormat(locale(), { style: 'percent', minimumFractionDigits: decimals, maximumFractionDigits: decimals }).format(v);
}

export function fmtBytes(v: number | null | undefined): string {
  return v == null || !Number.isFinite(v) || v <= 0 ? NA : formatBytes(v, 1);
}

export function fmtAgo(ms: number | null | undefined, now = Date.now()): string {
  if (ms == null) return NA;
  return t('an.ago', { time: fmtDuration(Math.max(0, (now - ms) / 1000)) });
}

export function fmtDateTime(ms: number | null | undefined): string {
  if (ms == null) return NA;
  return new Date(ms).toLocaleString(locale(), {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

export function fmtTime(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  return d.toLocaleString(locale(), sameDay ? { hour: '2-digit', minute: '2-digit', second: '2-digit' } : { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}

export function fmtDay(ms: number): string {
  return new Date(ms).toLocaleDateString(locale(), { month: 'short', day: 'numeric', timeZone: 'UTC' });
}

export type Range = '24h' | '7d' | '30d' | '90d' | 'all';
export const RANGES: Range[] = ['24h', '7d', '30d', '90d', 'all'];
const DAY = 86_400_000;
const SPAN: Record<Exclude<Range, 'all'>, number> = { '24h': DAY, '7d': 7 * DAY, '30d': 30 * DAY, '90d': 90 * DAY };

/** Start of a range (null = everything). */
export function rangeStart(r: Range, now = Date.now()): number | null {
  return r === 'all' ? null : now - SPAN[r];
}

/** Phase colours of the request timeline (theme tokens). */
export const PHASE_COLORS: Record<string, string> = {
  load: 'bg-chart-4',
  queue: 'bg-chart-5',
  prompt: 'bg-chart-2',
  generation: 'bg-chart-3',
  other: 'bg-muted-foreground/40',
};
