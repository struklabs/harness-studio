// Shared types, defaults and persistence for the Playground view.
import { thinkingBody, NO_REASONING, type ReasoningControl, type ThinkingLevel } from './thinking';

import type { InferencePreset } from '#lib/types.js';
import { invoke } from '#lib/ipc.js';
import { getToolSettings } from '#lib/tools-client.js';
import type { RagTurn } from './rag';
import type { PgImage } from './images';
import type { PgAudio } from './audio';
import type { TokenLogprob } from './logprobs';
import { sanitizeCompaction, type CompactionMeta, type CompactionSettings } from './compaction.svelte';
import { cleanRequest, DEFAULT_PRESET_ID, emptyRequest, requestBody, requestOf, sameRequest, type PresetRequest } from '../presets/request';

export interface ResponseStats {
  /** v2 stores promptN including cached input. */
  v?: number;
  /** "timings" = llama.cpp's own measurements; "usage" = OpenAI-style token counts only. */
  source: 'timings' | 'usage' | 'estimate' | 'none';
  predictedN?: number;
  predictedPerSecond?: number;
  predictedMs?: number;
  promptN?: number;
  promptPerSecond?: number;
  promptMs?: number;
  cachedN?: number;
  /** Speculative decoding: tokens the draft model proposed / the target accepted (`timings`). */
  draftN?: number;
  draftAcceptedN?: number;
  /** Client-side: request sent → first content/reasoning token received. */
  ttftMs?: number;
  /** Client-side: request sent → stream finished. */
  totalMs?: number;
}

/** Unversioned timings changed units on 5 October; persisted variants carry their creation time. */
export function normalizeStats(raw: unknown, createdAt?: number): ResponseStats | undefined {
  if (!raw || typeof raw !== 'object' || typeof (raw as ResponseStats).source !== 'string') return undefined;
  const stats = raw as ResponseStats;
  if (stats.v !== undefined && stats.v >= 2) return stats;
  const legacy = stats.v === 1 || createdAt === undefined || createdAt < Date.UTC(2026, 9, 5, 15, 43, 1);
  return {
    ...stats,
    v: 2,
    promptN: legacy && stats.source === 'timings'
      ? (stats.promptN ?? 0) + (stats.cachedN ?? 0)
      : stats.promptN,
  };
}

/** Playground's view of GET /tools: idle = not fetched (tools off / server stopped). */
export type ToolsState = 'idle' | 'loading' | 'ok' | 'disabled' | 'error';

export type ToolCallStatus =
  /** Streaming / queued. */
  | 'pending'
  /** Waiting for the user to approve or deny. */
  | 'approval'
  | 'running'
  | 'done'
  /** The tool ran but reported an error (or the arguments were invalid). */
  | 'error'
  | 'denied'
  /** Aborted by the user (Stop) or interrupted before it ran. */
  | 'stopped';

/** A tool call made by the assistant, with its execution result (sent back as a `tool` message). */
export interface PgToolCall {
  /** Normalized by the core parser from assistant content instead of native tool_calls. */
  recoveredFromText?: boolean;
  id: string;
  name: string;
  /** Raw JSON arguments string as produced by the model. */
  arguments: string;
  status: ToolCallStatus;
  /** Result text handed back to the model (already truncated). */
  result?: string;
  /** write permission / shell execution: needs explicit approval. */
  dangerous?: boolean;
  /** Core error code of a failed Harness tool call (`no_workspace`, `outside_workspace`...). */
  errorCode?: string;
  /** Approved automatically via "always allow in this session". */
  autoApproved?: boolean;
  /** Id of the core run (`tools_run` call id): what a tool that waits for the user is matched by. */
  runId?: string;
}

export interface ChatMessage {
  id: string;
  /** `compaction`: a context compaction marker (the summary is `content`, see compaction.svelte.ts). */
  role: 'user' | 'assistant' | 'compaction';
  content: string;
  reasoning?: string;
  /** First reasoning token to first answer token; omitted for old/unfinished replies. */
  reasoningMs?: number;
  stats?: ResponseStats;
  error?: string;
  stopped?: boolean;
  finishReason?: string;
  /** Per-slot context at request time; preserved when viewing the saved answer. */
  contextSize?: number;
  /** Tool calls requested in this assistant turn (tool loop). */
  toolCalls?: PgToolCall[];
  /** The tool loop stopped here because it hit the round limit. */
  toolLimitReached?: boolean;
  /** Pictures sent with a user message (stored in the variant's `meta.images`). */
  images?: PgImage[];
  audio?: PgAudio[];
  /** Transient; never persisted. */
  streaming?: boolean;
  /** Per-token probabilities of this answer (when requested); session memory only, see logprobs.ts. */
  logprobs?: TokenLogprob[];
  // ---- Conversation tree (state.db) ----
  /** Variant of this message that is shown (client-chosen id, also used when saving). */
  variantId?: string;
  /** Parent message on the current path (null = root). */
  parentId?: string | null;
  /** All variants of this message (regenerations), oldest first. */
  variantIds?: string[];
  variantIndex?: number;
  /** Messages sharing this message's parent (edited versions), oldest first. */
  siblingIds?: string[];
  siblingIndex?: number;
  /** Transient: this reply is a new variant of an existing message (regenerate). */
  newVariant?: boolean;
  /** Documents used for this answer (sources, stored in the variant's `meta.rag`). */
  rag?: RagTurn;
  /** Compaction marker (`role === 'compaction'`): what it summarized, undone or not. */
  compaction?: CompactionMeta;
  /**
   * Earlier rounds of this answer, oldest first; the message itself is the last round. Only a
   * split-view side keeps its tool loop this way (one stored variant per side, the earlier
   * rounds in `meta.rounds`); a normal answer stores every round as its own message.
   */
  priorRounds?: ChatMessage[];
  // ---- Split view ----
  /** This message is one side of a split turn. */
  compareSide?: 'a' | 'b';
  /** A split turn: both sides (side order); the message itself shows the active one. */
  compareSides?: ChatMessage[];
  /** A regenerated split turn: the answer pairs, oldest first, and the shown one. `ids` = side-A variant of each. */
  compareNav?: { index: number; count: number; ids: string[] };
  /** Stored `meta.compare` of a side (model, preset, whether it ran after the other side). */
  compareMeta?: {
    group: string;
    model: string;
    target: 'gateway' | 'server';
    server_id?: string | null;
    preset_id?: string | null;
    preset_name?: string | null;
    sequential?: boolean;
  };
}

export interface PgSettings {
  model: string;
  systemPrompt: string;
  /** Inference preset the request settings came from ('' = none). */
  presetId: string;
  /** Request-side settings (sampling, reasoning, output format); blank = not sent. */
  request: PresetRequest;
  thinking: ThinkingLevel;
  thinkingBudget: number | null;
  /** Offer tools to the model (Harness's own and its MCP servers' tools) and run the tool loop. */
  toolsEnabled: boolean;
  /** The user switched tools on/off themselves; otherwise the default (on unless the tool policy is off) applies.
   *  Settings saved before 10.10 carry `toolsEnabled: false` only because that was the old default. */
  toolsExplicit: boolean;
  /** Working directory of this conversation's file/shell tools ('' = the default from Settings > Tools). */
  /** Empty/missing follows the live default; harness://no-workspace explicitly opts out. */
  workdir: string;
  /** Tool names the user deselected. */
  disabledTools: string[];
  /** Ask the model for a short conversation title after the first answer. */
  autoTitle: boolean;
  /** Documents: embedding definition alias ('' = first available, 'none' = keywords only). */
  ragEmbedModel: string;
  /** Documents: rerank definition alias ('' = off). */
  ragRerankModel: string;
  /** Documents: excerpts per question. */
  ragTopK: number;
  /** This conversation's own compaction settings (null = the global defaults). */
  compaction: CompactionSettings | null;
}

export const DEFAULT_SETTINGS: PgSettings = {
  model: '',
  systemPrompt: '',
  presetId: DEFAULT_PRESET_ID,
  request: emptyRequest(),
  thinking: 'auto',
  thinkingBudget: null,
  toolsEnabled: true,
  toolsExplicit: false,
  workdir: '',
  disabledTools: [],
  autoTitle: true,
  ragEmbedModel: '',
  ragRerankModel: '',
  ragTopK: 5,
  compaction: null,
};

/** Old single-conversation storage (messages + settings); migrated into state.db once. */
export const LEGACY_STORAGE_KEY = 'harness_playground_v1';
/** Settings used for new conversations (the last ones used). */
export const DEFAULTS_KEY = 'harness_playground_settings_v2';

function isNum(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v);
}

/** Request body fields of the settings (llama-server names; blank values are omitted). */
export function samplingParams(s: PgSettings, opts: { tools: boolean; reasoningControl?: ReasoningControl } = { tools: false }): Record<string, unknown> {
  return thinkingBody(requestBody(s.request, opts), s.thinking, opts.reasoningControl ?? NO_REASONING, s.thinkingBudget);
}

/** Applies a preset: its request settings, and its system prompt when it has one. */
export function applyPreset(s: PgSettings, preset: InferencePreset | undefined, id: string) {
  s.presetId = id;
  s.request = requestOf(preset);
  if (preset?.system_prompt) s.systemPrompt = preset.system_prompt;
}

/** The settings no longer match their preset (shown as "changed", with a reset). */
export function presetModified(s: PgSettings, preset: InferencePreset | undefined): boolean {
  if (!preset) return false;
  if (preset.system_prompt && s.systemPrompt !== preset.system_prompt) return true;
  return !sameRequest(s.request, requestOf(preset));
}

/** earlier flat sampling keys of stored settings -> request fields. */
const LEGACY_KEYS: [string, keyof PresetRequest['sampling']][] = [
  ['temperature', 'temperature'],
  ['topP', 'top_p'],
  ['topK', 'top_k'],
  ['minP', 'min_p'],
  ['maxTokens', 'max_tokens'],
  ['repeatPenalty', 'repeat_penalty'],
  ['seed', 'seed'],
];

function isObject(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === 'object' && !Array.isArray(v);
}

export function newId(): string {
  return typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

export interface StoredState {
  messages: ChatMessage[];
  settings: PgSettings;
}

export function sanitizeSettings(raw: unknown): PgSettings {
  const s: PgSettings = { ...DEFAULT_SETTINGS, request: emptyRequest(), disabledTools: [] };
  if (!raw || typeof raw !== 'object') return s;
  const r = raw as Record<string, unknown>;
  if (typeof r.model === 'string') s.model = r.model;
  if (typeof r.thinking === 'string' && ['auto', 'off', 'light', 'normal', 'deep'].includes(r.thinking)) s.thinking = r.thinking as ThinkingLevel;
  if (isNum(r.thinkingBudget) && r.thinkingBudget >= -1) s.thinkingBudget = Math.trunc(r.thinkingBudget);
  if (typeof r.systemPrompt === 'string') s.systemPrompt = r.systemPrompt;
  if (typeof r.presetId === 'string') s.presetId = r.presetId;
  if (isObject(r.request)) {
    const req = r.request;
    s.request = cleanRequest({
      sampling: isObject(req.sampling) ? (req.sampling as PresetRequest['sampling']) : {},
      reasoning: isObject(req.reasoning) ? (req.reasoning as PresetRequest['reasoning']) : {},
      response_format: req.response_format,
      tool_choice: req.tool_choice,
    });
  } else {
    // Settings stored before presets: flat numbers, no preset.
    const sampling: Record<string, number> = {};
    for (const [from, to] of LEGACY_KEYS) if (isNum(r[from])) sampling[to] = r[from] as number;
    s.request = { sampling, reasoning: {} };
    if (typeof r.presetId !== 'string') s.presetId = Object.keys(sampling).length ? '' : DEFAULT_PRESET_ID;
  }
  s.toolsExplicit = r.toolsExplicit === true;
  s.toolsEnabled = s.toolsExplicit ? r.toolsEnabled === true : true;
  s.workdir = typeof r.workdir === 'string' ? r.workdir : '';
  // (`useServerBuiltinTools` of older saved settings is ignored: the llama-server `--tools` path is gone.)
  s.disabledTools = Array.isArray(r.disabledTools) ? r.disabledTools.filter((x): x is string => typeof x === 'string') : [];
  s.autoTitle = r.autoTitle !== false;
  if (typeof r.ragEmbedModel === 'string') s.ragEmbedModel = r.ragEmbedModel;
  if (typeof r.ragRerankModel === 'string') s.ragRerankModel = r.ragRerankModel;
  if (isNum(r.ragTopK)) s.ragTopK = Math.min(12, Math.max(1, Math.round(r.ragTopK)));
  s.compaction = isObject(r.compaction) ? sanitizeCompaction(r.compaction) : null;
  return s;
}

function sanitizeMessages(raw: unknown): ChatMessage[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter(
      (m): m is ChatMessage =>
        !!m && typeof m === 'object' && (m.role === 'user' || m.role === 'assistant') && typeof m.content === 'string'
    )
    .map((m) => ({
      ...m,
      id: typeof m.id === 'string' ? m.id : newId(),
      streaming: false,
      stats: normalizeStats(m.stats),
      toolCalls: sanitizeToolCalls((m as { toolCalls?: unknown }).toolCalls),
    }));
}

const FINAL_STATUSES: ToolCallStatus[] = ['done', 'error', 'denied', 'stopped'];

export function sanitizeToolCalls(raw: unknown): PgToolCall[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const calls = raw
    .filter((c): c is PgToolCall => !!c && typeof c === 'object' && typeof c.id === 'string' && typeof c.name === 'string')
    .map((c) => ({
      ...c,
      arguments: typeof c.arguments === 'string' ? c.arguments : '',
      result: typeof c.result === 'string' ? c.result : undefined,
      // A call that was pending/running when the app closed never finished.
      status: FINAL_STATUSES.includes(c.status) ? c.status : ('stopped' as const),
    }));
  return calls.length ? calls : undefined;
}

/** Reads the old single conversation, if it still needs to be migrated into state.db. */
export function loadLegacyState(): StoredState | null {
  try {
    const raw = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    return { messages: sanitizeMessages(parsed?.messages), settings: sanitizeSettings(parsed?.settings) };
  } catch {
    return null;
  }
}

/** Settings for new conversations: the last used ones, else the old storage's, else defaults. */
export async function loadDefaultSettings(): Promise<PgSettings> {
  try {
    const raw = await invoke<string | null>('get_playground_state', { key: DEFAULTS_KEY });
    if (!raw) {
      const tools = await getToolSettings();
      return { ...sanitizeSettings(null), toolsEnabled: tools.policy !== 'off' };
    }
    const parsed = JSON.parse(raw);
    const saved = sanitizeSettings(parsed?.settings ?? parsed);
    if (!saved.toolsExplicit) saved.toolsEnabled = (await getToolSettings()).policy !== 'off';
    // A conversation's own compaction settings stay with that conversation.
    return { ...saved, compaction: null };
  } catch {
    // If policy cannot be read, wait for the user to enable tools explicitly.
    return { ...sanitizeSettings(null), toolsEnabled: false };
  }
}

export async function saveDefaultSettings(settings: PgSettings) {
  try {
    await invoke<void>('set_playground_state', { key: DEFAULTS_KEY, value: JSON.stringify(settings) });
  } catch {
    // A conversation still stores its own snapshot; defaults are best-effort.
  }
}
