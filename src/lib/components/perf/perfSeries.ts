import type { TelemetryHistoryPoint } from '$lib/types';

/** Chart window: about five minutes at one sample per second. */
export const WINDOW = 300;

export interface Series {
  /** One value per second; `null` = the source reported nothing for that second. */
  values: (number | null)[];
  /** Tailwind text colour class (stroke uses currentColor). */
  color: string;
  label?: string;
}

export function recent(history: TelemetryHistoryPoint[]): TelemetryHistoryPoint[] {
  return history.length > WINDOW ? history.slice(history.length - WINDOW) : history;
}

export function pick(
  points: TelemetryHistoryPoint[],
  f: (p: TelemetryHistoryPoint) => number | null,
): (number | null)[] {
  return points.map(f);
}

/** `used / total` in percent, or `null` when the total isn't known. */
export function pct(used: number, total: number): number | null {
  return total > 0 ? Math.min(100, (used / total) * 100) : null;
}
