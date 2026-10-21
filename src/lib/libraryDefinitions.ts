// Which library files the Playground and the gateway cannot reach yet, and where a library file
// is opened in the Playground. Pure helpers (no stores, no IPC) so they can be unit-tested.
import type { LocalModel, ModelDefinition, ModelKind } from './types';
import type { TKey } from './i18n.svelte';

/** Library subtitle from the detected runtime, with file-kind fallbacks for older records. */
export function libraryKindKey(
  file: Pick<LocalModel, 'kind' | 'path' | 'architecture' | 'task'> & { gguf?: LocalModel['gguf'] },
  runtime?: string | null
): TKey {
  if (file.kind === 'llm' && file.gguf?.decision_type) return 'lib.kind_decision';
  if (runtime === 'audio.cpp' || file.architecture === 'audiocpp')
    return file.kind === 'tts' ? 'lib.kind_tts_audiocpp' : 'lib.kind_audio_process';
  if (file.kind === 'tts' && (runtime === 'crispasr' || /\.gguf$/i.test(file.path))) return 'lib.kind_tts_crispasr';
  if (runtime === 'mflux') return 'lib.kind_image_mflux';
  if (runtime === 'mlx-audio')
    return file.task === 'automatic-speech-recognition' ? 'lib.kind_asr_mlx' : 'lib.kind_tts_mlx';
  if (file.kind === 'asr') return speechKindKey(file);
  if (runtime === 'python-hf' && file.kind === 'pipeline')
    return file.task === 'text-to-image' || file.task === 'image-to-image' ? 'lib.kind_image_python' : 'lib.kind_pipeline_python';
  const keys: Record<ModelKind, TKey> = {
    llm: 'lib.kind_llm',
    embedding: 'lib.kind_embedding',
    reranker: 'lib.kind_reranker',
    asr: 'lib.kind_asr',
    image: 'lib.kind_image',
    tts: 'lib.kind_tts',
    pipeline: 'lib.kind_pipeline',
    mmproj: 'lib.kind_mmproj',
    adapter: 'lib.kind_adapter',
    unknown: 'lib.kind_unknown',
  };
  return keys[file.kind ?? 'unknown'];
}

/** Kinds that are served through a model definition but get none from "Load and Chat". */
export const DEFINITION_KINDS: readonly ModelKind[] = ['image', 'asr', 'embedding', 'reranker', 'pipeline'];

const norm = (p: string) => p.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();

/** Speech engine label from the library's detected format and architecture. */
export function speechKindKey(file: Pick<LocalModel, 'kind' | 'path' | 'architecture'>): 'lib.kind_asr' | 'lib.kind_asr_ctc' | 'lib.kind_asr_whisper' | 'lib.kind_asr_crispasr' | 'sasr.kind' {
  if (file.kind !== 'asr') return 'lib.kind_asr';
  if (/\.gguf$/i.test(file.path)) return 'lib.kind_asr_crispasr';
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
      return ['image', 'pipeline'];
    case 'transcription':
      return ['asr'];
    case 'embedding':
      return ['embedding'];
    case 'rerank':
      return ['reranker'];
    case 'ocr':
      return ['pipeline'];
    case 'pipeline':
      return ['pipeline'];
    default:
      return [];
  }
}

export type PlaygroundTab = 'embedding' | 'rerank' | 'transcription' | 'image' | 'pipeline' | 'ocr';

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
  if (file.kind === 'pipeline' && ['image-to-text', 'document-question-answering'].includes(file.task ?? ''))
    return { tab: 'ocr', pick: 'definition' };
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
      // A diffusers text-to-image or image-to-image folder (Python engine) draws: the Image tab lists it.
      return { tab: file.task === 'text-to-image' || file.task === 'image-to-image' ? 'image' : 'pipeline', pick: 'definition' };
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
