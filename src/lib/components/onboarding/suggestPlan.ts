// Pure parts of the first-run suggestion logic (no app imports, so node can test them): which
// small-model listings to add when the top downloads do not fit this machine, and how the best
// candidate is picked.
import type { FitCategory } from '$lib/types';

/** Fit tier order, best first. */
export const TIER_RANK: Record<FitCategory, number> = { full_gpu: 0, tight: 1, hybrid: 2, cpu_heavy: 3, oom_risk: 4 };

/** Parameter sizes the second wave searches for, smallest machines first. */
export const SMALL_SIZES = ['1B', '2B', '3B', '4B', '0.6B', '1.7B'];

/** Search terms of the small-model wave: `<size>` for chat, `coder-<size>` for code models. */
export function smallQueries(role: 'chat' | 'code'): string[] {
  return SMALL_SIZES.map((s) => (role === 'code' ? `coder-${s}` : s));
}

/**
 * Whether the top-downloads wave is good enough. The most downloaded repos are mostly big models;
 * on a small card or little RAM none of them fits, or only as a slow partial offload. Then the
 * small-model wave runs and its candidates join the pool.
 */
export function needsSmallModelWave(categories: FitCategory[]): boolean {
  return categories.length === 0 || categories.every((c) => TIER_RANK[c] > TIER_RANK.tight);
}

export interface Pickable {
  category: FitCategory;
  sizeBytes: number;
}

/**
 * The best candidate: a full fit beats a tight one (the context and the desktop need VRAM too),
 * then the biggest file. For code, `toolRank` (lower is better) decides first.
 */
export function pickBest<T extends Pickable>(found: T[], toolRank?: (c: T) => number): T | null {
  if (found.length === 0) return null;
  return found.reduce((a, b) => {
    if (toolRank && toolRank(a) !== toolRank(b)) return toolRank(b) < toolRank(a) ? b : a;
    if (TIER_RANK[b.category] !== TIER_RANK[a.category]) return TIER_RANK[b.category] < TIER_RANK[a.category] ? b : a;
    return b.sizeBytes > a.sizeBytes ? b : a;
  });
}
