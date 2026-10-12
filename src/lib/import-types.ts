// Types for importing models from Ollama / LM Studio (see src-tauri/src/importer.rs).
import type { LocalModel } from '$lib/types';

export type ImportSource = 'ollama' | 'lmstudio';
export type ImportMode = 'link' | 'copy';

/** A GGUF found in an Ollama / LM Studio model store. */
export interface ImportCandidate {
  source: ImportSource;
  /** `llama3.1:8b` (Ollama) or `publisher/repo/file` (LM Studio). */
  name: string;
  path: string;
  size_bytes: number;
  /** Vision projector belonging to this model, if any. */
  mmproj_path: string | null;
  architecture: string | null;
  quantization: string | null;
  /** Already linked or copied. */
  imported: boolean;
}

/** Result of `scan_import_sources`. */
export interface ImportScan {
  ollama_dir: string | null;
  lmstudio_dir: string | null;
  candidates: ImportCandidate[];
}

/** One entry of `import_models`' result. */
export interface ImportResult {
  name: string;
  ok: boolean;
  /** 'linked' | 'hardlinked' | 'copied' | error message */
  detail: string;
  /** Model id (launch path) in the library after import. */
  model_id: string | null;
}

/** LocalModel plus the backend fields describing a model linked in place. */
export type LocalModelExt = LocalModel & {
  source?: ImportSource | null;
  mmproj_path?: string | null;
};

/** Display name of an import source. */
export function sourceLabel(source: ImportSource): string {
  return source === 'ollama' ? 'Ollama' : 'LM Studio';
}
