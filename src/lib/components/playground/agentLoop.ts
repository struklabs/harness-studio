import { guardToolCalls } from './toolLoopGuard';
// Agent tool loop for the Playground: approval waiter map, tool execution and the
// model -> tools -> model round loop. Pure TS (no DOM, no Svelte runes); the caller
// hands in the reactive message objects and callbacks.
//
// Tools run on the Harness core (`tools_run`): the policy, the working directory, the limits
// and the tool log are the core's, this file only orchestrates a streaming conversation and
// shows the approval prompt when the core says "ask". That includes the tools of MCP servers,
// which Harness's own MCP client (core, T3) runs.

import { parseToolArgs } from '#lib/mcp-client.js';
import type { ServerTool } from '#lib/mcp-types.js';
import {
  cancelCoreTool,
  coreToolDecision,
  denyCoreTool,
  newRunId,
  runCoreTool,
  type ToolLimits,
} from '#lib/tools-client.js';
import type { ChatMessage, PgSettings, PgToolCall, ResponseStats } from './types';
import { activeCompaction, summaryBlock } from './compaction.svelte';
import { repairSearchQuery } from './searchQueryRepair';
import { userContent } from './images';

export const DENIED_TEXT = 'User denied this tool call.';
export const CANCELLED_TEXT = 'Tool call was cancelled before it ran.';

export type ApprovalDecision = 'approve' | 'deny';

/** Tracks tool calls waiting for the user and the tools allowed for the rest of the session. */
export class ApprovalBroker {
  /**
   * Tools the user chose to "always allow" in this session (memory only, never persisted). It
   * only answers the prompt: the policy in Settings > Tools is not touched, and a policy of
   * "off" still refuses.
   */
  private sessionAllowed = new Set<string>();
  private waiters = new Map<string, (decision: ApprovalDecision) => void>();

  isAllowed(toolName: string): boolean {
    return this.sessionAllowed.has(toolName);
  }

  wait(id: string, signal: AbortSignal): Promise<ApprovalDecision | 'stop'> {
    return new Promise((resolve) => {
      if (signal.aborted) return resolve('stop');
      const onAbort = () => {
        this.waiters.delete(id);
        resolve('stop');
      };
      signal.addEventListener('abort', onAbort, { once: true });
      this.waiters.set(id, (decision) => {
        signal.removeEventListener('abort', onAbort);
        this.waiters.delete(id);
        resolve(decision);
      });
    });
  }

  approve(callId: string, always: boolean, toolName?: string) {
    if (always && toolName) this.sessionAllowed.add(toolName);
    this.waiters.get(callId)?.('approve');
  }

  deny(callId: string) {
    this.waiters.get(callId)?.('deny');
  }
}

export interface ExecuteContext {
  /** The tools offered to the model (core tools incl. MCP servers' tools). */
  tools: ServerTool[];
  /** The core's limits (per-call time, result size, rounds). */
  limits: ToolLimits;
  /** For the tool log (Harness tools). */
  conversationId?: string | null;
  model?: string;
  latestUserMessage?: string;
  /** Working directory of the Harness file/shell tools ('' = the default from Settings > Tools). */
  workdir?: string;
  broker: ApprovalBroker;
  /** Called after visible state changes (used to keep the list scrolled down). */
  onProgress?: () => void;
  /**
   * Key a call waits for its approval under (default: the call id). The split view runs two
   * tool loops at once, so the same id on both sides must not share one answer.
   */
  approvalKey?: (call: PgToolCall) => string;
}

/** Waits for the user's answer to one call; `null` = the run was stopped meanwhile. */
async function askUser(call: PgToolCall, signal: AbortSignal, ctx: ExecuteContext): Promise<ApprovalDecision | null> {
  call.status = 'approval';
  ctx.onProgress?.();
  const decision = await ctx.broker.wait(ctx.approvalKey?.(call) ?? call.id, signal);
  return decision === 'stop' ? null : decision;
}

/** A call on the Harness core: the core decides, the user is asked only when it says "ask". */
async function runCoreCall(call: PgToolCall, args: Record<string, unknown>, signal: AbortSignal, ctx: ExecuteContext) {
  const decision = await coreToolDecision(call.name);
  const base = {
    call_id: newRunId(),
    tool: call.name,
    args,
    workdir: ctx.workdir ?? '',
    conversation: ctx.conversationId ?? null,
    model: ctx.model ?? null,
  };
  let approved = false;
  if (decision === 'ask') {
    if (ctx.broker.isAllowed(call.name)) {
      call.autoApproved = true;
      approved = true;
    } else {
      const answer = await askUser(call, signal, ctx);
      if (answer === null) {
        call.status = 'stopped';
        return;
      }
      if (answer === 'deny') {
        // Logged as a refusal; the call does not run.
        await denyCoreTool(base).catch(() => {});
        call.status = 'denied';
        call.result = DENIED_TEXT;
        return;
      }
      approved = true;
    }
  }
  call.status = 'running';
  ctx.onProgress?.();
  // Stop cancels the running call in the core (a shell's process tree dies with it).
  const onAbort = () => void cancelCoreTool(base.call_id).catch(() => {});
  signal.addEventListener('abort', onAbort, { once: true });
  try {
    const r = await runCoreTool({ ...base, approved });
    if (r.error === 'cancelled' && signal.aborted) {
      call.status = 'stopped';
      return;
    }
    call.result = r.error ? `Error: ${r.content}` : r.content;
    call.status = r.error ? 'error' : 'done';
    call.errorCode = r.error ?? undefined;
  } catch (e) {
    call.status = signal.aborted ? 'stopped' : 'error';
    if (!signal.aborted) call.result = `Error: ${e instanceof Error ? e.message : String(e)}`;
  } finally {
    signal.removeEventListener('abort', onAbort);
  }
}

/** Runs the tool calls of one assistant message sequentially, updating each call in place. */
export async function executeToolCalls(msg: ChatMessage, signal: AbortSignal, ctx: ExecuteContext) {
  const available = new Map(ctx.tools.map((tl) => [tl.tool, tl]));
  const progress = () => ctx.onProgress?.();
  for (const call of msg.toolCalls ?? []) {
    if (signal.aborted) {
      call.status = 'stopped';
      continue;
    }
    const meta = available.get(call.name);
    call.dangerous = meta?.class === 'dangerous';
    if (!meta) {
      call.status = 'error';
      call.result = `Error: tool "${call.name}" is not available.`;
      continue;
    }
    const parsed = parseToolArgs(call.arguments);
    if (!parsed.ok) {
      call.status = 'error';
      call.result = `Error: invalid JSON arguments (${parsed.error}).`;
      continue;
    }
    if (call.name === 'web_search' && typeof parsed.value.query === 'string' && ctx.latestUserMessage) {
      const query = repairSearchQuery(parsed.value.query, ctx.latestUserMessage);
      if (query !== parsed.value.query) {
        parsed.value.query = query;
        // The tool card, persisted history and next model round show the executed args.
        call.arguments = JSON.stringify(parsed.value);
        progress();
      }
    }
    await runCoreCall(call, parsed.value, signal, ctx);
    progress();
  }
}

export interface AgentLoopOptions {
  controller: AbortController;
  /** Rounds before the loop stops (the core's `max_rounds`). */
  maxRounds: number;
  /** One streaming request; appends and returns the (reactive) assistant message. */
  streamTurn: (c: AbortController) => Promise<ChatMessage>;
  /** Number of tools currently offered to the model (0 = nothing can run). */
  activeToolCount: () => number;
  execute: (msg: ChatMessage, signal: AbortSignal) => Promise<void>;
  persist: () => void;
  /**
   * Runs between a finished tool round and the next request (the context may have outgrown its
   * limit by now): the place to compact the conversation. Not called after the last round.
   */
  beforeNextRound?: (msg: ChatMessage, signal: AbortSignal) => Promise<void>;
}

/** Streams replies and runs the requested tools until the model answers without tool calls. */
export async function runAgentLoop(o: AgentLoopOptions): Promise<void> {
  const c = o.controller;
  let last: ChatMessage | undefined;
  try {
    for (let round = 1; ; round++) {
      const msg = await o.streamTurn(c);
      last = msg;
      if (msg.toolCalls) {
        const checked = guardToolCalls(msg.toolCalls, o.activeToolCount(), true);
        msg.toolCalls = checked.calls;
        if (checked.stopped) { msg.toolLimitReached = true; c.abort(); }
      }
      // No tools offered (tools off / list unavailable): nothing to run, the calls end up "cancelled".
      if (c.signal.aborted || msg.error || !msg.toolCalls?.length || o.activeToolCount() === 0) break;
      await o.execute(msg, c.signal);
      if (c.signal.aborted) break;
      if (round >= o.maxRounds) {
        msg.toolLimitReached = true;
        break;
      }
      o.persist();
      await o.beforeNextRound?.(msg, c.signal);
      if (c.signal.aborted) break;
    }
  } finally {
    // Anything that never got to run was cancelled.
    for (const call of last?.toolCalls ?? []) {
      if (call.status === 'pending' || call.status === 'approval' || call.status === 'running') call.status = 'stopped';
    }
    if (last && c.signal.aborted) last.stopped = true;
  }
}

// ---- Request helpers ----

function toolMessageContent(c: PgToolCall): string {
  if (c.status === 'denied') return DENIED_TEXT;
  return c.result ?? CANCELLED_TEXT;
}

/** One round of an answer as OpenAI messages: assistant(tool_calls) + one `tool` message per call. */
function pushRound(out: Record<string, unknown>[], m: ChatMessage) {
  if (m.toolCalls?.length) {
    out.push({
      role: 'assistant',
      content: m.content,
      tool_calls: m.toolCalls.map((c) => ({
        id: c.id,
        type: 'function',
        function: { name: c.name, arguments: c.arguments },
      })),
    });
    for (const c of m.toolCalls) out.push({ role: 'tool', tool_call_id: c.id, content: toolMessageContent(c) });
  } else if (m.content) {
    // Skip failed/empty assistant turns so they don't pollute the context.
    out.push({ role: 'assistant', content: m.content });
  }
}

/**
 * OpenAI-format history: tool rounds become assistant(tool_calls) + one `tool` message per call.
 * An active compaction marker replaces everything up to its boundary by its summary (in the
 * system message); markers themselves are never sent (`harness_core::compaction::request_messages`).
 * A marker made between two tool rounds also pins the running turn's user message: it lies
 * before the boundary but is sent right after the summary.
 */
export function buildApiMessages(messages: ChatMessage[], settings: PgSettings): Record<string, unknown>[] {
  const out: Record<string, unknown>[] = [];
  const active = activeCompaction(messages);
  let system = settings.systemPrompt.trim() ? settings.systemPrompt : '';
  if (active) system = [system.trim(), summaryBlock(active.summary)].filter(Boolean).join('\n\n');
  if (system) out.push({ role: 'system', content: system });
  if (active?.pinnedIndex != null) {
    const pinned = messages[active.pinnedIndex];
    out.push({ role: 'user', content: userContent(pinned.content, pinned.images) });
  }
  for (const m of active ? messages.slice(active.boundaryIndex + 1) : messages) {
    if (m.role === 'compaction') continue;
    if (m.role === 'user') {
      out.push({ role: 'user', content: userContent(m.content, m.images) });
    } else {
      for (const r of m.priorRounds ?? []) pushRound(out, r);
      pushRound(out, m);
    }
  }
  return out;
}

// ---- Response stats ----

function num(v: unknown): number | undefined {
  return typeof v === 'number' && Number.isFinite(v) ? v : undefined;
}

export interface Timings {
  predicted_n?: number;
  predicted_ms?: number;
  predicted_per_second?: number;
  prompt_n?: number;
  prompt_ms?: number;
  prompt_per_second?: number;
  cache_n?: number;
  /** Speculative decoding (only present when the server drafted). */
  draft_n?: number;
  draft_n_accepted?: number;
}

export interface Usage {
  completion_tokens?: number;
  prompt_tokens?: number;
  prompt_tokens_details?: { cached_tokens?: number };
}

export function buildStats(timings: Timings | null, usage: Usage | null, ttftMs?: number, totalMs?: number): ResponseStats {
  if (timings) {
    return {
      source: 'timings',
      v: 2,
      predictedN: num(timings.predicted_n) ?? num(usage?.completion_tokens),
      predictedMs: num(timings.predicted_ms),
      predictedPerSecond: num(timings.predicted_per_second),
      promptN: num(usage?.prompt_tokens) ?? ((num(timings.prompt_n) ?? 0) + (num(timings.cache_n) ?? num(usage?.prompt_tokens_details?.cached_tokens) ?? 0)),
      promptMs: num(timings.prompt_ms),
      promptPerSecond: num(timings.prompt_per_second),
      cachedN: num(timings.cache_n) ?? num(usage?.prompt_tokens_details?.cached_tokens),
      draftN: num(timings.draft_n),
      draftAcceptedN: num(timings.draft_n_accepted),
      ttftMs,
      totalMs,
    };
  }
  if (usage) {
    const predictedN = num(usage.completion_tokens);
    // Approximate decode speed from wall-clock time after the first token.
    const decodeMs = totalMs !== undefined && ttftMs !== undefined ? totalMs - ttftMs : undefined;
    const predictedPerSecond =
      predictedN !== undefined && predictedN > 1 && decodeMs !== undefined && decodeMs > 0
        ? ((predictedN - 1) * 1000) / decodeMs
        : undefined;
    return {
      source: 'usage',
      v: 2,
      predictedN,
      predictedPerSecond,
      promptN: num(usage.prompt_tokens),
      cachedN: num(usage.prompt_tokens_details?.cached_tokens),
      ttftMs,
      totalMs,
    };
  }
  return { v: 2, source: 'none', ttftMs, totalMs };
}
