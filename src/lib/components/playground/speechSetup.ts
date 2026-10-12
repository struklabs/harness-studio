import type { ModelDefinition, RuntimeView } from '$lib/types';
import { SHERPA_FAMILY } from './asr';
import { isSpeechDefinition } from './transcribe';

/** Keep the user's Speech selection while it is still available: a whisper definition (alias) or
 *  an installed sherpa-onnx recognition model (`sherpa`, folder names). */
export function speechModel(current: string, definitions: ModelDefinition[], sherpa: string[] = []): string {
  const all = [...definitions.filter(isSpeechDefinition).map(d => d.alias), ...sherpa];
  return all.includes(current) ? current : all[0] ?? '';
}

/** What is missing before `model` can transcribe; a sherpa-onnx model needs the sherpa-onnx runtime, a whisper one whisper.cpp. */
export function dictationRequirement(model: string, runtimes: RuntimeView[], gatewayRunning: boolean, sherpa: string[] = []): 'model' | 'runtime' | 'gateway' | null {
  if (!model) return 'model';
  const family = sherpa.includes(model) ? SHERPA_FAMILY : 'whisper.cpp';
  if (!runtimes.some(r => r.available && r.entry?.family === family)) return 'runtime';
  return gatewayRunning ? null : 'gateway';
}

/** Whether `model` (a picker value) is one of the sherpa-onnx recognition models and no whisper alias. */
export function isSherpaChoice(model: string, definitions: ModelDefinition[], sherpa: string[]): boolean {
  return sherpa.includes(model) && !definitions.some(d => isSpeechDefinition(d) && d.alias === model);
}
