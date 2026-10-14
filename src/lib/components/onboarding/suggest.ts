// Purpose-based suggestions of the first-run wizard. Everything here is real data: the Hugging Face
// listing and file sizes from the Hub backend, the Rust memory estimate for this machine, the
// installed runtimes. The pick logic reuses the Hub's (`hub/recommend.ts`).
import { findAgentModel, listAgentModels, type AgentModel } from '../connections/agentModels';
import { codeToolEvidence, codeToolRank, type CodeToolEvidence } from './purposeCapabilities';
import { invoke } from '#lib/ipc.js';
import { errorCode, rawError } from '#lib/errors.js';
import { defaultLoadFit } from '#lib/fit.js';
import { formatBytes } from '#lib/format.js';
import type { TKey } from '#lib/i18n.svelte.js';
import type { FitCategory, HfModelSearchResult, HfModelVariant, HfSearchOptions, RuntimeReleasePick, RuntimeView, SystemHardwareInfo } from '#lib/types.js';
import { fetchPage } from '../hub/hubData';
import { quantBits, recommendVariant, recommendWhisperVariant, whisperFitsMemory, whisperOnCpu } from '../hub/recommend';
import { asrEngine, CHAT_FAMILY_QUERIES, isGeneralChatRepo, mlxWhisperRepo, needsSmallModelWave, pickBest, pickChat, smallQueries } from './suggestPlan';
import { engineInstall } from '#lib/stores/engineInstall.svelte.js';

export type Purpose = 'chat' | 'code' | 'rag' | 'asr';

export const PURPOSES: Purpose[] = ['chat', 'code', 'rag', 'asr'];

/** The whisper.cpp ggml models live in this repo (`ggml-small-q8_0.bin`, ...). */
export const WHISPER_REPO = 'ggerganov/whisper.cpp';

/** What a model suggestion is for; names the Hub listing it comes from. */
export type ListedRole = 'chat' | 'code' | 'embedding' | 'reranker';

export interface Reason {
  key: TKey;
  params?: Record<string, string | number>;
}

export interface ModelSuggestion {
  id: string;
  kind: 'model';
  purpose: Purpose;
  role: ListedRole | 'asr';
  repoId: string;
  variant: HfModelVariant;
  toolEvidence?: CodeToolEvidence;
  sizeBytes: number;
  reason: Reason;
  /** False when the required runtime cannot be installed on this platform. */
  defaultSelected?: boolean;
}

/** The whisper.cpp runtime (`family: whisper.cpp`), downloaded from its GitHub release. */
export interface RuntimeSuggestion {
  id: string;
  kind: 'runtime';
  purpose: Purpose;
  pick: RuntimeReleasePick;
  /** Real size of the archive(s) from the release listing. */
  sizeBytes: number;
  reason: Reason;
}

/** An engine installed into its own Python environment (mlx-audio, for MLX Whisper). */
export interface EngineSuggestion {
  id: string;
  kind: 'engine';
  purpose: Purpose;
  family: 'mlx-audio';
  /** The environment's download size as the backend estimates it. */
  sizeBytes: number;
  reason: Reason;
}

export type Suggestion = ModelSuggestion | RuntimeSuggestion | EngineSuggestion;

export function suggestionSelected(s: Suggestion, unchecked?: boolean): boolean {
  return !(unchecked ?? (s.kind === 'model' && s.defaultSelected === false));
}

/** Hub listing per role: tag + sort are the Hub's own, `query` narrows code models. */
const LISTINGS: Record<ListedRole, HfSearchOptions> = {
  chat: { query: '', pipeline_tag: 'text-generation', sort: 'downloads', limit: 12 },
  code: { query: 'coder', pipeline_tag: 'text-generation', sort: 'downloads', limit: 12 },
  embedding: { query: '', pipeline_tag: 'feature-extraction', sort: 'downloads', limit: 12 },
  reranker: { query: '', pipeline_tag: 'text-ranking', sort: 'downloads', limit: 12 },
};

/**
 * Chat and code suggestions come from these publishers' GGUF builds of the official models: the
 * open listing's top repos include community fine-tunes (uncensored merges, role-play models) that
 * are no first suggestion.
 */
const TRUSTED_AUTHORS = ['unsloth', 'bartowski', 'lmstudio-community', 'ggml-org'];
const TRUSTED_ROLES = new Set<ListedRole>(['chat', 'code']);
const PER_AUTHOR = 6;

/** Repo names a first suggestion skips in any listing. */
const SKIPPED_NAMES = /abliterat|uncensor|heretic|nsfw|erotic|roleplay|(^|[-_])rp([-_]|$)/i;

const ROLES_OF: Record<Exclude<Purpose, 'asr'>, ListedRole[]> = {
  chat: ['chat'],
  code: ['code'],
  rag: ['embedding', 'reranker'],
};

/** Reason line of a fit result: what memory budget the file sits in. */
function fitReason(category: FitCategory, budget: { vram_bytes: number; shared_igpu?: boolean; unified: boolean }): Reason {
  const mem = formatBytes(budget.vram_bytes, 0);
  const shared = budget.unified || !!budget.shared_igpu;
  if (category === 'full_gpu') {
    return shared ? { key: 'onboarding.sg_reason_shared', params: { mem } } : { key: 'onboarding.sg_reason_vram', params: { mem } };
  }
  if (category === 'tight') return { key: 'onboarding.sg_reason_tight', params: { mem } };
  if (category === 'hybrid') return { key: 'onboarding.sg_reason_hybrid' };
  return { key: 'onboarding.sg_reason_cpu' };
}

interface Candidate {
  repo: HfModelSearchResult;
  variant: HfModelVariant;
  category: FitCategory;
  reason: Reason;
}

/** The recommended file of one repo with its fit, or null (gated, not runnable, nothing sized). */
async function evaluate(repo: HfModelSearchResult, ctx: number, role?: ListedRole): Promise<Candidate | null> {
  // A repo whose runtime has no build here (ONNX on an Intel Mac) is never a first suggestion.
  if (repo.gated || repo.not_runnable || repo.compatibility.status === 'unsupported_platform') return null;
  const variants = await invoke<HfModelVariant[]>('get_hf_model_variants', { repoId: repo.id });
  // Below 4 bits quality drops fast: a smaller model at Q4+ is the better first suggestion.
  let sized = variants.filter((v) => v.total_size_bytes > 0 && quantBits(v.quantization) >= 4);
  // A general chat model is downloaded as Q4_K_M when there is one: the usual size/quality point.
  const q4 = sized.filter((v) => /^q4_k_m$/i.test(v.quantization));
  if (role === 'chat' && q4.length > 0) sized = q4;
  // As a default load starts it, like the Hub's variant table.
  const results = await Promise.all(sized.map((v) => defaultLoadFit({ model_size_bytes: v.total_size_bytes, context_size: ctx, gpu_layers: 999 })));
  const fits = new Map<string, FitCategory>();
  const reasons = new Map<string, Reason>();
  sized.forEach((v, i) => {
    fits.set(v.name, results[i].category);
    reasons.set(v.name, fitReason(results[i].category, results[i].budget));
  });
  const variant = recommendVariant(sized, fits);
  if (!variant) return null;
  const category = fits.get(variant.name) as FitCategory;
  if (category === 'oom_risk') return null;
  return { repo, variant, category, reason: reasons.get(variant.name) as Reason };
}

/**
 * The model suggestion of one role. The Hub listing's top repos are each reduced to the file that
 * suits this machine; of those that fit in the GPU the biggest file wins (best quality that still
 * fits), else the best fit tier there is.
 */
async function suggestModel(purpose: Purpose, role: ListedRole, ctx: number): Promise<ModelSuggestion | null> {
  const [repos, agents] = await Promise.all([
    listRepos(role),
    role === 'code' ? listAgentModels() : Promise.resolve([] as AgentModel[]),
  ]);
  const settled = await Promise.allSettled(repos.map((r) => evaluate(r, ctx, role)));
  const found = settled.flatMap((s) => (s.status === 'fulfilled' && s.value ? [s.value] : []));
  // The most downloaded repos are mostly big models: when none of them fits well here (a 4 GB
  // card, little RAM), small models of the same publishers join the pool.
  if (role === 'chat') {
    // The known general families, searched by name: the downloads ranking alone is full of
    // coder, math and vision repos.
    const have = new Set(repos.map((r) => r.id));
    const named = (await listFamilyRepos()).filter((r) => !have.has(r.id));
    const more = await Promise.allSettled(named.map((r) => evaluate(r, ctx, role)));
    for (const m of more) if (m.status === 'fulfilled' && m.value) found.push(m.value);
  }
  if (TRUSTED_ROLES.has(role) && needsSmallModelWave(found.map((c) => c.category))) {
    const have = new Set(repos.map((r) => r.id));
    const small = (await listSmallRepos(role as 'chat' | 'code')).filter((r) => !have.has(r.id));
    const more = await Promise.allSettled(small.map((r) => evaluate(r, ctx, role)));
    for (const s of more) if (s.status === 'fulfilled' && s.value) found.push(s.value);
  }
  if (found.length === 0) {
    // Every lookup failed: a network problem, not "no suggestion".
    const failed = settled.find((s): s is PromiseRejectedResult => s.status === 'rejected');
    if (failed && settled.every((s) => s.status === 'rejected')) throw failed.reason;
    return null;
  }
  const evidence = (c: Candidate) => codeToolEvidence(findAgentModel(agents, c.variant.shards[0]?.filename ?? c.variant.name));
  const sized = found.map((c) => ({ ...c, repoId: c.repo.id, sizeBytes: c.variant.total_size_bytes }));
  const best = (role === 'chat' ? pickChat(sized) : pickBest(sized, role === 'code' ? (c) => codeToolRank(evidence(c)) : undefined)) as Candidate | null;
  if (!best) return null;
  return {
    id: `${role}:${best.repo.id}:${best.variant.name}`,
    kind: 'model',
    purpose,
    role,
    repoId: best.repo.id,
    variant: best.variant,
    toolEvidence: role === 'code' ? evidence(best) : undefined,
    sizeBytes: best.variant.total_size_bytes,
    reason: best.reason,
  };
}

/** Repos of the well-known general chat families, by name, from the trusted publishers. */
async function listFamilyRepos(): Promise<HfModelSearchResult[]> {
  const authors = TRUSTED_AUTHORS.slice(0, 3);
  const pages = await Promise.allSettled(
    authors.flatMap((author) => CHAT_FAMILY_QUERIES.map((query) => fetchPage({ ...LISTINGS.chat, query, author, limit: 2 }))),
  );
  const seen = new Set<string>();
  return pages
    .flatMap((p) => (p.status === 'fulfilled' ? p.value.items : []))
    .filter((r) => isGeneralChatRepo(r.id) && !SKIPPED_NAMES.test(r.id) && !seen.has(r.id) && !!seen.add(r.id));
}

/** Small models (1B to 4B) of the trusted publishers; a failed lookup just adds nothing. */
async function listSmallRepos(role: 'chat' | 'code'): Promise<HfModelSearchResult[]> {
  const pages = await Promise.allSettled(
    TRUSTED_AUTHORS.flatMap((author) =>
      smallQueries(role).map((query) => fetchPage({ ...LISTINGS[role], query, author, limit: 3 })),
    ),
  );
  const seen = new Set<string>();
  return pages
    .flatMap((p) => (p.status === 'fulfilled' ? p.value.items : []))
    .filter((r) => !SKIPPED_NAMES.test(r.id) && !seen.has(r.id) && !!seen.add(r.id));
}

/** The repos a role's suggestion is chosen from (trusted publishers for chat and code). */
async function listRepos(role: ListedRole): Promise<HfModelSearchResult[]> {
  const pages = TRUSTED_ROLES.has(role)
    ? await Promise.all(TRUSTED_AUTHORS.map((author) => fetchPage({ ...LISTINGS[role], author, limit: PER_AUTHOR })))
    : [await fetchPage(LISTINGS[role])];
  const seen = new Set<string>();
  return pages
    .flatMap((p) => p.items)
    .filter((r) => !SKIPPED_NAMES.test(r.id) && !seen.has(r.id) && !!seen.add(r.id));
}

/** The whisper ggml model sized for this machine's free memory. */
async function suggestWhisperModel(hardware: SystemHardwareInfo | null): Promise<ModelSuggestion | null> {
  const variants = await invoke<HfModelVariant[]>('get_hf_model_variants', { repoId: WHISPER_REPO });
  const ram = hardware?.available_ram_bytes;
  const variant = recommendWhisperVariant(variants, ram, whisperOnCpu(hardware?.os));
  if (!variant || whisperFitsMemory(variant.total_size_bytes, ram) !== true) return null;
  return {
    id: `asr:${WHISPER_REPO}:${variant.name}`,
    kind: 'model',
    purpose: 'asr',
    role: 'asr',
    repoId: WHISPER_REPO,
    variant,
    sizeBytes: variant.total_size_bytes,
    reason: { key: 'onboarding.sg_reason_whisper', params: { ram: formatBytes(ram, 0) } },
  };
}

/** The whisper.cpp runtime release when none is installed yet; null when one is. */
async function suggestWhisperRuntime(): Promise<RuntimeSuggestion | null> {
  const installed = await invoke<RuntimeView[]>('list_runtimes');
  if (installed.some((r) => r.entry?.family === 'whisper.cpp')) return null;
  const pick = await invoke<RuntimeReleasePick>('preview_runtime_release', {
    query: { family: 'whisper.cpp', repo: 'ggml-org/whisper.cpp', tag: null, backend: null, asset: null },
  });
  // An archive without a published checksum needs a manual confirmation: Settings > Runtime.
  if (pick.unverified.length > 0) return null;
  return {
    id: `asr:runtime:${pick.tag}`,
    kind: 'runtime',
    purpose: 'asr',
    pick,
    sizeBytes: pick.assets.reduce((n, a) => n + a.size, 0),
    reason: { key: 'onboarding.sg_reason_runtime' },
  };
}

/** The mlx-audio engine when it is not installed yet; null when it is. */
function suggestMlxEngine(): EngineSuggestion | null {
  if (engineInstall.installed('mlx-audio')) return null;
  return {
    id: 'asr:engine:mlx-audio',
    kind: 'engine',
    purpose: 'asr',
    family: 'mlx-audio',
    sizeBytes: engineInstall.pyEnv('mlx-audio')?.estimated_size_bytes ?? 0,
    reason: { key: 'onboarding.sg_reason_mlx_engine' },
  };
}

/** An MLX Whisper model (a folder of safetensors; one variant) sized for the free memory. */
async function suggestMlxWhisper(hardware: SystemHardwareInfo | null): Promise<ModelSuggestion | null> {
  const repoId = mlxWhisperRepo(hardware?.available_ram_bytes);
  const variants = await invoke<HfModelVariant[]>('get_hf_model_variants', { repoId });
  const variant = variants[0];
  if (!variant) return null;
  return {
    id: `asr:${repoId}`,
    kind: 'model',
    purpose: 'asr',
    role: 'asr',
    repoId,
    variant,
    sizeBytes: variant.total_size_bytes,
    reason: { key: 'onboarding.sg_reason_mlx_whisper' },
  };
}

/** Speech to text: MLX Whisper on Apple Silicon, else whisper.cpp and its ggml model. */
async function asrJobs(hardware: SystemHardwareInfo | null): Promise<Promise<Suggestion | null>[]> {
  let mlx = false;
  if (asrEngine(hardware, true) === 'mlx') {
    await engineInstall.refresh().catch(() => undefined);
    mlx = asrEngine(hardware, engineInstall.offered('mlx-audio')) === 'mlx';
  }
  return mlx
    ? [Promise.resolve(suggestMlxEngine()), suggestMlxWhisper(hardware)]
    : [suggestWhisperRuntime(), suggestWhisperModel(hardware)];
}

export interface PurposeResult {
  suggestions: Suggestion[];
  /** A lookup that failed (offline, rate limited): shown instead of a silent gap. */
  error: string | null;
  note: string | null;
}

/** All suggestions of one purpose; lookups of different roles run in parallel. */
export async function suggestFor(purpose: Purpose, hardware: SystemHardwareInfo | null, ctx: number): Promise<PurposeResult> {
  const jobs: Promise<Suggestion | null>[] =
    purpose === 'asr'
      ? await asrJobs(hardware)
      : ROLES_OF[purpose].map((role) => suggestModel(purpose, role, ctx));
  const settled = await Promise.allSettled(jobs);
  const suggestions = settled.flatMap((s) => (s.status === 'fulfilled' && s.value ? [s.value] : []));
  const failures = settled.filter((s): s is PromiseRejectedResult => s.status === 'rejected');
  const unavailable = purpose === 'asr'
    ? failures.find((s) => errorCode(s.reason) === 'runtime.whisper_no_macos')
    : undefined;
  if (unavailable) {
    for (const s of suggestions) {
      if (s.kind === 'model' && s.role === 'asr') s.defaultSelected = false;
    }
  }
  const failed = failures.find((s) => s !== unavailable);
  return { suggestions, error: failed ? rawError(failed.reason) : null, note: unavailable ? rawError(unavailable.reason) : null };
}
