// Splits a memory bar (VRAM or RAM) by who uses it. Pure: the numbers come from the backend
// (`MemoryBreakdown`: Harness Studio, each llama-server, "other" = used - the rest, floored at
// 0), nothing is estimated here. Segments are scaled down together if the per-process figures
// add up to more than the capacity (shared RAM pages are counted in several processes).

import type { MemoryBreakdown } from './types';

export type SegmentKind = 'harness' | 'server' | 'other';

export interface Segment {
  key: string;
  kind: SegmentKind;
  /** Server id for `kind: 'server'`. */
  serverId?: string;
  /** Model file name of the server run (for the legend). */
  model?: string;
  bytes: number;
  /** Share of the capacity, 0..100, after scaling. */
  pct: number;
}

export interface SegmentedBar {
  segments: Segment[];
  /** Capacity not used by anyone. */
  freeBytes: number;
  freePct: number;
}

export function buildSegments(used: number, total: number, b: MemoryBreakdown): SegmentedBar {
  const parts: Omit<Segment, 'pct'>[] = [
    { key: 'harness', kind: 'harness', bytes: b.harness_bytes },
    ...b.servers.map((s) => ({ key: `server:${s.id}`, kind: 'server' as const, serverId: s.id, model: s.name, bytes: s.bytes })),
    { key: 'other', kind: 'other', bytes: b.other_bytes },
  ];
  const sum = parts.reduce((a, p) => a + p.bytes, 0);
  // Only when the parts overshoot the capacity: scale them to fit; otherwise they are exact.
  const scale = total > 0 && sum > total ? total / sum : 1;
  const segments = parts.map((p) => ({ ...p, pct: total > 0 ? ((p.bytes * scale) / total) * 100 : 0 }));
  const freeBytes = Math.max(0, total - used);
  const freePct = total > 0 ? Math.max(0, 100 - segments.reduce((a, s) => a + s.pct, 0)) : 0;
  return { segments, freeBytes, freePct };
}

/** Share of `bytes` in `total` as shown in a legend ("12.4%", "<0.1%"). */
export function legendPct(bytes: number, total: number): string {
  if (total <= 0 || bytes <= 0) return '0%';
  const p = (bytes / total) * 100;
  return p < 0.1 ? '<0.1%' : `${p.toFixed(1)}%`;
}
