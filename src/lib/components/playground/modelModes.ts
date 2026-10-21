import type { LocalModel, ModelDefinition, PlaygroundMode, ServeMode } from '#lib/types.js';
import { chatModelsOnly } from '../../chatModels';
import { DIFFUSERS_IMAGE_TASKS } from './image';

/** The library file a definition points at, when it is in `files`. */
export function definitionFile(def: ModelDefinition, files: LocalModel[] = []): LocalModel | undefined {
  const path = def.base.kind === 'local' ? def.base.path : def.base.file;
  const normalize = (value: string) => value.replace(/\\/g, '/').toLowerCase();
  return files.find(file => normalize(file.path) === normalize(path)
    || (def.base.kind === 'hf' && normalize(file.path).endsWith(`/${def.base.repo.toLowerCase()}/${normalize(path)}`)));
}

export function definitionMode(def: ModelDefinition, files: LocalModel[] = []): ServeMode {
  const path = def.base.kind === 'local' ? def.base.path : def.base.file;
  const file = definitionFile(def, files);
  if (def.load?.task === 'asr_ctc' || file?.kind === 'asr' || /\.bin$/i.test(path)) return 'transcription';
  // A diffusers folder of the Python engine draws through the Image tab like sd.cpp's files.
  if (file?.kind === 'pipeline') return DIFFUSERS_IMAGE_TASKS.includes(file.task ?? '') ? 'image' : 'pipeline';
  if (file?.kind === 'image' || /\.(safetensors|ckpt)$/i.test(path)) return 'image';
  const mode = def.load?.serve_mode;
  if (file?.kind === 'reranker' || mode === 'rerank' || def.load?.rerank === true) return 'rerank';
  if (file?.kind === 'embedding' || mode === 'embedding' || def.load?.embedding === true || def.load?.embeddings === true) return 'embedding';
  return 'chat';
}

export function definitionsForMode(definitions: ModelDefinition[], mode: PlaygroundMode, files: LocalModel[] = []): ModelDefinition[] {
  if (mode === 'ocr') return definitions.filter(def => {
    const file = definitionFile(def, files);
    if (['image-to-text', 'document-question-answering'].includes(file?.task ?? '')) return true;
    const projector = files.find(f => f.path.replace(/\\/g, '/').toLowerCase() === def.mmproj?.replace(/\\/g, '/').toLowerCase());
    return !!def.mmproj && !(projector?.gguf?.has_audio_encoder && !projector.gguf.has_vision_encoder);
  });
  const rows = definitions.map(def => ({ def, mode: definitionMode(def, files), chat: definitionMode(def, files) === 'chat' }));
  return (mode === 'chat' ? chatModelsOnly(rows) : rows.filter(row => row.mode === mode)).map(row => row.def);
}

/** Served ids that can chat: known non-chat definitions (speech, embedding, rerank) drop out, ids of foreign endpoints stay. */
export function chatModelIds(ids: string[], definitions: ModelDefinition[], files: LocalModel[] = []): string[] {
  const nonChat = new Set(definitions.filter(def => definitionMode(def, files) !== 'chat').map(def => def.alias));
  return ids.filter(id => !nonChat.has(id));
}

export function selectModeModel(previous: string, available: string[]): string {
  return available.includes(previous) ? previous : available[0] ?? '';
}
