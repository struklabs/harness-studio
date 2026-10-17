import type { FitResult, HfCompatibility, HfModelSearchResult, HfModelVariant } from '#lib/types.js';
import { isCompanionVariant, recommendFloatVariant, recommendVariant } from './recommend.ts';

export type HubFitTier = 'full' | 'partial' | 'unknown' | 'unavailable';
export interface HubFit {
  tier: HubFitTier;
  quant?: string;
}

export function unavailable(compat: HfCompatibility): boolean {
  return compat.status === 'unsupported' || compat.status === 'unsupported_platform';
}

export function repoUnavailable(repo: HfModelSearchResult): boolean {
  return unavailable(repo.compatibility) || ['unsupported_architecture', 'foreign_speech_gguf', 'apple_only',
    'pipeline_config_only', 'component_only', 'needs_own_package', 'unsupported_weight_layout'].includes(repo.not_runnable ?? '');
}

export function hasCompleteSize(v: HfModelVariant): boolean {
  return v.total_size_bytes > 0 && (!v.is_split || (v.shards.length > 0
    && v.shards.length === v.shards[0].total_shards
    && new Set(v.shards.map(s => s.shard_index)).size === v.shards.length
    && v.shards.every(s => s.size_bytes > 0 && s.shard_index >= 1 && s.shard_index <= s.total_shards)));
}

/** Use the estimator's actual memory split, including context and both memory budgets. */
export function fitTier(result: FitResult): HubFitTier {
  if (!result.budget.ram_bytes) return 'unknown';
  if (result.category === 'oom_risk') return 'unavailable';
  return result.budget.vram_bytes > 0 && result.estimate.model_gpu_bytes > 0
    && result.estimate.model_host_bytes === 0 ? 'full' : 'partial';
}

/** A companion cannot make an otherwise un-runnable repository look like a fit. */
export function bestRepoFit(variants: HfModelVariant[], fits: Map<string, FitResult>): HubFit {
  const mains = variants.filter(v => !v.not_runnable && !isCompanionVariant(v));
  const categories = new Map([...fits].map(([name, r]) => [name, r.category]));
  const known = mains.filter(v => fits.has(v.name) && fitTier(fits.get(v.name)!) !== 'unknown');
  const rank: Record<HubFitTier, number> = { full: 0, partial: 1, unknown: 2, unavailable: 3 };
  known.sort((a, b) => rank[fitTier(fits.get(a.name)!)] - rank[fitTier(fits.get(b.name)!)]);
  const tier = known[0] ? fitTier(fits.get(known[0].name)!) : 'unknown';
  // An incomplete estimate must not turn an unexamined alternative into a refusal.
  if (tier === 'unavailable' && mains.some(v => !fits.has(v.name))) return { tier: 'unknown' };
  const pool = known.filter(v => fitTier(fits.get(v.name)!) === tier);
  const choice = recommendVariant(pool, categories) ?? recommendFloatVariant(pool, categories) ?? pool[0];
  return { tier, quant: choice?.quantization };
}

/** Tier first, download popularity second; equal counts keep the original listing order. */
export function rankHardware(repos: HfModelSearchResult[], fits: ReadonlyMap<string, HubFit>): HfModelSearchResult[] {
  const rank: Record<HubFitTier, number> = { full: 0, partial: 1, unknown: 2, unavailable: 3 };
  const tier = (r: HfModelSearchResult) => repoUnavailable(r) ? 'unavailable' : fits.get(r.id)?.tier ?? 'unknown';
  return [...repos].sort((a, b) => rank[tier(a)] - rank[tier(b)] || b.downloads - a.downloads);
}
