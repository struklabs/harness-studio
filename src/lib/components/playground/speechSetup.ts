import type { ModelDefinition } from '#lib/types.js';
import { isSpeechDefinition, isTranscriptionDefinition, type HeaderFile } from './speechModels';

/** Keep the user's Speech selection while it is still available: a whisper definition (alias), an
 *  installed sherpa-onnx recognition model (`sherpa`, folder names), an mlx-audio one (`engine`, aliases) or a
 *  llama.cpp chat model that hears (its projector's header in `files`). */
export function speechModel(current: string, definitions: ModelDefinition[], sherpa: string[] = [], engine: string[] = [], files: HeaderFile[] = []): string {
  const all = [...definitions.filter(d => isTranscriptionDefinition(d, files)).map(d => d.alias), ...sherpa, ...engine];
  return all.includes(current) ? current : all[0] ?? '';
}

export { dictationFamily, dictationRequirement } from './speechModels';

/** Whether `model` (a picker value) is one of the sherpa-onnx recognition models and no whisper alias. */
export function isSherpaChoice(model: string, definitions: ModelDefinition[], sherpa: string[]): boolean {
  return sherpa.includes(model) && !definitions.some(d => isSpeechDefinition(d) && d.alias === model);
}
