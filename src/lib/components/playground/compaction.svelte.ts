// Context compaction (harness_core::compaction): settings, the marker kept in a conversation,
// the trigger rule and `invoke` wrappers. The summary request itself runs in Rust; the request
// builder (`buildApiMessages`) applies the active marker the same way `request_messages` does.

import { invoke } from '#lib/ipc.js';
import type { ConversationView } from './conversations';

/** Auto-compaction and summary settings (global defaults, or a conversation's own). */
export interface CompactionSettings {
  /** Compact automatically before a message when the threshold is reached. */
  auto: boolean;
  /** Threshold as a share of the model's context, or as a fixed token count. */
  mode: 'percent' | 'tokens';
  percent: number;
  tokens: number;
  /** Newest turns kept verbatim (a turn = a user message and its answer, tool rounds included). */
  keepTurns: number;
  /** Model that writes the summary ('' = the chat model). */
  summaryModel: string;
  /** Shorten long tool results before summarizing them. */
  clearToolResults: boolean;
  /** Summary size in tokens (0 = 1/8 of the context, 256–2048). */
  summaryTokens: number;
  /** Standing instruction for every summary of this scope ('' = none). */
  instruction: string;
}

/**
 * Defaults (see docs/en/ARCHITECTURE.md, "Context compaction"): 80 % leaves the summary request
 * room in the same context (local models summarize with the model they chat with) and the
 * next answer room before llama-server's limit; two kept turns keep the thread of the
 * conversation verbatim; tool results are the cheapest thing to drop.
 */
export const DEFAULT_COMPACTION: CompactionSettings = {
  auto: true,
  mode: 'percent',
  percent: 80,
  tokens: 16000,
  keepTurns: 2,
  summaryModel: '',
  clearToolResults: true,
  summaryTokens: 0,
  instruction: '',
};

export const PERCENT_MIN = 50;
export const PERCENT_MAX = 95;
export const TOKENS_MIN = 1000;
export const KEEP_MAX = 10;
export const SUMMARY_TOKENS_MAX = 8192;

function clampInt(v: unknown, min: number, max: number, fallback: number): number {
  const n = typeof v === 'number' && Number.isFinite(v) ? Math.round(v) : fallback;
  return Math.min(max, Math.max(min, n));
}

export function sanitizeCompaction(raw: unknown): CompactionSettings {
  const r = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  const d = DEFAULT_COMPACTION;
  return {
    auto: typeof r.auto === 'boolean' ? r.auto : d.auto,
    mode: r.mode === 'tokens' ? 'tokens' : 'percent',
    percent: clampInt(r.percent, PERCENT_MIN, PERCENT_MAX, d.percent),
    tokens: clampInt(r.tokens, TOKENS_MIN, 10_000_000, d.tokens),
    keepTurns: clampInt(r.keepTurns, 0, KEEP_MAX, d.keepTurns),
    summaryModel: typeof r.summaryModel === 'string' ? r.summaryModel : d.summaryModel,
    clearToolResults: typeof r.clearToolResults === 'boolean' ? r.clearToolResults : d.clearToolResults,
    summaryTokens: r.summaryTokens === 0 ? 0 : clampInt(r.summaryTokens, 0, SUMMARY_TOKENS_MAX, d.summaryTokens),
    instruction: typeof r.instruction === 'string' ? r.instruction : d.instruction,
  };
}

// ---- Global defaults (every conversation without its own settings) ----

export const COMPACTION_DEFAULTS_KEY = 'harness_compaction_defaults_v1';

async function loadGlobal(): Promise<CompactionSettings> {
  try {
    const raw = await invoke<string | null>('get_playground_state', { key: COMPACTION_DEFAULTS_KEY });
    return sanitizeCompaction(raw ? JSON.parse(raw) : null);
  } catch {
    return { ...DEFAULT_COMPACTION };
  }
}

/** The global compaction defaults (reactive; saved when changed through `saveGlobalCompaction`). */
export const compactionDefaults = $state<{ value: CompactionSettings }>({ value: { ...DEFAULT_COMPACTION } });

export async function initGlobalCompaction() {
  compactionDefaults.value = await loadGlobal();
}

export async function saveGlobalCompaction(next: CompactionSettings) {
  compactionDefaults.value = sanitizeCompaction(next);
  try {
    await invoke<void>('set_playground_state', {
      key: COMPACTION_DEFAULTS_KEY,
      value: JSON.stringify(compactionDefaults.value),
    });
  } catch {
    // Conversation overrides remain stored; global defaults are best-effort.
  }
}

// ---- Trigger ----

/** Token count from which compaction triggers (a fixed count never exceeds 95 % of the context). */
export function thresholdTokens(s: CompactionSettings, ctx: number): number {
  if (s.mode === 'tokens') return Math.max(1, Math.min(s.tokens, Math.floor(ctx * 0.95)));
  return Math.floor((ctx * Math.min(98, Math.max(10, s.percent))) / 100);
}

/** Rough token count of a text (≈ 3 characters per token, like the backend's estimate). */
export function estimateTokens(text: string): number {
  return Math.ceil([...text].length / 3);
}

// ---- Marker ----

/** `meta.compaction` of a marker message (mirrors `harness_core::compaction::Marker`). */
export interface CompactionMeta {
  version: number;
  boundary_id: string;
  covered: number;
  turns: number;
  kept_turns: number;
  trigger: 'manual' | 'auto';
  strategy: string;
  instruction?: string | null;
  model?: string | null;
  tokens_before: number;
  tokens_after: number;
  summary_tokens: number;
  tool_results_cleared?: number;
  exact_tokens?: boolean;
  previous_id?: string | null;
  undone?: boolean;
  /** Made between two tool rounds of a running answer. */
  mid_turn?: boolean;
  /** The running turn's user message: before the boundary, still sent verbatim after the summary. */
  pinned_id?: string | null;
  created_at: number;
}

export function compactionOf(meta: unknown): CompactionMeta | undefined {
  const c = (meta as { compaction?: unknown } | null | undefined)?.compaction as Partial<CompactionMeta> | undefined;
  if (!c || typeof c !== 'object' || typeof c.boundary_id !== 'string') return undefined;
  return c as CompactionMeta;
}

/** What the request builder needs of a message list. */
interface MarkerLike {
  id: string;
  role: string;
  content: string;
  compaction?: CompactionMeta;
}

export interface ActiveCompaction {
  markerIndex: number;
  boundaryIndex: number;
  /** Index of the pinned user message (before the boundary), if the marker has one. */
  pinnedIndex: number | null;
  summary: string;
  meta: CompactionMeta;
}

/** The newest marker that is not undone and whose boundary is before it (as in Rust). */
export function activeCompaction(messages: readonly MarkerLike[]): ActiveCompaction | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role !== 'compaction' || !m.compaction || m.compaction.undone) continue;
    const boundary = m.compaction.boundary_id;
    const b = messages.slice(0, i).findIndex((x) => x.id === boundary);
    if (b >= 0) {
      const pinned = m.compaction.pinned_id ? messages.slice(0, b).findIndex((x) => x.id === m.compaction?.pinned_id) : -1;
      return { markerIndex: i, boundaryIndex: b, pinnedIndex: pinned >= 0 ? pinned : null, summary: m.content, meta: m.compaction };
    }
  }
  return null;
}

/** The newest marker was undone: automatic compaction waits (it would redo it right away). */
export function autoPaused(messages: readonly MarkerLike[]): boolean {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role === 'compaction' && m.compaction) return !!m.compaction.undone;
  }
  return false;
}

/** The summary as part of the system message (same text as `summary_block` in Rust). */
export function summaryBlock(summary: string): string {
  return (
    '<conversation_summary>\nThe earlier part of this conversation was compacted to save context. ' +
    'Summary of those messages (treat it as what was said; the newest messages follow verbatim):\n\n' +
    `${summary.trim()}\n</conversation_summary>`
  );
}

// ---- API ----

export interface CompactEndpoint {
  base_url: string;
  headers: Record<string, string>;
  model?: string | null;
}

export interface CompactRequest {
  conversation_id: string;
  leaf_id?: string | null;
  endpoint: CompactEndpoint;
  tokenize?: { host_url: string; headers: Record<string, string> } | null;
  n_ctx: number;
  summary_ctx?: number | null;
  keep_turns: number;
  clear_tool_results: boolean;
  summary_tokens?: number | null;
  instruction?: string | null;
  trigger: 'manual' | 'auto';
  /** Between two tool rounds of a running answer (the path ends with the running turn). */
  mid_turn?: boolean;
  documents: string[];
  system_prompt: string;
}

export interface CompactOutcome {
  view: ConversationView;
  marker_id: string;
  marker: CompactionMeta;
  requests: number;
}

export const compactionApi = {
  compact: (request: CompactRequest) => invoke<CompactOutcome>('compact_conversation', { request }),
  cancel: (conversationId: string) => invoke<boolean>('cancel_compaction', { conversationId }),
  setUndone: (messageId: string, undone: boolean) =>
    invoke<ConversationView>('set_compaction_undone', { messageId, undone }),
  countTokens: (tokenize: { host_url: string; headers: Record<string, string> } | null, body: Record<string, unknown>) =>
    invoke<[number, boolean]>('count_playground_tokens', { tokenize, body }),
};

/** `/compact [instruction]` (also `/sıkıştır`) typed in the composer. */
export function parseCompactCommand(text: string): { instruction: string } | null {
  const m = /^\/(compact|sıkıştır|sikistir)(?:\s+([\s\S]*))?$/iu.exec(text.trim());
  return m ? { instruction: (m[2] ?? '').trim() } : null;
}
