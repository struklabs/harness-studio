/** Gateway's legacy text is also nested inside keyed tool-generation errors. */
export function gatewayMemoryError(error: unknown): { model: string; blockers: string[]; raw: string } | null {
  const raw = error && typeof error === 'object' && 'raw' in error ? String(error.raw) : error instanceof Error ? error.message : String(error ?? '');
  const match = raw.match(/Not enough GPU memory to load ["\\]+([^"\\]+)["\\]+/i);
  if (!match) return null;
  const blocked = raw.match(/loaded and not evictable: ([^\n]+?)(?:\.?(?:["\\}]|$))/i);
  return { model: match[1], blockers: (blocked?.[1] ?? '').replace(/\.$/, '').split(', ').filter(Boolean), raw };
}

export function validModelEndpoint(value: string): boolean {
  try {
    const url = new URL(value);
    return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password && !url.search && !url.hash && !/\/chat\/completions\/?$/.test(url.pathname);
  } catch { return false; }
}

export type GatewayBusy = { model: string; busy: string[]; raw: string; reason: string | null; retryAfter: number | null; retryWhen: string | null };

/** The gateway's `error.harness` object of a `model_busy` 503, from a JSON body or text that embeds one. */
function busyDetails(raw: string): Record<string, unknown> | null {
  const start = raw.indexOf('{');
  if (start < 0) return null;
  for (const text of [raw.slice(start), raw]) {
    try {
      const parsed = JSON.parse(text);
      const harness = parsed?.error?.harness ?? parsed?.harness;
      if (harness && typeof harness === 'object' && Array.isArray(harness.busy_models)) return harness;
    } catch { /* not JSON */ }
  }
  return null;
}

/** Busy-model admission (JSON `harness` details, or the legacy text) can also be nested in tool-generation errors. */
export function gatewayBusyError(error: unknown): GatewayBusy | null {
  const raw = error && typeof error === 'object' && 'raw' in error ? String(error.raw) : error instanceof Error ? error.message : String(error ?? '');
  const details = busyDetails(raw);
  const retry = details ? Number(details.retry_after) : NaN;
  if (details && typeof details.model === 'string') {
    return {
      model: details.model,
      busy: (details.busy_models as unknown[]).map(String),
      raw,
      reason: typeof details.reason === 'string' ? details.reason : null,
      retryAfter: Number.isFinite(retry) && retry >= 0 ? retry : null,
      retryWhen: typeof details.retry_when === 'string' ? details.retry_when : null,
    };
  }
  const match = raw.match(/Cannot load ["\\]+([^"\\]+)["\\]+: model\(s\) (.+?) are busy; retry after their requests finish/i);
  return match ? { model: match[1], busy: match[2].split(', ').filter(Boolean), raw, reason: null, retryAfter: null, retryWhen: null } : null;
}

/** llama-server's "request exceeds the available context size" 400 (also passed through by the
 *  gateway): the request's and the server's token counts. Null for any other error. */
export function contextExceededError(error: unknown): { request: number; context: number } | null {
  const raw = typeof error === 'string' ? error : error instanceof Error ? error.message : '';
  let parsed: any = null;
  try {
    parsed = JSON.parse(raw.slice(raw.indexOf('{')));
  } catch {
    // plain text
  }
  const message = String(parsed?.error?.message ?? raw);
  const text = /request \((\d+) tokens?\) exceeds the available context size \((\d+) tokens?\)/i.exec(message);
  if (text) return { request: Number(text[1]), context: Number(text[2]) };
  const e = parsed?.error;
  if (e?.type === 'exceed_context_size_error' && Number(e.n_prompt_tokens) > 0 && Number(e.n_ctx) > 0) {
    return { request: Number(e.n_prompt_tokens), context: Number(e.n_ctx) };
  }
  return null;
}
