// Per-token probabilities of an answer (OpenAI `logprobs`, streamed by llama-server as
// `choices[].logprobs.content`). Kept in memory for the session only: they are not stored with
// the conversation (a long answer with alternatives per token would bloat state.db).

export interface TokenAlternative {
  token: string;
  logprob: number;
}

export interface TokenLogprob extends TokenAlternative {
  /** Most likely alternatives at this position (includes the chosen token when it ranks). */
  top: TokenAlternative[];
}

function num(v: unknown): number | null {
  // JSON cannot carry -Infinity; llama-server may send a very negative number or null.
  return typeof v === 'number' && !Number.isNaN(v) ? v : null;
}

function alternative(raw: unknown): TokenAlternative | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as { token?: unknown; logprob?: unknown };
  const logprob = num(r.logprob);
  if (typeof r.token !== 'string' || logprob === null) return null;
  return { token: r.token, logprob };
}

/** Token entries of one streamed choice (or non-streamed message choice); `[]` when it has none. */
export function parseChoiceLogprobs(choice: unknown): TokenLogprob[] {
  const content = (choice as { logprobs?: { content?: unknown } | null } | null | undefined)?.logprobs?.content;
  if (!Array.isArray(content)) return [];
  const out: TokenLogprob[] = [];
  for (const raw of content) {
    const base = alternative(raw);
    if (!base) continue;
    const topRaw = (raw as { top_logprobs?: unknown }).top_logprobs;
    const top = Array.isArray(topRaw)
      ? topRaw.map(alternative).filter((x): x is TokenAlternative => x !== null)
      : [];
    out.push({ ...base, top });
  }
  return out;
}

/** Probability (0-1) of a log probability. */
export function probability(logprob: number): number {
  return Math.min(1, Math.max(0, Math.exp(logprob)));
}

/** Percent text of a probability: `97.3%`, `<0.1%` for tiny values. */
export function formatProbability(logprob: number, locale?: string): string {
  const pct = probability(logprob) * 100;
  if (pct > 0 && pct < 0.1) return '<0.1%';
  return `${pct.toLocaleString(locale, { maximumFractionDigits: 1, minimumFractionDigits: 1 })}%`;
}

/**
 * Tint strength 0-1 of a token: 0 when it was (nearly) certain, growing as the model was less
 * sure of the token it produced.
 */
export function uncertainty(logprob: number): number {
  return 1 - probability(logprob);
}

const SESSION_LIMIT = 100;
const session = new Map<string, TokenLogprob[]>();

/** Remembers the probabilities of an answer (by variant id) for the rest of the session. */
export function rememberLogprobs(variantId: string, tokens: TokenLogprob[]): void {
  session.delete(variantId);
  if (!tokens.length) return;
  session.set(variantId, tokens);
  while (session.size > SESSION_LIMIT) {
    const oldest = session.keys().next().value;
    if (oldest === undefined) break;
    session.delete(oldest);
  }
}

export function recalledLogprobs(variantId: string | undefined): TokenLogprob[] | undefined {
  return variantId ? session.get(variantId) : undefined;
}
