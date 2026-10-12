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

// --- Chat: a general assistant, not a specialist ---

/** Repos that are no general chat model: code, math, embedding, reranking, vision-only, speech... */
const NOT_GENERAL_CHAT =
  /coder|code|math|embed|rerank|reward|guard|safety|vision|ocr|whisper|tts|diffusion|translat|(^|[-_/])vl([-_]|$)|(^|[-_/])(base|pt)([-_]|$)/i;

export function isGeneralChatRepo(repoId: string): boolean {
  return !NOT_GENERAL_CHAT.test(repoId);
}

/** Well-known general instruct families a first suggestion prefers. */
const KNOWN_CHAT = [/qwen3/i, /gemma-?[34](?!n)/i, /llama-?3(\.\d)?.*instruct/i, /ministral|mistral.*instruct/i, /phi-?4-mini/i];

export function isKnownChatFamily(repoId: string): boolean {
  return isGeneralChatRepo(repoId) && KNOWN_CHAT.some((r) => r.test(repoId));
}

/** Search terms (per trusted publisher) that bring the known families into the pool. */
export const CHAT_FAMILY_QUERIES = ['qwen3-4b-instruct', 'qwen3-8b', 'gemma-3-4b-it', 'gemma-3-12b-it', 'llama-3.2-3b-instruct', 'llama-3.1-8b-instruct', 'ministral', 'phi-4-mini'];

/** A first download should stay modest: a 4B-8B Q4_K_M is 2.5-5 GB. */
export const CHAT_COMFORT_BYTES = 6 * 1024 ** 3;

export interface ChatCandidate extends Pickable {
  repoId: string;
}

/**
 * The chat pick: general models only, known families first. Of those that fit the GPU fully and
 * stay under the comfort size, the largest; else the best fit tier with the smallest file.
 */
export function pickChat<T extends ChatCandidate>(found: T[]): T | null {
  const general = found.filter((c) => isGeneralChatRepo(c.repoId));
  const known = general.filter((c) => isKnownChatFamily(c.repoId));
  const pool = known.length > 0 ? known : general;
  if (pool.length === 0) return null;
  const comfortable = pool.filter((c) => c.category === 'full_gpu' && c.sizeBytes <= CHAT_COMFORT_BYTES);
  if (comfortable.length > 0) return comfortable.reduce((a, b) => (b.sizeBytes > a.sizeBytes ? b : a));
  return pool.reduce((a, b) => {
    if (TIER_RANK[b.category] !== TIER_RANK[a.category]) return TIER_RANK[b.category] < TIER_RANK[a.category] ? b : a;
    return b.sizeBytes < a.sizeBytes ? b : a;
  });
}
