// Request-side settings of an InferencePreset (sampling, reasoning, output format) as the
// Playground and the preset manager edit them, and their mapping to llama-server's
// request body (mirrors `InferencePreset::request_params` in harness-core).

import type { InferencePreset, ReasoningSettings, SamplingParams } from '$lib/types';

export interface PresetRequest {
  sampling: SamplingParams;
  reasoning: ReasoningSettings;
  response_format?: unknown;
  tool_choice?: unknown;
}

export const DEFAULT_PRESET_ID = 'builtin:default';

export function isBuiltinPreset(id: string | null | undefined): boolean {
  return !!id && id.startsWith('builtin:');
}

export function emptyRequest(): PresetRequest {
  return { sampling: {}, reasoning: {} };
}

/** A value that means "not set" (omitted from the request). */
export function isBlank(v: unknown): boolean {
  if (v === null || v === undefined) return true;
  if (typeof v === 'number') return !Number.isFinite(v);
  if (typeof v === 'string') return v.trim() === '';
  if (Array.isArray(v)) return v.length === 0;
  return false;
}

function cleanObject<T extends object>(o: T | undefined): T {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(o ?? {})) {
    if (isBlank(v)) continue;
    out[k] = Array.isArray(v) ? v.filter((x) => !(typeof x === 'string' && x === '')) : v;
    if (Array.isArray(out[k]) && (out[k] as unknown[]).length === 0) delete out[k];
  }
  return out as T;
}

/** Only the values that are set (stable shape for comparing and storing). */
export function cleanRequest(r: PresetRequest | null | undefined): PresetRequest {
  const out: PresetRequest = { sampling: cleanObject(r?.sampling), reasoning: cleanObject(r?.reasoning) };
  if (!isBlank(r?.response_format)) out.response_format = r?.response_format;
  if (!isBlank(r?.tool_choice)) out.tool_choice = r?.tool_choice;
  return out;
}

/** Detached copy of a preset's request settings. */
export function requestOf(p: InferencePreset | null | undefined): PresetRequest {
  if (!p) return emptyRequest();
  return structuredClone(
    cleanRequest({
      sampling: p.sampling ?? {},
      reasoning: p.reasoning ?? {},
      response_format: p.response_format ?? undefined,
      tool_choice: p.tool_choice ?? undefined,
    })
  );
}

function stable(v: unknown): string {
  if (Array.isArray(v)) return `[${v.map(stable).join(',')}]`;
  if (v && typeof v === 'object') {
    return `{${Object.keys(v as object)
      .sort()
      .map((k) => `${JSON.stringify(k)}:${stable((v as Record<string, unknown>)[k])}`)
      .join(',')}}`;
  }
  return JSON.stringify(v);
}

export function sameRequest(a: PresetRequest | null | undefined, b: PresetRequest | null | undefined): boolean {
  return stable(cleanRequest(a)) === stable(cleanRequest(b));
}

const INT_KEYS = new Set([
  'top_k',
  'repeat_last_n',
  'dry_allowed_length',
  'dry_penalty_last_n',
  'mirostat',
  'top_logprobs',
  'seed',
  'max_tokens',
]);

/**
 * Request body fields (llama-server names). Blank values are omitted; `tool_choice` is only
 * sent together with tools.
 */
export function requestBody(r: PresetRequest, opts: { tools: boolean }): Record<string, unknown> {
  const c = cleanRequest(r);
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(c.sampling)) {
    out[k] = typeof v === 'number' && INT_KEYS.has(k) ? Math.trunc(v) : v;
  }
  const rs = c.reasoning;
  if (rs.format) out.reasoning_format = rs.format;
  if (rs.effort) out.reasoning_effort = rs.effort;
  if (typeof rs.budget === 'number') out.thinking_budget_tokens = Math.trunc(rs.budget);
  if (typeof rs.enabled === 'boolean') out.chat_template_kwargs = { enable_thinking: rs.enabled };
  if (c.response_format !== undefined) out.response_format = c.response_format;
  if (opts.tools && c.tool_choice !== undefined) out.tool_choice = c.tool_choice;
  return out;
}

/** A user preset (new, empty id) holding these request settings. */
export function presetFromRequest(name: string, systemPrompt: string, r: PresetRequest): InferencePreset {
  const c = cleanRequest(r);
  return {
    id: '',
    name,
    system_prompt: systemPrompt.trim() ? systemPrompt : null,
    sampling: c.sampling,
    reasoning: c.reasoning,
    response_format: c.response_format ?? null,
    tool_choice: c.tool_choice ?? null,
    created_at: 0,
    updated_at: 0,
  };
}
