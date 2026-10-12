// Form model of a named token: scopes plus its gateway policy (allowed models, endpoints,
// rate limits). Mirrors harness_core::token_policy; the core validates again.
import type { ApiScope, ApiTokenInfo, GatewayEndpoint, RateLimits, TokenPolicy } from '$lib/types';
import type { TKey } from '$lib/i18n.svelte';

export const SCOPES: { id: ApiScope; label: TKey }[] = [
  { id: 'read', label: 'dev.scope_read' },
  { id: 'inference', label: 'dev.scope_inference' },
  { id: 'models:write', label: 'dev.scope_models_write' },
  { id: 'downloads:write', label: 'dev.scope_downloads_write' },
  { id: 'servers:write', label: 'dev.scope_servers_write' },
  { id: 'servers:control', label: 'dev.scope_servers_control' },
  { id: 'conversations', label: 'dev.scope_conversations' },
  { id: 'tools', label: 'dev.scope_tools' },
  { id: 'admin', label: 'dev.scope_admin' },
];

export const ENDPOINTS: { id: GatewayEndpoint; label: TKey }[] = [
  { id: 'chat', label: 'dev.endpoint_chat' },
  { id: 'embeddings', label: 'dev.endpoint_embeddings' },
  { id: 'rerank', label: 'dev.endpoint_rerank' },
  { id: 'audio', label: 'dev.endpoint_audio' },
  { id: 'images', label: 'dev.endpoint_images' },
  { id: 'models', label: 'dev.endpoint_models' },
  { id: 'messages', label: 'dev.endpoint_messages' },
  { id: 'ollama', label: 'dev.endpoint_ollama' },
  { id: 'metrics', label: 'dev.endpoint_metrics' },
];

export interface TokenDraft {
  name: string;
  scopes: Record<ApiScope, boolean>;
  /** Model patterns, one per line (or comma-separated). */
  models: string;
  endpoints: Record<GatewayEndpoint, boolean>;
  /** Limits as typed; empty = unlimited. */
  rpm: string;
  tpm: string;
  concurrency: string;
}

function record<K extends string>(ids: K[], on: (id: K) => boolean): Record<K, boolean> {
  return Object.fromEntries(ids.map((id) => [id, on(id)])) as Record<K, boolean>;
}

export function emptyDraft(): TokenDraft {
  return {
    name: '',
    scopes: record(
      SCOPES.map((s) => s.id),
      (id) => id === 'read',
    ),
    models: '',
    endpoints: record(
      ENDPOINTS.map((e) => e.id),
      () => true,
    ),
    rpm: '',
    tpm: '',
    concurrency: '',
  };
}

export function draftOf(t: ApiTokenInfo): TokenDraft {
  const num = (n: number | null) => (n == null ? '' : String(n));
  return {
    name: t.name,
    scopes: record(
      SCOPES.map((s) => s.id),
      (id) => t.scopes.includes(id),
    ),
    models: t.models.join('\n'),
    endpoints: record(
      ENDPOINTS.map((e) => e.id),
      (id) => t.endpoints.length === 0 || t.endpoints.includes(id),
    ),
    rpm: num(t.limits.requests_per_minute),
    tpm: num(t.limits.tokens_per_minute),
    concurrency: num(t.limits.max_concurrent),
  };
}

/** Scopes a wider selected scope already covers (shown checked and disabled). */
export function impliedBy(d: TokenDraft, id: ApiScope): boolean {
  if (id !== 'admin' && d.scopes.admin) return true;
  if (id === 'servers:control' && d.scopes['servers:write']) return true;
  if (id === 'downloads:write' && d.scopes['models:write']) return true;
  return false;
}

/** Selected scopes without the implied ones (the core would accept them, but they add nothing). */
export function selectedScopes(d: TokenDraft): ApiScope[] {
  return SCOPES.map((s) => s.id).filter((id) => d.scopes[id] && !impliedBy(d, id));
}

/** Whether the token can reach the gateway at all (the policy only matters then). */
export function usesGateway(d: TokenDraft): boolean {
  return d.scopes.inference || d.scopes.admin;
}

export function parsePatterns(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** A limit field: '' = unlimited, otherwise a positive whole number; `undefined` = invalid. */
export function parseLimit(text: string): number | null | undefined {
  const s = text.trim();
  if (s === '') return null;
  if (!/^\d+$/.test(s)) return undefined;
  const n = Number(s);
  return n > 0 && Number.isSafeInteger(n) ? n : undefined;
}

/** The policy of a draft, or `null` when a limit is not a positive whole number. */
export function policyOf(d: TokenDraft): TokenPolicy | null {
  const rpm = parseLimit(d.rpm);
  const tpm = parseLimit(d.tpm);
  const conc = parseLimit(d.concurrency);
  if (rpm === undefined || tpm === undefined || conc === undefined) return null;
  const chosen = ENDPOINTS.map((e) => e.id).filter((id) => d.endpoints[id]);
  const limits: RateLimits = { requests_per_minute: rpm, tokens_per_minute: tpm, max_concurrent: conc };
  return {
    models: parsePatterns(d.models),
    endpoints: chosen.length === ENDPOINTS.length ? [] : chosen,
    limits,
  };
}

export function limitsInvalid(d: TokenDraft): boolean {
  return policyOf(d) === null;
}

export function noEndpoint(d: TokenDraft): boolean {
  return ENDPOINTS.every((e) => !d.endpoints[e.id]);
}
