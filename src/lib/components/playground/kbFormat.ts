// Pure formatting helpers of the knowledge base UI (no imports, so node tests can load them).

/** `1:05`, `0:05`, `1:02:03` for a position in a recording. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** `1:05-1:32` for a span, a single time when the span is under a second; '' without a start. */
export function formatTimeSpan(startMs: number | null | undefined, endMs: number | null | undefined): string {
  if (startMs == null) return '';
  if (endMs == null || endMs - startMs < 1000) return formatClock(startMs);
  return `${formatClock(startMs)}-${formatClock(endMs)}`;
}

/** `12 %` style number with one decimal below 10 (the caller adds the unit). */
export function formatPercentValue(pct: number, locale: string): string {
  return pct.toLocaleString(locale, { maximumFractionDigits: pct < 10 ? 1 : 0 });
}

/** Average of the recent question latencies, or null when there are none. */
export function averageMs(total: number, count: number): number | null {
  return count > 0 ? total / count : null;
}
