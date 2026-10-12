// Playground Embedding / Rerank tabs: request helpers for llama-server's /v1/embeddings and
// /v1/rerank (real requests against the selected server; nothing is simulated).
import { describeError } from '$lib/gateway-busy-text';

/** A 200 answer without usable vectors / scores (e.g. `null`s from a rerank server asked to embed). */
export class InvalidResponseError extends Error {
  constructor() {
    super('invalid_response');
  }
}

export interface EmbeddingResult {
  vectors: number[][];
  dims: number;
  promptTokens: number | null;
  model: string | null;
  ms: number;
}

export interface RerankItem {
  index: number;
  score: number;
}

export interface RerankResult {
  items: RerankItem[];
  promptTokens: number | null;
  model: string | null;
  ms: number;
}

async function postJson(url: string, body: unknown, headers: Record<string, string>, signal?: AbortSignal) {
  const started = performance.now();
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...headers },
    body: JSON.stringify(body),
    signal,
  });
  const text = await res.text();
  const ms = performance.now() - started;
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${describeError(text) || res.statusText}`);
  return { json: JSON.parse(text) as Record<string, unknown>, ms };
}

function tokens(json: Record<string, unknown>): number | null {
  const usage = json.usage as { prompt_tokens?: unknown } | undefined;
  return typeof usage?.prompt_tokens === 'number' ? usage.prompt_tokens : null;
}

/** POST {baseUrl}/embeddings (OpenAI shape: `data[].embedding`, ordered by `index`). */
export async function fetchEmbeddings(
  baseUrl: string,
  input: string[],
  model: string,
  headers: Record<string, string>,
  signal?: AbortSignal
): Promise<EmbeddingResult> {
  const body: Record<string, unknown> = { input, encoding_format: 'float' };
  if (model) body.model = model;
  const { json, ms } = await postJson(`${baseUrl}/embeddings`, body, headers, signal);
  const data = Array.isArray(json.data) ? (json.data as { index?: number; embedding?: unknown }[]) : [];
  const sorted = [...data].sort((a, b) => (a.index ?? 0) - (b.index ?? 0));
  const vectors = sorted.map((d) => d.embedding);
  if (vectors.length !== input.length || !vectors.every((v) => Array.isArray(v) && v.length > 0 && v.every((x) => typeof x === 'number'))) {
    throw new InvalidResponseError();
  }
  const vecs = vectors as number[][];
  return { vectors: vecs, dims: vecs[0]?.length ?? 0, promptTokens: tokens(json), model: typeof json.model === 'string' ? json.model : null, ms };
}

/** POST {baseUrl}/rerank (llama-server: `results[].index` + `relevance_score`), best first. */
export async function fetchRerank(
  baseUrl: string,
  query: string,
  documents: string[],
  model: string,
  headers: Record<string, string>,
  signal?: AbortSignal
): Promise<RerankResult> {
  const body: Record<string, unknown> = { query, documents, top_n: documents.length };
  if (model) body.model = model;
  const { json, ms } = await postJson(`${baseUrl}/rerank`, body, headers, signal);
  const results = Array.isArray(json.results) ? (json.results as { index?: unknown; relevance_score?: unknown }[]) : null;
  if (!results || !results.every((r) => typeof r.index === 'number' && typeof r.relevance_score === 'number')) {
    throw new InvalidResponseError();
  }
  const items = results
    .map((r) => ({ index: r.index as number, score: r.relevance_score as number }))
    .sort((a, b) => b.score - a.score);
  return { items, promptTokens: tokens(json), model: typeof json.model === 'string' ? json.model : null, ms };
}

export function cosine(a: number[], b: number[]): number {
  let dot = 0;
  let na = 0;
  let nb = 0;
  for (let i = 0; i < a.length && i < b.length; i++) {
    dot += a[i] * b[i];
    na += a[i] * a[i];
    nb += b[i] * b[i];
  }
  return na > 0 && nb > 0 ? dot / Math.sqrt(na * nb) : 0;
}

export function similarityMatrix(vectors: number[][]): number[][] {
  return vectors.map((a) => vectors.map((b) => cosine(a, b)));
}
