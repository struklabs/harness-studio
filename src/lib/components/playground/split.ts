import { toast } from 'svelte-sonner';
import { t } from '#lib/i18n.svelte.js';
import { observeThinking, resetThinking } from './thinking';
import { guardToolCalls } from './toolLoopGuard';
// Playground split view (P2): the same prompt against two models or two presets side by side.
// Pure helpers (no Svelte runes): pane settings, per-side history, one streaming request,
// capacity errors (sequential fallback) and the small comparison summary.

import type { InferencePreset, ServerSummary } from '#lib/types.js';
import { salvageToolCalls, TextCallStreamBuffer } from './textToolCalls';
import { chatRequest } from './chatRequest';
import { notifyRoutingFallback } from './routingNotice';
import { parseTextToolCalls } from '#lib/tools-client.js';
import { invoke } from '#lib/ipc.js';
import { requestBody, requestOf } from '../presets/request';
import { requestErrorText } from '../connections/net';
import { buildApiMessages, buildStats, type Timings, type Usage } from './agentLoop';
import { SseParser, type SseEvent } from './sse';
import { describeError } from '#lib/gateway-busy-text.js';
import { ToolCallAccumulator } from '#lib/mcp-client.js';
import { newId, type ChatMessage, type PgSettings } from './types';

export type Side = 'a' | 'b';
/** What a pane is doing during a split turn. */
export type SideState = 'idle' | 'running' | 'queued';
export const SIDES: readonly Side[] = ['a', 'b'];

/**
 * What a pane talks to: `gw:<alias>` (a model definition through the gateway, loaded on the
 * first request) or `srv:<serverId>` (a running llama-server directly).
 */
export type TargetKey = string;

export interface PaneConfig {
  target: TargetKey;
  /** Inference preset of this side ('' = none: the server's defaults). */
  presetId: string;
}

export interface CompareSettings {
  enabled: boolean;
  a: PaneConfig;
  b: PaneConfig;
  /** Scroll both panes together. */
  syncScroll: boolean;
  /** Always run A, then B (instead of both at once). */
  sequential: boolean;
}

export const DEFAULT_COMPARE: CompareSettings = {
  enabled: false,
  a: { target: '', presetId: '' },
  b: { target: '', presetId: '' },
  syncScroll: true,
  sequential: false,
};

/** Split settings used for new chats (the last ones used). */
export const COMPARE_KEY = 'harness_playground_compare_v1';

function isObj(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === 'object' && !Array.isArray(v);
}

function pane(raw: unknown): PaneConfig {
  const r = isObj(raw) ? raw : {};
  return {
    target: typeof r.target === 'string' ? r.target : '',
    presetId: typeof r.presetId === 'string' ? r.presetId : '',
  };
}

export function sanitizeCompare(raw: unknown): CompareSettings {
  if (!isObj(raw)) return { ...DEFAULT_COMPARE, a: { ...DEFAULT_COMPARE.a }, b: { ...DEFAULT_COMPARE.b } };
  return {
    enabled: raw.enabled === true,
    a: pane(raw.a),
    b: pane(raw.b),
    syncScroll: raw.syncScroll !== false,
    sequential: raw.sequential === true,
  };
}

export async function loadCompareDefaults(): Promise<CompareSettings> {
  try {
    const raw = await invoke<string | null>('get_playground_state', { key: COMPARE_KEY });
    return sanitizeCompare(raw ? JSON.parse(raw) : null);
  } catch {
    return sanitizeCompare(null);
  }
}

export async function saveCompareDefaults(c: CompareSettings) {
  try {
    await invoke<void>('set_playground_state', { key: COMPARE_KEY, value: JSON.stringify(c) });
  } catch {
    // The open conversation also stores this value; defaults are best-effort.
  }
}

// ---- Targets ----

export const gwTarget = (alias: string): TargetKey => `gw:${alias}`;
export const srvTarget = (serverId: string): TargetKey => `srv:${serverId}`;

export type ParsedTarget = { kind: 'gateway'; alias: string } | { kind: 'server'; serverId: string } | null;

export function parseTarget(key: TargetKey): ParsedTarget {
  if (key.startsWith('gw:') && key.length > 3) return { kind: 'gateway', alias: key.slice(3) };
  if (key.startsWith('srv:') && key.length > 4) return { kind: 'server', serverId: key.slice(4) };
  return null;
}

/** File name of a server's model without `.gguf` (what `model` carries for a direct server). */
export function modelFileName(path: string): string {
  const parts = path.split(/[/\\]/);
  return (parts[parts.length - 1] || path).replace(/\.gguf$/i, '');
}

/** Running chat servers a pane can talk to directly. */
export function chatServers(servers: ServerSummary[]): ServerSummary[] {
  return servers.filter((s) => s.status === 'Running' && s.active && (s.active.serve_mode ?? 'chat') === 'chat');
}

export interface Endpoint {
  /** `.../v1` */
  baseUrl: string;
  headers: Record<string, string>;
  /** `model` of the request. */
  model: string;
  kind: 'gateway' | 'server';
  serverId?: string;
}

/** `gateway_co_residency` (harness_core::gateway::scheduler::CoResidency). */
export interface CoResidency {
  concurrent: boolean;
  reason: 'max_loaded_models' | 'insufficient_memory' | null;
  limit: number | null;
  need_bytes: number | null;
  available_bytes: number | null;
  to_load: string[];
  slots: number | null;
}

// ---- Stored split turns ----

/** `meta.compare` of a stored side (see `harness_core::state::compare_of`). */
export interface CompareMeta {
  group: string;
  side: Side;
  model: string;
  target: 'gateway' | 'server';
  server_id?: string | null;
  preset_id?: string | null;
  preset_name?: string | null;
  /** Ran after the other side instead of at the same time. */
  sequential?: boolean;
}

export function compareMetaOf(meta: unknown): CompareMeta | null {
  if (!isObj(meta) || !isObj(meta.compare)) return null;
  const c = meta.compare;
  if (typeof c.group !== 'string' || (c.side !== 'a' && c.side !== 'b')) return null;
  return {
    group: c.group,
    side: c.side,
    model: typeof c.model === 'string' ? c.model : '',
    target: c.target === 'server' ? 'server' : 'gateway',
    server_id: typeof c.server_id === 'string' ? c.server_id : null,
    preset_id: typeof c.preset_id === 'string' ? c.preset_id : null,
    preset_name: typeof c.preset_name === 'string' ? c.preset_name : null,
    sequential: c.sequential === true,
  };
}

/** The message a pane shows for `m`: its side of a split turn, else the (shared) message itself. */
export function sideOf(m: ChatMessage, side: Side): ChatMessage {
  return m.compareSides?.find((s) => s.compareSide === side) ?? m;
}

/** A split turn: an assistant message with both sides. */
export function isSplit(m: ChatMessage): boolean {
  return (m.compareSides?.length ?? 0) >= 2;
}

/**
 * OpenAI-format history as side `side` saw it (its own answers of earlier split turns, tool
 * rounds included). `current` = the rounds of the running answer so far (its `priorRounds`):
 * they follow the history, so a tool loop's next request carries what the side already did.
 */
export function sideHistory(
  messages: ChatMessage[],
  side: Side,
  systemPrompt: string,
  current?: ChatMessage[]
): Record<string, unknown>[] {
  const view = messages.map((m) => sideOf(m, side));
  if (current?.length) view.push({ id: 'running', role: 'assistant', content: '', priorRounds: current });
  return buildApiMessages(view, { systemPrompt } as PgSettings);
}

/** Sampling fields of a preset for the request body (none without a preset). */
export function presetBody(preset: InferencePreset | undefined, tools = false): Record<string, unknown> {
  return preset ? requestBody(requestOf(preset), { tools }) : {};
}

/**
 * Starts the next round of a side's tool loop: what the side has now (text, tool calls and their
 * results) moves into `priorRounds` as a plain copy and the message is reset for the new request.
 * Fields that belong to the whole answer (`rag`, ids, `compareMeta`) stay.
 */
export function beginRound(msg: ChatMessage) {
  const round: ChatMessage = {
    id: newId(),
    role: 'assistant',
    content: msg.content,
    reasoning: msg.reasoning,
    reasoningMs: msg.reasoningMs,
    stats: msg.stats ? { ...msg.stats } : undefined,
    error: msg.error,
    stopped: msg.stopped,
    finishReason: msg.finishReason,
    toolCalls: msg.toolCalls?.map((c) => ({ ...c })),
    parentId: msg.parentId,
  };
  msg.priorRounds = [...(msg.priorRounds ?? []), round];
  resetThinking(msg);
  msg.content = '';
  msg.reasoning = undefined;
  msg.stats = undefined;
  msg.error = undefined;
  msg.stopped = undefined;
  msg.finishReason = undefined;
  msg.toolCalls = undefined;
  msg.toolLimitReached = undefined;
  msg.streaming = true;
}

// ---- One streaming request ----

export interface StreamResult {
  /** HTTP status of a rejected request (0 = network error or none). */
  status: number;
  /** Gateway error code (`max_loaded_models`, `insufficient_memory`…) of a rejected request. */
  code?: string;
}

/**
 * Streams one chat completion into `msg` (reactive object: content, reasoning, tool calls, stats, error).
 * Returns the HTTP status and gateway error code when the request was rejected, so the caller
 * can retry sequentially.
 */
export async function streamInto(
  msg: ChatMessage,
  ep: Endpoint,
  body: Record<string, unknown>,
  signal: AbortSignal,
  text: {
    http: (status: number, message: string) => string;
    stream: (message: string) => string;
    network: (message: string) => string;
    noBody: string;
    toolLoop: string;
  },
  onChunk?: () => void
): Promise<StreamResult> {
  const local = new AbortController();
  const abort = () => local.abort();
  signal.addEventListener("abort", abort, { once: true });
  if (signal.aborted) local.abort();
  const started = performance.now();
  let ttftMs: number | undefined;
  let timings: Timings | null = null;
  let usage: Usage | null = null;
  let finished = false;
  let result: StreamResult = { status: 0 };
  const acc = new ToolCallAccumulator();
  const names = ((body.tools ?? []) as { function: { name: string } }[]).map((tool) => tool.function.name);
  // Both split sides use the same completion/recovery path as the single view.
  const contentBuffer = new TextCallStreamBuffer();

  const handle = (events: SseEvent[]) => {
    for (const ev of events) {
      if (ev.data === '[DONE]') {
        finished = true;
        continue;
      }
      let chunk: Record<string, unknown>;
      try {
        chunk = JSON.parse(ev.data);
      } catch {
        if (ev.event === 'error') msg.error = text.stream(ev.data);
        continue;
      }
      if (ev.event === 'error' || chunk.error) {
        const err = (chunk.error ?? chunk) as { message?: unknown };
        const message = typeof err === 'string' ? err : typeof err.message === 'string' ? err.message : ev.data;
        msg.error = text.stream(message);
        continue;
      }
      const choice = (chunk.choices as { delta?: Record<string, unknown>; finish_reason?: unknown }[] | undefined)?.[0];
      const delta = choice?.delta;
      if (delta) {
        const reasoning = typeof delta.reasoning_content === 'string' ? delta.reasoning_content : '';
        const content = typeof delta.content === 'string' ? delta.content : '';
        const hasCalls = Array.isArray(delta.tool_calls) && delta.tool_calls.length > 0;
        if ((reasoning || content || hasCalls) && ttftMs === undefined) ttftMs = performance.now() - started;
        if (reasoning) msg.reasoning = (msg.reasoning ?? '') + reasoning;
        if (content) {
          if (names.length) msg.content += contentBuffer.push(content);
          else msg.content += content;
        }
        observeThinking(msg, performance.now());
        if (hasCalls) {
          acc.push(delta.tool_calls);
          const checked = guardToolCalls(acc.result().map((tc) => ({
            id: tc.id,
            name: tc.function.name,
            arguments: tc.function.arguments,
            status: 'pending' as const,
          })), names.length);
          if (checked.stopped) {
            msg.toolCalls = checked.calls;
            msg.error = text.toolLoop;
            msg.finishReason = 'tool_calls';
            finished = true;
            local.abort();
            break;
          }
        }
      }
      if (typeof choice?.finish_reason === 'string') {
        msg.finishReason = choice.finish_reason;
        const checked = guardToolCalls(acc.result().map((tc) => ({
          id: tc.id, name: tc.function.name, arguments: tc.function.arguments, status: 'pending' as const,
        })), names.length, true);
        if (checked.calls.length) msg.toolCalls = checked.calls;
        if (checked.stopped) {
          msg.error = text.toolLoop;
          msg.finishReason = 'tool_calls';
          finished = true;
          local.abort();
          break;
        }
      }
      if (chunk.timings && typeof chunk.timings === 'object') timings = chunk.timings as Timings;
      if (chunk.usage && typeof chunk.usage === 'object') usage = chunk.usage as Usage;
    }
    if (events.length) onChunk?.();
  };

  try {
    const request = await chatRequest(ep.baseUrl,
      { ...body, stream: true, stream_options: { include_usage: true } },
      { 'Content-Type': 'application/json', Accept: 'text/event-stream', ...ep.headers }, local.signal);
    if (request.routingFallback) notifyRoutingFallback(request.routingError, body.model, request.routingWithoutTools);
    const res = request.response;
    names.splice(0, names.length, ...((request.body.tools ?? []) as { function: { name: string } }[]).map(tool => tool.function.name));
    if (!res.ok) {
      const raw = await res.text().catch(() => '');
      result = { status: res.status, code: errorCode(raw) ?? res.headers.get('x-harness-error') ?? undefined };
      msg.error = text.http(res.status, describeError(raw) || res.statusText);
      return result;
    }
    if (!res.body) {
      msg.error = text.noBody;
      return result;
    }
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    const parser = new SseParser();
    while (!finished) {
      const { value, done } = await reader.read();
      if (done) {
        handle(parser.push(decoder.decode()));
        handle(parser.flush());
        break;
      }
      handle(parser.push(decoder.decode(value, { stream: true })));
    }
    if (finished) reader.cancel().catch(() => {});
    await salvageToolCalls(msg, names, signal, parseTextToolCalls, msg.content + contentBuffer.drain());
    if (msg.toolCalls && !msg.error) {
      const checked = guardToolCalls(msg.toolCalls, names.length, true);
      msg.toolCalls = checked.calls;
      if (checked.stopped) { msg.error = text.toolLoop; msg.finishReason = 'tool_calls'; local.abort(); }
    }
    result = { status: res.status };
  } catch (e) {
    if (signal.aborted) msg.stopped = true;
    else if (!msg.error) msg.error = requestErrorText(e, `${ep.baseUrl}/chat/completions`, text.network);
  } finally {
    signal.removeEventListener("abort", abort);
    msg.content += contentBuffer.drain();
    if (msg.content || msg.reasoning || msg.toolCalls?.length || !msg.error) {
      msg.stats = buildStats(timings, usage, ttftMs, performance.now() - started);
    }
    if (msg.stats?.source === 'none' && !msg.error) {
      msg.stats = { ...msg.stats, source: 'estimate', promptN: Math.ceil(JSON.stringify(body).length / 3), predictedN: Math.ceil((msg.content + (msg.reasoning ?? '')).length / 3) };
    }
    msg.streaming = false;
  }
  return result;
}

function errorCode(raw: string): string | undefined {
  try {
    const v = JSON.parse(raw);
    const code = v?.error?.code;
    return typeof code === 'string' ? code : undefined;
  } catch {
    return undefined;
  }
}

/** The gateway could not hold this model next to the other one right now. */
export function isCapacityError(r: StreamResult): boolean {
  return r.status === 503 && (r.code === 'max_loaded_models' || r.code === 'insufficient_memory');
}

/** A fresh, empty side message (reactive once pushed into state). */
export function emptySide(side: Side, parentId: string | null): ChatMessage {
  return {
    id: newId(),
    role: 'assistant',
    content: '',
    streaming: true,
    variantId: newId(),
    parentId,
    compareSide: side,
  };
}

// ---- Comparison summary ----

/** Words (letters/digits runs) and single punctuation marks, for a word-level diff. */
export function tokenize(text: string): string[] {
  return text.match(/[\p{L}\p{N}_]+|[^\s\p{L}\p{N}_]/gu) ?? [];
}

export type DiffOp = { op: 'same' | 'add' | 'del'; text: string };

/** Word-level diff limit: beyond it only the similarity (bigram overlap) is computed. */
export const DIFF_MAX_TOKENS = 1500;

/** Longest-common-subsequence diff of two token lists (a → b). */
export function diffTokens(a: string[], b: string[]): DiffOp[] {
  const n = a.length;
  const m = b.length;
  // dp[i][j] = LCS length of a[i..] and b[j..] (flat array, one row per i).
  const dp = new Uint16Array((n + 1) * (m + 1));
  const at = (i: number, j: number) => i * (m + 1) + j;
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[at(i, j)] = a[i] === b[j] ? dp[at(i + 1, j + 1)] + 1 : Math.max(dp[at(i + 1, j)], dp[at(i, j + 1)]);
    }
  }
  const out: DiffOp[] = [];
  const push = (op: DiffOp['op'], text: string) => {
    const last = out[out.length - 1];
    if (last && last.op === op) last.text += ` ${text}`;
    else out.push({ op, text });
  };
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      push('same', a[i]);
      i++;
      j++;
    } else if (dp[at(i + 1, j)] >= dp[at(i, j + 1)]) {
      push('del', a[i++]);
    } else {
      push('add', b[j++]);
    }
  }
  while (i < n) push('del', a[i++]);
  while (j < m) push('add', b[j++]);
  return out;
}

/** Dice coefficient of word bigrams (0..1); cheap for long texts. */
export function bigramSimilarity(a: string[], b: string[]): number {
  const grams = (t: string[]) => {
    const map = new Map<string, number>();
    const src = t.length === 1 ? [t[0]] : t.slice(1).map((w, k) => `${t[k]}\u0000${w}`);
    for (const g of src) map.set(g, (map.get(g) ?? 0) + 1);
    return { map, size: src.length };
  };
  const ga = grams(a.map((w) => w.toLowerCase()));
  const gb = grams(b.map((w) => w.toLowerCase()));
  if (ga.size + gb.size === 0) return 1;
  let common = 0;
  for (const [g, c] of ga.map) common += Math.min(c, gb.map.get(g) ?? 0);
  return (2 * common) / (ga.size + gb.size);
}

export interface CompareSummary {
  /** 0..1: shared words (LCS) over the longer answer, or bigram overlap for long answers. */
  similarity: number;
  /** Word-level diff A → B (null when the answers are too long). */
  diff: DiffOp[] | null;
  wordsA: number;
  wordsB: number;
  /** Generation speed ratio (faster / slower) and which side is faster. */
  faster: Side | null;
  speedRatio: number | null;
  /** Side with the shorter time to first token and the difference (ms). */
  quicker: Side | null;
  ttftDeltaMs: number | null;
}

function words(tokens: string[]): number {
  return tokens.filter((w) => /[\p{L}\p{N}]/u.test(w)).length;
}

export function summarize(a: ChatMessage, b: ChatMessage): CompareSummary {
  const ta = tokenize(a.content);
  const tb = tokenize(b.content);
  let diff: DiffOp[] | null = null;
  let similarity: number;
  if (ta.length <= DIFF_MAX_TOKENS && tb.length <= DIFF_MAX_TOKENS) {
    diff = diffTokens(ta, tb);
    const same = diff.filter((d) => d.op === 'same').reduce((n, d) => n + d.text.split(' ').length, 0);
    const longest = Math.max(ta.length, tb.length);
    similarity = longest === 0 ? 1 : same / longest;
  } else {
    similarity = bigramSimilarity(ta, tb);
  }
  const sa = a.stats;
  const sb = b.stats;
  let faster: Side | null = null;
  let speedRatio: number | null = null;
  const va = sa?.predictedPerSecond;
  const vb = sb?.predictedPerSecond;
  if (va && vb && va > 0 && vb > 0) {
    faster = va >= vb ? 'a' : 'b';
    speedRatio = Math.max(va, vb) / Math.min(va, vb);
  }
  let quicker: Side | null = null;
  let ttftDeltaMs: number | null = null;
  if (sa?.ttftMs !== undefined && sb?.ttftMs !== undefined) {
    quicker = sa.ttftMs <= sb.ttftMs ? 'a' : 'b';
    ttftDeltaMs = Math.abs(sa.ttftMs - sb.ttftMs);
  }
  return { similarity, diff, wordsA: words(ta), wordsB: words(tb), faster, speedRatio, quicker, ttftDeltaMs };
}
