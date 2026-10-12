// Tool-call diagnosis of one gateway request (`harness_core::gateway::tool_diag`): the headline
// and the suggestion shown in the request inspector. The gateway stores a category (who is at
// fault), a finding code and a JSON evidence object; the wording lives in the locale files
// (`td.t_<code>` headline, `td.s_<code>` suggestion).
import { t, type TKey } from '$lib/i18n.svelte';
import type { RequestRow } from '$lib/types';

export type ToolCategory = 'runtime' | 'context' | 'model' | 'client';

export const TOOL_CATEGORIES: ToolCategory[] = ['runtime', 'context', 'model', 'client'];

const CATEGORY_KEY: Record<ToolCategory, TKey> = {
  runtime: 'td.cat_runtime',
  context: 'td.cat_context',
  model: 'td.cat_model',
  client: 'td.cat_client',
};

/** Context sizes the suggestions round up to (the presets of the launch settings). */
const CONTEXT_STEPS = [8192, 16384, 32768, 65536, 131072, 262144];

/** The smallest usual context that holds `needed` tokens with room for the answer, and is
 *  larger than the current one. */
export function suggestContext(needed: number | null | undefined, current: number | null | undefined): number {
  const want = Math.max((needed ?? 0) * 1.25 + 2048, (current ?? 0) * 2);
  return CONTEXT_STEPS.find((c) => c >= want) ?? CONTEXT_STEPS[CONTEXT_STEPS.length - 1];
}

/** `32768` -> `32k`. */
export function fmtContext(n: number): string {
  return n % 1024 === 0 ? `${n / 1024}k` : String(n);
}

export interface ToolDiagView {
  category: ToolCategory;
  code: string;
  /** One-line headline. */
  title: string;
  /** One-line suggestion. */
  suggestion: string;
  /** Evidence the gateway stored (tool, excerpt, schema path...). */
  detail: Record<string, unknown>;
}

export function categoryLabel(c: ToolCategory): string {
  return t(CATEGORY_KEY[c]);
}

function parseDetail(text: string | null | undefined): Record<string, unknown> {
  if (!text) return {};
  try {
    const v = JSON.parse(text);
    return v && typeof v === 'object' ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

const str = (v: unknown, fallback = ''): string => (typeof v === 'string' ? v : v == null ? fallback : String(v));
const num = (v: unknown): number | null => (typeof v === 'number' ? v : null);

/** The diagnosis of a request row, `null` for a clean round or a request without tools. */
export function toolDiagnosis(r: RequestRow): ToolDiagView | null {
  const category = r.tool_diag_category as ToolCategory | null | undefined;
  const code = r.tool_diag_code;
  if (!category || !code) return null;
  const d = parseDetail(r.tool_diag_detail);
  // The tool result of the previous round is the client's evidence: for a client finding the
  // detail itself, otherwise the `client` member a model finding carries.
  const params: Record<string, string | number> = {
    tool: str(d.tool, t('td.tool_unknown_name')),
    excerpt: str(d.excerpt),
    marker: str(d.marker),
    reason: str(d.reason),
    path: str(d.path),
    rule: str(d.rule),
    choice: str(d.tool_choice),
    times: num(d.times) ?? 0,
    tokens: num(d.completion_tokens) ?? 0,
  };
  if (code === 'context_exceeded') {
    const need = num(d.n_prompt_tokens);
    const nctx = num(d.n_ctx);
    params.tokens = need ?? 0;
    params.n_ctx = nctx ?? 0;
    params.ctx = fmtContext(suggestContext(need, nctx));
  } else if (code === 'context_near_full') {
    const fill = num(d.fill) ?? 0;
    params.fill = Math.round(fill * 100);
    params.ctx = fmtContext(suggestContext(num(d.prompt_tokens), num(d.ctx_size) ?? r.ctx_size));
  }
  return {
    category,
    code,
    title: t(`td.t_${code}` as TKey, params),
    suggestion: d.recovered === true ? t('td.s_recovered', { n: num(d.recovered_calls) ?? 0 }) : t(`td.s_${code}` as TKey, params),
    detail: d,
  };
}

/** The client's tool error that a model finding outranked, if the gateway kept it. */
export function alsoClient(d: ToolDiagView): Record<string, unknown> | null {
  const also = d.detail.also;
  if (Array.isArray(also) && also.includes('tool_result_error') && d.detail.client && typeof d.detail.client === 'object') {
    return d.detail.client as Record<string, unknown>;
  }
  return null;
}

/** The request that made the call a failed tool result answers (the detail of a client finding
 *  or its `client` member). */
export function linkedRequest(d: ToolDiagView): string | null {
  const own = d.category === 'client' ? d.detail : alsoClient(d);
  const id = own?.linked_request;
  return typeof id === 'string' ? id : null;
}
