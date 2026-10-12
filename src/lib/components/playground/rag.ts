// Playground documents (local RAG, harness_core::rag): types mirroring the Rust API, `invoke`
// wrappers and small helpers (citations, page labels, applying the context to a request).
import { invoke } from '#lib/ipc.js';

export type RagStatus = 'pending' | 'extracting' | 'embedding' | 'ready' | 'failed' | 'cancelled';
export type RagMode = 'auto' | 'full' | 'retrieval';

export interface RagDocument {
  id: string;
  conversation_id: string;
  name: string;
  kind: 'pdf' | 'docx' | 'markdown' | 'code' | 'text';
  source_path: string | null;
  size: number;
  sha256: string;
  pages: number | null;
  chars: number;
  /** Estimated tokens of the whole text. */
  tokens: number;
  chunks: number;
  embedded: number;
  status: RagStatus;
  error: string | null;
  mode: RagMode;
  embed_model: string | null;
  embed_dims: number | null;
  created_at: number;
  updated_at: number;
}

export interface RetrievalModel {
  definition_id: string;
  alias: string;
  role: 'embedding' | 'rerank';
  file: string;
  dims: number | null;
  context_length: number | null;
}

export interface RagModels {
  embedding: RetrievalModel[];
  rerank: RetrievalModel[];
  gateway: boolean;
}

export interface RagProgress {
  conversation_id: string;
  document_id: string;
  status: RagStatus;
  done: number;
  total: number;
  error: string | null;
}

export interface RagSource {
  /** Citation number; 0 = a whole document (full text). */
  n: number;
  document_id: string;
  document_name: string;
  chunk_id: number | null;
  page_start: number | null;
  page_end: number | null;
  heading: string | null;
  text: string;
  similarity: number | null;
  rerank_score: number | null;
  full: boolean;
}

export interface RagPrepared {
  system: string | null;
  user: string | null;
  sources: RagSource[];
  retrieval: 'hybrid' | 'keyword' | null;
  reranked: string | null;
  notices: string[];
  tokens: number;
}

/** What a turn used, stored in the assistant variant's `meta.rag`. */
export interface RagTurn {
  sources: RagSource[];
  retrieval: RagPrepared['retrieval'];
  reranked: string | null;
  notices: string[];
}

/** Embedding choice: '' = the first embedding model of the library, 'none' = keywords only. */
export const EMBED_NONE = 'none';
export const DEFAULT_TOP_K = 5;

export const ragApi = {
  models: () => invoke<RagModels>('rag_models'),
  list: (conversationId: string) => invoke<RagDocument[]>('rag_list_documents', { conversationId }),
  pick: (label: string) => invoke<string[]>('rag_pick_files', { label }),
  add: (conversationId: string, paths: string[], embedModel: string | null) =>
    invoke<RagDocument[]>('rag_add_files', { conversationId, paths, embedModel }),
  reindex: (conversationId: string, embedModel: string | null, documentIds?: string[]) =>
    invoke<string[]>('rag_reindex', { conversationId, embedModel, documentIds: documentIds ?? null }),
  cancel: (documentId: string) => invoke<boolean>('rag_cancel', { documentId }),
  remove: (documentId: string) => invoke<void>('rag_delete_document', { documentId }),
  setMode: (documentId: string, mode: RagMode) => invoke<RagDocument>('rag_set_document_mode', { documentId, mode }),
  prepare: (input: {
    conversation_id: string;
    query: string;
    top_k: number;
    context_tokens: number | null;
    embed_model: string | null;
    rerank_model: string | null;
  }) => invoke<RagPrepared>('rag_prepare', { input }),
};

/** Alias to embed with: the chosen one if it still exists, else the first one ('' = auto). */
export function resolveEmbedModel(choice: string, models: RetrievalModel[]): string | null {
  if (choice === EMBED_NONE) return null;
  if (choice && models.some((m) => m.alias === choice)) return choice;
  return models[0]?.alias ?? null;
}

export function isActive(d: RagDocument): boolean {
  return d.status === 'pending' || d.status === 'extracting' || d.status === 'embedding';
}

/** `p. 3`, `pp. 3–4` (localized by the caller's templates). */
export function pageRange(start: number | null, end: number | null): { one: number } | { from: number; to: number } | null {
  if (start == null) return null;
  return end != null && end !== start ? { from: start, to: end } : { one: start };
}

/** Citation numbers used in an answer (`[1]`, `[2, 3]`, `[1][4]`). */
export function citedNumbers(text: string): Set<number> {
  const out = new Set<number>();
  for (const m of text.matchAll(/\[(\d{1,2}(?:\s*,\s*\d{1,2})*)\]/g)) {
    for (const n of m[1].split(',')) out.add(Number(n.trim()));
  }
  return out;
}

/** Adds the document context to OpenAI-format messages (full text in the system message,
 *  excerpts in place of the last user message). */
export function applyRag(messages: Record<string, unknown>[], rag: RagPrepared | null): Record<string, unknown>[] {
  if (!rag || (!rag.system && !rag.user)) return messages;
  const out = messages.map((m) => ({ ...m }));
  if (rag.user) {
    for (let i = out.length - 1; i >= 0; i--) {
      if (out[i].role === 'user') {
        // A message with pictures keeps them; only its text is replaced.
        const parts = out[i].content;
        out[i].content = Array.isArray(parts)
          ? [{ type: 'text', text: rag.user }, ...parts.filter((p) => (p as { type?: unknown })?.type === 'image_url')]
          : rag.user;
        break;
      }
    }
  }
  if (rag.system) {
    const first = out[0];
    if (first?.role === 'system' && typeof first.content === 'string') {
      first.content = `${first.content}\n\n${rag.system}`;
    } else {
      out.unshift({ role: 'system', content: rag.system });
    }
  }
  return out;
}

export function ragTurnOf(meta: unknown): RagTurn | undefined {
  const rag = (meta as { rag?: unknown } | null | undefined)?.rag as Partial<RagTurn> | undefined;
  if (!rag || !Array.isArray(rag.sources)) return undefined;
  return {
    sources: rag.sources.filter((s): s is RagSource => !!s && typeof s === 'object' && typeof (s as RagSource).document_name === 'string'),
    retrieval: rag.retrieval ?? null,
    reranked: rag.reranked ?? null,
    notices: Array.isArray(rag.notices) ? rag.notices.filter((n): n is string => typeof n === 'string') : [],
  };
}

