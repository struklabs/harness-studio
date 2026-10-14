// "Prepare the server for this tool": agent tools (OpenCode, Open Design) send 30-40k-token
// prompts, sometimes two at once. They need a large context, flash attention (long prompts are
// several times slower without it) and the automatic concurrent-request count for agents (every
// concurrent request keeps an agent-sized share of the shared context; `harness_core::slots`).
import { invoke } from '#lib/ipc.js';
import type { KvLevel } from '#lib/types.js';
import { fetchWithTimeout } from './net';

/** The fields of the running server's launch config this feature reads. */
export interface RunningConfig {
  model_path: string;
  router_mode: boolean;
  gpu_layers: number;
  context_size: number;
  flash_attn: boolean;
  cache_type_k: string | null;
  cache_type_v: string | null;
  parallel_slots: number | null;
}

export type CacheType = 'f16' | 'q8_0' | 'q4_0';

/** One way to load the model (mirrors `harness_core::prepare::PrepareOption`). */
export interface PrepareOption {
  context_size: number;
  /** Conversation-memory precision (`kv_policy::KvLevel`) and its K/V types. */
  kv_level: KvLevel;
  cache_type_k: CacheType;
  cache_type_v: CacheType;
  /** null = everything on the GPU. */
  gpu_layers: number | null;
  vram_bytes: number;
  ram_bytes: number;
  percent: number;
  kind: 'chosen' | 'q8_kv' | 'f16_kv' | 'larger_ctx' | 'offload';
}

/** `q8_0` when K and V match, else `K q8_0 · V q4_0`. */
export function kvText(o: { cache_type_k: string; cache_type_v: string }): string {
  return o.cache_type_k === o.cache_type_v ? o.cache_type_k : `K ${o.cache_type_k} · V ${o.cache_type_v}`;
}

/** What "prepare the server" found (mirrors `harness_core::prepare::PreparePlan`). */
export interface PreparePlan {
  need: { min: number; recommended: number; ceiling: number };
  model: { trained: number; max: number; extended_by_rope: boolean; rope: string | null; known: boolean };
  verdict: 'recommended' | 'below_recommended' | 'below_minimum';
  limit: 'ceiling' | 'model' | 'memory' | 'available';
  available_memory: { total_vram_bytes: number; available_vram_bytes: number; headroom_bytes: number } | null;
  choice: PrepareOption | null;
  alternatives: PrepareOption[];
  budget_vram_bytes: number;
  budget_ram_bytes: number;
  cpu_only: boolean;
  /** Integrated GPU sharing system RAM. */
  shared_igpu?: boolean;
  suggest_smaller_model: boolean;
}

export interface TuningPlan {
  from: RunningConfig;
  /** The selectable options, best first (the choice, then the alternatives). */
  options: PrepareOption[];
  /** Index of the pre-selected option (-1 = none fits; the user has to pick one). */
  selected: number;
  /** Null when there is no single model to plan for (router mode) or nothing is known. */
  plan: PreparePlan | null;
}

export function getRunningConfig(serverId: string): Promise<RunningConfig | null> {
  return invoke<RunningConfig | null>('get_server_launch_config', { serverId });
}

/**
 * True when the running server already suits a tool that needs `minCtx`. Slots share one context
 * (`--kv-unified`), so any slot count gives a request the whole window.
 */
export function isTuned(cfg: RunningConfig, minCtx: number): boolean {
  return cfg.context_size >= minCtx && cfg.flash_attn;
}

/**
 * Asks the backend for the largest context the model supports and the GPU holds
 * (`harness_core::prepare`), with alternatives. Without a single model (router mode) the tool's
 * minimum with a q8_0 cache is all that can be said.
 */
export async function planTuning(
  cfg: RunningConfig,
  serverId: string,
  toolId: string,
  minCtx: number
): Promise<TuningPlan> {
  let plan: PreparePlan | null = null;
  if (!cfg.router_mode) {
    plan = await invoke<PreparePlan | null>('plan_server_prepare', { serverId, toolId, minCtx });
  }
  if (!plan) {
    const fallback: PrepareOption = {
      context_size: Math.max(cfg.context_size, minCtx),
      kv_level: 'balanced',
      cache_type_k: 'q8_0',
      cache_type_v: 'q8_0',
      gpu_layers: null,
      vram_bytes: 0,
      ram_bytes: 0,
      percent: 0,
      kind: 'chosen',
    };
    return { from: cfg, options: [fallback], selected: 0, plan: null };
  }
  const options = [...(plan.choice ? [plan.choice] : []), ...plan.alternatives];
  // A below-minimum choice is not pre-selected: the user decides between the alternatives.
  return { from: cfg, options, selected: plan.choice ? 0 : -1, plan };
}

/** Restarts the server with the option; the model preset is updated too. */
export function applyTuning(serverId: string, option: PrepareOption, from: RunningConfig): Promise<void> {
  return invoke<void>('tune_server_for_agent', {
    serverId,
    contextSize: option.context_size,
    cacheTypeK: option.cache_type_k,
    cacheTypeV: option.cache_type_v,
    // Everything on the GPU is what the memory estimate assumed; an already-full value stays.
    gpuLayers: option.gpu_layers ?? (from.gpu_layers >= 99 ? null : 999),
  });
}

/** Model id and context size the server reports right now (fresh, not the page's cached values). */
export async function servedModel(
  baseUrl: string,
  headers: Record<string, string>
): Promise<{ id: string; nCtx: number } | null> {
  const res = await fetchWithTimeout(`${baseUrl}/models`, { headers }, 5000);
  if (!res.ok) return null;
  const data = await res.json();
  const first: { id?: string; meta?: { n_ctx?: number } } | undefined = data?.data?.[0];
  return first?.id ? { id: first.id, nCtx: first.meta?.n_ctx ?? 0 } : null;
}
