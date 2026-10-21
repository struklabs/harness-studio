import type { LocalModel, ModelDefinition } from '#lib/types.js';

/** Whether Read Document can use `def` (its library file `file`): an OCR or document-QA pipeline,
 *  a GGUF with a vision projector, or an MLX vision-language folder (mlx-vlm reads the page
 *  images itself). */
export function readsDocuments(def: ModelDefinition, file: LocalModel | undefined, files: LocalModel[] = []): boolean {
  if (['image-to-text', 'document-question-answering'].includes(file?.task ?? '')) return true;
  if (file?.vision) return true;
  const norm = (p: string) => p.replace(/\\/g, '/').toLowerCase();
  const projector = def.mmproj ? files.find(f => norm(f.path) === norm(def.mmproj!)) : undefined;
  return !!def.mmproj && !(projector?.gguf?.has_audio_encoder && !projector.gguf.has_vision_encoder);
}
