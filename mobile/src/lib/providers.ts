// BYOK providers called straight from the phone (no computer): OpenAI, Gemini (its
// OpenAI-compatible endpoint), Anthropic, OpenRouter and custom OpenAI-compatible endpoints.
// The rules mirror the desktop's `harness-core::cloud` (model filters, prices, quota), so a
// provider lists the same models on both.

export type ProviderKind = 'openai' | 'gemini' | 'anthropic' | 'openrouter' | 'custom';

export const PROVIDER_KINDS: ProviderKind[] = ['openai', 'gemini', 'anthropic', 'openrouter', 'custom'];

export const ANTHROPIC_VERSION = '2023-06-01';

export function defaultBaseUrl(kind: ProviderKind): string | null {
  switch (kind) {
    case 'openai':
      return 'https://api.openai.com/v1';
    case 'gemini':
      return 'https://generativelanguage.googleapis.com/v1beta/openai';
    case 'anthropic':
      return 'https://api.anthropic.com/v1';
    case 'openrouter':
      return 'https://openrouter.ai/api/v1';
    case 'custom':
      return null;
  }
}

export function providerLabel(kind: ProviderKind): string {
  return { openai: 'OpenAI', gemini: 'Gemini', anthropic: 'Anthropic', openrouter: 'OpenRouter', custom: 'Custom' }[kind];
}

/** Custom endpoints (a self-hosted server) may run without a key. */
export function keyRequired(kind: ProviderKind): boolean {
  return kind !== 'custom';
}

/** A base URL the phone may call: https, or http to a private/loopback address only. */
export function validBaseUrl(raw: string): string | null {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    return null;
  }
  if (url.username || url.password || url.search || url.hash) return null;
  if (url.protocol === 'http:' && !isLocalHost(url.hostname)) return null;
  if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;
  return url.toString().replace(/\/+$/, '');
}

/** Loopback, private IPv4 ranges, link-local, `.local` and `.test` names. */
export function isLocalHost(host: string): boolean {
  const h = host.replace(/^\[|\]$/g, '').toLowerCase();
  if (h === 'localhost' || h.endsWith('.local') || h.endsWith('.localhost') || h.endsWith('.test')) return true;
  if (h === '::1' || h.startsWith('fe80:') || h.startsWith('fd') || h.startsWith('fc')) return h.includes(':');
  const m = h.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (!m) return false;
  const [a, b] = [Number(m[1]), Number(m[2])];
  return a === 127 || a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254) || (a === 100 && b >= 64 && b <= 127);
}

export interface ProviderModel {
  id: string;
  displayName: string | null;
  contextLength: number | null;
  /** USD per million tokens (OpenRouter publishes prices). */
  inputPrice: number | null;
  outputPrice: number | null;
  tools: boolean | null;
  vision: boolean | null;
}

const NOT_CHAT = ['embedding', 'tts', 'whisper', 'dall-e', 'moderation', 'transcribe', 'image', 'davinci', 'babbage', 'realtime', 'audio', 'search'];

function clean(v: unknown): string | null {
  return typeof v === 'string' && v && v.length <= 512 && !/[\u0000-\u001f\u007f]/.test(v) ? v : null;
}

function perMillion(v: unknown): number | null {
  const n = typeof v === 'string' ? Number(v) : typeof v === 'number' ? v : NaN;
  return Number.isFinite(n) && n >= 0 ? Math.round(n * 1e6 * 1e6) / 1e6 : null;
}

function int(v: unknown): number | null {
  return typeof v === 'number' && Number.isInteger(v) && v > 0 ? v : null;
}

/** The chat models of a provider's `/models` answer. */
export function parseModels(kind: ProviderKind, body: unknown): ProviderModel[] {
  const root = body as Record<string, unknown> | null;
  const data = Array.isArray(root?.data) ? root.data : Array.isArray(root?.models) ? root.models : null;
  if (!data) throw new Error('mobile.invalid_response');
  const out = new Map<string, ProviderModel>();
  for (const raw of data as Record<string, unknown>[]) {
    let id = clean(raw?.id) ?? clean(raw?.name);
    if (!id) continue;
    if (kind === 'gemini') id = id.replace(/^models\//, '');
    if (id.includes('embed')) continue;
    if (kind === 'openai' && NOT_CHAT.some((s) => id!.includes(s))) continue;
    if (kind === 'gemini' && id.includes('aqa')) continue;
    const model: ProviderModel = {
      id,
      displayName: clean(raw.display_name) ?? (typeof raw.id === 'string' ? clean(raw.name) : null),
      contextLength: int(raw.context_length) ?? int(raw.context_window) ?? int(raw.max_input_tokens) ?? int(raw.inputTokenLimit),
      inputPrice: null,
      outputPrice: null,
      tools: null,
      vision: null,
    };
    if (kind === 'openrouter') {
      const pricing = (raw.pricing ?? {}) as Record<string, unknown>;
      model.inputPrice = perMillion(pricing.prompt);
      model.outputPrice = perMillion(pricing.completion);
      if (Array.isArray(raw.supported_parameters)) model.tools = raw.supported_parameters.includes('tools');
      const input = (raw.architecture as Record<string, unknown> | undefined)?.input_modalities;
      if (Array.isArray(input)) model.vision = input.includes('image');
    }
    out.set(id, model);
  }
  return [...out.values()].sort((a, b) => a.id.localeCompare(b.id));
}

export interface Quota {
  limit: number | null;
  usage: number | null;
  remaining: number | null;
}

/** OpenRouter's `/key` answer: credits in USD. */
export function parseQuota(body: unknown): Quota | null {
  const data = (body as { data?: Record<string, unknown> } | null)?.data;
  if (!data) return null;
  const num = (v: unknown) => (typeof v === 'number' ? v : null);
  const quota = { limit: num(data.limit), usage: num(data.usage), remaining: num(data.limit_remaining) };
  return quota.limit === null && quota.usage === null && quota.remaining === null ? null : quota;
}

export function modelsUrl(kind: ProviderKind, base: string): string {
  return kind === 'anthropic' ? `${base}/models?limit=1000` : `${base}/models`;
}

/** Extra headers (the key itself is added by the native side). */
export function providerHeaders(kind: ProviderKind): [string, string][] {
  const headers: [string, string][] = [['content-type', 'application/json']];
  if (kind === 'anthropic') headers.push(['anthropic-version', ANTHROPIC_VERSION]);
  if (kind === 'openrouter') headers.push(['x-title', 'Harness Studio Mobile']);
  return headers;
}

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant';
  content: string;
}

/** The streaming chat request of a provider: (path, JSON body). */
export function chatRequest(kind: ProviderKind, model: string, messages: ChatMessage[], maxTokens = 2048): { path: string; body: string } {
  if (kind === 'anthropic') {
    const system = messages.filter((m) => m.role === 'system').map((m) => m.content).join('\n\n');
    const rest = messages.filter((m) => m.role !== 'system');
    return {
      path: '/messages',
      body: JSON.stringify({ model, max_tokens: maxTokens, stream: true, messages: rest, ...(system ? { system } : {}) }),
    };
  }
  return {
    path: '/chat/completions',
    body: JSON.stringify({ model, messages, stream: true, stream_options: { include_usage: true } }),
  };
}

export interface Usage {
  promptTokens: number;
  completionTokens: number;
}

export interface Delta {
  text: string;
  usage: Usage | null;
  done: boolean;
  error: string | null;
}

/** One SSE event of a streamed chat answer: OpenAI-style chunks or Anthropic message events. */
export function streamDelta(kind: ProviderKind | 'harness', event: string | null, data: string): Delta {
  const none: Delta = { text: '', usage: null, done: false, error: null };
  if (data === '[DONE]') return { ...none, done: true };
  let v: Record<string, any>;
  try {
    v = JSON.parse(data);
  } catch {
    return none;
  }
  if (v?.error) return { ...none, error: String(v.error.message ?? v.error.type ?? 'error') };
  if (kind === 'anthropic') {
    switch (event ?? v.type) {
      case 'content_block_delta':
        return { ...none, text: v.delta?.type === 'text_delta' ? String(v.delta.text ?? '') : '' };
      case 'message_start': {
        const u = v.message?.usage;
        return { ...none, usage: u ? { promptTokens: u.input_tokens ?? 0, completionTokens: u.output_tokens ?? 0 } : null };
      }
      case 'message_delta': {
        const u = v.usage;
        return { ...none, usage: u ? { promptTokens: u.input_tokens ?? 0, completionTokens: u.output_tokens ?? 0 } : null };
      }
      case 'message_stop':
        return { ...none, done: true };
      default:
        return none;
    }
  }
  const text = v.choices?.[0]?.delta?.content;
  const u = v.usage;
  return {
    ...none,
    text: typeof text === 'string' ? text : '',
    usage: u ? { promptTokens: u.prompt_tokens ?? 0, completionTokens: u.completion_tokens ?? 0 } : null,
  };
}

/** Merges Anthropic's split usage (input at start, output at the end). */
export function mergeUsage(a: Usage | null, b: Usage | null): Usage | null {
  if (!b) return a;
  if (!a) return b;
  return { promptTokens: b.promptTokens || a.promptTokens, completionTokens: b.completionTokens || a.completionTokens };
}

/** USD for a reply, when the model has prices. */
export function cost(model: Pick<ProviderModel, 'inputPrice' | 'outputPrice'> | null, usage: Usage | null): number | null {
  if (!model || !usage || model.inputPrice === null || model.outputPrice === null) return null;
  return (usage.promptTokens * model.inputPrice + usage.completionTokens * model.outputPrice) / 1e6;
}

/** The translation key for an HTTP error of a provider or Harness. */
export function statusError(status: number): string {
  if (status === 401 || status === 403) return 'mobile.err.unauthorized';
  if (status === 402) return 'mobile.err.payment';
  if (status === 404) return 'mobile.err.not_found';
  if (status === 429) return 'mobile.err.rate_limited';
  if (status >= 500) return 'mobile.err.server';
  return 'mobile.err.request';
}

/** The provider's own error text from an error body (OpenAI, Anthropic and Harness shapes). */
export function errorMessage(body: string): string | null {
  try {
    const v = JSON.parse(body);
    const m = v?.error?.message ?? v?.message ?? v?.error;
    return typeof m === 'string' ? m.slice(0, 400) : null;
  } catch {
    return null;
  }
}
