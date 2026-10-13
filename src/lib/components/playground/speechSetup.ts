import type { ModelDefinition } from '$lib/types';
import { isSpeechDefinition } from './speechModels';

/** Keep the user's Speech selection while it is still available: a whisper definition (alias) or
 *  an installed sherpa-onnx recognition model (`sherpa`, folder names). */
export function speechModel(current: string, definitions: ModelDefinition[], sherpa: string[] = []): string {
  const all = [...definitions.filter(isSpeechDefinition).map(d => d.alias), ...sherpa];
  return all.includes(current) ? current : all[0] ?? '';
}

export { dictationFamily, dictationRequirement } from './speechModels';

/** Whether `model` (a picker value) is one of the sherpa-onnx recognition models and no whisper alias. */
export function isSherpaChoice(model: string, definitions: ModelDefinition[], sherpa: string[]): boolean {
  return sherpa.includes(model) && !definitions.some(d => isSpeechDefinition(d) && d.alias === model);
}
