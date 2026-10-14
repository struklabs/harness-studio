// Which library files the Playground and the gateway cannot reach yet, and where a library file
// is opened in the Playground. Pure helpers (no stores, no IPC) so they can be unit-tested.
import type { LocalModel, ModelDefinition, ModelKind } from './types';

/** Kinds that are served through a model definition but get none from "Load and Chat". */
export const DEFINITION_KINDS: readonly ModelKind[] = ['image', 'asr', 'embedding', 'reranker', 'pipeline'];

const norm = (p: string) => p.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();

/** Speech engine label from the library's detected format and architecture. */
export function speechKindKey(file: Pick<LocalModel, 'kind' | 'path' | 'architecture'>): 'lib.kind_asr' | 'lib.kind_asr_ctc' | 'lib.kind_asr_whisper' | 'sasr.kind' {
  if (file.kind !== 'asr') return 'lib.kind_asr';
  if (isOnnxAsr(file)) return 'sasr.kind';
  if (/\.onnx$/i.test(file.path) && file.architecture === 'onnx') return 'lib.kind_asr_ctc';
  return /\.bin$/i.test(file.path) ? 'lib.kind_asr_whisper' : 'lib.kind_asr';
}

/** A sherpa-onnx recognition model (ONNX): run per request, no definition. whisper.cpp's are ggml `.bin`. */
function isOnnxAsr(file: Pick<LocalModel, 'kind' | 'path' | 'architecture'>): boolean {
  return file.kind === 'asr' && file.architecture !== 'onnx' && /\.onnx$/i.test(file.path);
}

/** Ready library files of `kinds` that no definition points at: the ones the pickers and `/v1/models` do not list yet. */
export function filesNeedingDefinition(
  files: readonly LocalModel[],
  definitions: readonly ModelDefinition[],
  kinds: readonly ModelKind[],
): LocalModel[] {
  const taken = new Set(definitions.flatMap((d) => (d.base.kind === 'local' ? [norm(d.base.path)] : [])));
  return files.filter(
    (f) =>
      f.is_ready &&
      !!f.kind &&
      kinds.includes(f.kind) &&
      DEFINITION_KINDS.includes(f.kind) &&
      !isOnnxAsr(f) &&
      !taken.has(norm(f.path)),
  );
}

/** The kinds a Playground tab lists. */
export function kindsForTab(tab: string): ModelKind[] {
  switch (tab) {
    case 'image':
      return ['image'];
    case 'transcription':
      return ['asr'];
    case 'embedding':
      return ['embedding'];
    case 'rerank':
      return ['reranker'];
    case 'pipeline':
      return ['pipeline'];
    default:
      return [];
  }
}

export type PlaygroundTab = 'embedding' | 'rerank' | 'transcription' | 'image' | 'pipeline';

/**
 * The Playground tab a library file opens in, and whether it is picked by definition alias
 * (`definition`) or by the name its engine lists it under (`name`: Kokoro / Piper voices and
 * sherpa-onnx recognition models, which run per request). Null for files without a tab.
 */
export function playgroundTarget(
  file: Pick<LocalModel, 'kind' | 'path' | 'is_ready' | 'architecture' | 'task'>,
  /** Runtime family of the file (`LibraryEntry.record.runtime`). */
  runtime?: string | null,
): { tab: PlaygroundTab; pick: 'definition' | 'name'; speech?: 'transcribe' | 'speak' } | null {
  if (!file.is_ready) return null;
  // An mlx-audio folder speaks or transcribes: the Speech tab lists it under its definition's alias.
  if (runtime === 'mlx-audio')
    return { tab: 'transcription', pick: 'name', speech: file.task === 'automatic-speech-recognition' ? 'transcribe' : 'speak' };
  switch (file.kind) {
    case 'image':
      return { tab: 'image', pick: 'definition' };
    case 'asr':
      return isOnnxAsr(file)
        ? { tab: 'transcription', pick: 'name', speech: 'transcribe' }
        : { tab: 'transcription', pick: 'definition', speech: 'transcribe' };
    case 'tts':
      return { tab: 'transcription', pick: 'name', speech: 'speak' };
    case 'embedding':
      return { tab: 'embedding', pick: 'definition' };
    case 'reranker':
      return { tab: 'rerank', pick: 'definition' };
    case 'pipeline':
      return { tab: 'pipeline', pick: 'definition' };
    default:
      return null;
  }
}

/**
 * Name of the model a server runs, for the Playground's picker: the alias of the definition
 * that points at `path`, else the file name. An ONNX export's file is named after its precision
 * (`onnx/model_quantized.onnx`), so its repo folder names it instead.
 */
export function servedModelName(path: string, definitions: readonly ModelDefinition[]): string {
  const want = norm(path);
  const def = definitions.find((d) => d.base.kind === 'local' && norm(d.base.path) === want);
  if (def) return def.alias;
  const parts = path.split(/[/\\]/).filter(Boolean);
  const file = parts[parts.length - 1] ?? path;
  if (!/\.onnx$/i.test(file)) return file.replace(/\.gguf$/i, '');
  const dirs = parts.slice(0, -1);
  if (dirs.length > 0 && dirs[dirs.length - 1].toLowerCase() === 'onnx') dirs.pop();
  return dirs[dirs.length - 1] ?? file;
}
