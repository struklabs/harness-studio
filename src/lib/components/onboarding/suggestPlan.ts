// Pure parts of the first-run suggestion logic (no app imports, so node can test them): which
// small-model listings to add when the top downloads do not fit this machine, and how the best
// candidate is picked.
import type { FitCategory } from '#lib/types.js';

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

/**
 * Repos that are no general chat model: code, math, embedding, reranking, vision-only, speech,
 * and reasoning distills / community fine-tunes / merges (slow and verbose for everyday chat).
 */
const NOT_GENERAL_CHAT = new RegExp(
  [
    'coder|code|math|embed|rerank|reward|guard|safety|vision|ocr|whisper|tts|diffusion|translat',
    '(^|[-_/])vl([-_]|$)|(^|[-_/])(base|pt)([-_]|$)',
    '(^|[-_/])r1([-_]|$)|distill|reason|think|abliterat|uncensor|roleplay|(^|[-_/])rp([-_]|$)|merge',
  ].join('|'),
  'i',
);

export function isGeneralChatRepo(repoId: string): boolean {
  return !NOT_GENERAL_CHAT.test(repoId);
}

/**
 * Official instruct releases, best first. The repo name must start with the family (after an
 * optional publisher prefix as bartowski writes it), so "DeepSeek-...-Qwen3" never matches.
 */
const ORG = '((qwen|google|meta-llama|mistralai|microsoft)[_-])?';
const FAMILIES: RegExp[] = [
  // The non-thinking Qwen3 releases; plain Qwen3 models think by default (slow, verbose).
  new RegExp(`^${ORG}qwen3-.*instruct-2507`, 'i'),
  new RegExp(`^${ORG}gemma-?[34]-.*it`, 'i'),
  new RegExp(`^${ORG}qwen3(?![-_.]?(coder|vl))`, 'i'),
  new RegExp(`^${ORG}(meta-)?llama-?3(\\.\\d)?-.*instruct`, 'i'),
  new RegExp(`^${ORG}(ministral|mistral).*instruct`, 'i'),
  new RegExp(`^${ORG}phi-?4-mini.*instruct`, 'i'),
];

/** Rank of the family of a repo (0 = Qwen3 Instruct-2507, 1 = Gemma, 2 = hybrid Qwen3 ... 5 = Phi-4-mini), -1 when none. */
export function familyRank(repoId: string): number {
  if (!isGeneralChatRepo(repoId)) return -1;
  const name = repoId.split('/').pop() ?? repoId;
  return FAMILIES.findIndex((r) => r.test(name));
}

export function isKnownChatFamily(repoId: string): boolean {
  return familyRank(repoId) >= 0;
}

/** Search terms (per trusted publisher) that bring the official releases into the pool. */
export const CHAT_FAMILY_QUERIES = ['qwen3-4b-instruct-2507', 'qwen3-8b', 'gemma-3-4b-it', 'gemma-3-12b-it', 'llama-3.2-3b-instruct', 'llama-3.1-8b-instruct', 'ministral', 'phi-4-mini-instruct'];

/** A first download should stay modest: a 4B-8B Q4_K_M is 2.5-5 GB. */
export const CHAT_COMFORT_BYTES = 6 * 1024 ** 3;

export interface ChatCandidate extends Pickable {
  repoId: string;
}

/**
 * The chat pick: official general instruct releases only (any other general repo only when none
 * of those exists). Of those that sit in the GPU (fully or tight) and stay under the comfort size,
 * the best family (Qwen3 Instruct-2507, Gemma, hybrid Qwen3, Llama, Mistral, Phi), then a full fit
 * before a tight one, then the largest file; otherwise the best fit tier, then the best family,
 * then the smallest file. Tight counts as comfortable: on a 4 GB card a 4B Q4_K_M is tight, yet
 * it loads all layers on the GPU (12k context) and beats a 1B model that fits with room to spare.
 */
export function pickChat<T extends ChatCandidate>(found: T[]): T | null {
  const general = found.filter((c) => isGeneralChatRepo(c.repoId));
  const known = general.filter((c) => isKnownChatFamily(c.repoId));
  const pool = known.length > 0 ? known : general;
  if (pool.length === 0) return null;
  const rank = (c: T) => (familyRank(c.repoId) < 0 ? 99 : familyRank(c.repoId));
  const comfortable = pool.filter(
    (c) => TIER_RANK[c.category] <= TIER_RANK.tight && c.sizeBytes <= CHAT_COMFORT_BYTES,
  );
  if (comfortable.length > 0) {
    return comfortable.reduce((a, b) => {
      if (rank(b) !== rank(a)) return rank(b) < rank(a) ? b : a;
      if (TIER_RANK[b.category] !== TIER_RANK[a.category]) return TIER_RANK[b.category] < TIER_RANK[a.category] ? b : a;
      return b.sizeBytes > a.sizeBytes ? b : a;
    });
  }
  return pool.reduce((a, b) => {
    if (TIER_RANK[b.category] !== TIER_RANK[a.category]) return TIER_RANK[b.category] < TIER_RANK[a.category] ? b : a;
    if (rank(b) !== rank(a)) return rank(b) < rank(a) ? b : a;
    return b.sizeBytes < a.sizeBytes ? b : a;
  });
}

/** MLX Whisper repos in mlx-audio's layout: large-v3-turbo, and small for little free memory. */
export const MLX_WHISPER_REPOS = ['mlx-community/whisper-large-v3-turbo-asr-fp16', 'mlx-community/whisper-small-asr-fp16'];
/** Free memory below which the small model is suggested (turbo's weights are 1.6 GB). */
export const MLX_TURBO_FREE_BYTES = 3 * 1024 ** 3;

/** Speech to text on Apple Silicon runs on MLX when its engine can be installed there (macOS
 *  new enough); elsewhere, and as the fallback, on whisper.cpp. */
export function asrEngine(hardware: { os?: string; arch?: string } | null, mlxOffered: boolean): 'mlx' | 'whisper.cpp' {
  return hardware?.os === 'macos' && hardware.arch === 'aarch64' && mlxOffered ? 'mlx' : 'whisper.cpp';
}

/** The MLX Whisper repo for this much free memory (unknown: the turbo model). */
export function mlxWhisperRepo(freeRam: number | null | undefined): string {
  return freeRam != null && freeRam < MLX_TURBO_FREE_BYTES ? MLX_WHISPER_REPOS[1] : MLX_WHISPER_REPOS[0];
}
