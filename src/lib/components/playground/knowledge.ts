// Knowledge bases (local RAG, harness_core::rag::kb): types mirroring the Rust side, `invoke`
// wrappers and small helpers for the Playground's Knowledge popover.
import { invoke } from '#lib/ipc.js';
import type { RagDocument } from './rag';

export type KbState = 'empty' | 'indexing' | 'ready' | 'incomplete' | 'stale';

export interface KnowledgeBase {
  id: string;
  name: string;
  folder: string | null;
  /** Embedding model (alias) of the index; null = keyword search only. */
  embed_model: string | null;
  embed_dims: number | null;
  /** OCR model (alias) that reads PDF pages without a text layer; null = none. */
  ocr_model: string | null;
  /** Total milliseconds spent indexing. */
  index_ms: number;
  indexed_at: number | null;
  created_at: number;
  updated_at: number;
  documents: number;
  chunks: number;
  embedded: number;
  failed: number;
  /** Pages of its documents read with the OCR model. */
  ocr_pages: number;
  state: KbState;
  stale_reason: 'model' | 'dims' | null;
}

export interface KbProgress {
  kb_id: string;
  status: 'indexing' | 'done' | 'cancelled';
  docs_done: number;
  docs_total: number;
  current: string | null;
  chunk_done: number;
  chunk_total: number;
  elapsed_ms: number;
}

export interface FolderPreview {
  files: number;
  skipped: number;
  truncated: boolean;
}

export interface EmbedderStatus {
  id: string;
  repo: string;
  file: string;
  size_bytes: number;
  dims: number;
  installed: boolean;
  alias: string | null;
  download_id: number | null;
  downloaded_bytes: number;
  download_status: string | null;
}

export const kbApi = {
  list: (embedModel: string | null) => invoke<KnowledgeBase[]>('kb_list', { embedModel }),
  create: (name: string, folder: string | null) => invoke<KnowledgeBase>('kb_create', { name, folder }),
  setOcrModel: (kbId: string, ocrModel: string | null) => invoke<KnowledgeBase>('kb_set_ocr_model', { kbId, ocrModel }),
  rename: (kbId: string, name: string) => invoke<void>('kb_rename', { kbId, name }),
  remove: (kbId: string) => invoke<void>('kb_delete', { kbId }),
  documents: (kbId: string) => invoke<RagDocument[]>('kb_documents', { kbId }),
  scanFolder: (folder: string) => invoke<FolderPreview>('kb_scan_folder', { folder }),
  pickFolder: () => invoke<string | null>('pick_directory_dialog'),
  index: (kbId: string, opts: { embedModel: string | null; folder?: string | null; paths?: string[]; rebuild?: boolean }) =>
    invoke<void>('kb_index', {
      kbId,
      folder: opts.folder ?? null,
      paths: opts.paths ?? null,
      embedModel: opts.embedModel,
      rebuild: opts.rebuild ?? false,
    }),
  cancel: (kbId: string) => invoke<boolean>('kb_cancel', { kbId }),
  attach: (conversationId: string, kbId: string) => invoke<void>('kb_attach', { conversationId, kbId }),
  detach: (conversationId: string, kbId: string) => invoke<void>('kb_detach', { conversationId, kbId }),
  attached: (conversationId: string, embedModel: string | null) =>
    invoke<KnowledgeBase[]>('kb_attached', { conversationId, embedModel }),
};

export const embedderApi = {
  status: () => invoke<EmbedderStatus>('embedder_status'),
  install: () => invoke<EmbedderStatus>('embedder_install'),
  sync: () => invoke<string | null>('embedder_sync'),
  remove: () => invoke<void>('embedder_remove'),
};

/** The embedder's download is queued or running. */
export function embedderBusy(s: EmbedderStatus | null): boolean {
  return !!s && !s.installed && ['queued', 'downloading', 'verifying'].includes(s.download_status ?? '');
}

/** `850 ms`, `12.3 s`, `2 min 5 s` for an indexing time. */
export function formatIndexTime(ms: number, locale: string): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toLocaleString(locale, { maximumFractionDigits: 1 })} s`;
  const m = Math.floor(s / 60);
  return `${m} min ${Math.round(s - m * 60)} s`;
}

/** Last path segment: the default name of a base made from a folder or file. */
export function baseNameOf(path: string): string {
  const parts = path.replace(/[/\\]+$/, '').split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

/** Whether the base can answer questions now (has indexed documents, no run in progress). */
export function kbUsable(kb: KnowledgeBase): boolean {
  return kb.documents > 0 && kb.state !== 'empty';
}

/** Share of the run done, 0..1 (documents finished plus the current document's chunks). */
export function runFraction(p: KbProgress | undefined): number {
  if (!p || p.docs_total === 0) return 0;
  const current = p.chunk_total > 0 ? p.chunk_done / p.chunk_total : 0;
  return Math.min(1, (p.docs_done + current) / p.docs_total);
}
