import type { ModelDefinition, RuntimeView } from '#lib/types.js';

/** Models a speech request can name: definitions whose file is a whisper ggml `.bin`. */
export function isSpeechFile(name: string): boolean {
  return name.toLowerCase().endsWith('.bin');
}

/** CTC task metadata is retained by definition normalization on every save. */
export function isOnnxCtcDefinition(d: ModelDefinition): boolean {
  return d.load?.task === 'asr_ctc';
}

export function isSpeechDefinition(d: ModelDefinition): boolean {
  return isOnnxCtcDefinition(d) || isSpeechFile(d.base.kind === 'local' ? d.base.path : d.base.file);
}

/** The engine family `model` transcribes with: sherpa-onnx for its recognition models, else whisper.cpp. */
export function dictationFamily(model: string, sherpa: string[] = [], definitions: ModelDefinition[] = []): string {
  if (definitions.some(d => d.alias === model && isOnnxCtcDefinition(d))) return 'onnxruntime';
  return sherpa.includes(model) ? 'sherpa-onnx' : 'whisper.cpp';
}

/** What is missing before `model` can transcribe; a sherpa-onnx model needs the sherpa-onnx runtime, a whisper one whisper.cpp. */
export function dictationRequirement(model: string, runtimes: RuntimeView[], gatewayRunning: boolean, sherpa: string[] = [], definitions: ModelDefinition[] = []): 'model' | 'runtime' | 'gateway' | null {
  if (!model) return 'model';
  const family = dictationFamily(model, sherpa, definitions);
  if (family !== 'onnxruntime' && !runtimes.some(r => r.available && r.entry?.family === family)) return 'runtime';
  return gatewayRunning ? null : 'gateway';
}

