import type { ModelDefinition, RuntimeView } from '#lib/types.js';

/** Models a speech request can name: definitions whose file is a whisper ggml `.bin`. */
export function isSpeechFile(name: string): boolean {
  return name.toLowerCase().endsWith('.bin');
}

/** CTC task metadata is retained by definition normalization on every save. */
export function isOnnxCtcDefinition(d: ModelDefinition): boolean {
  return d.load?.task === 'asr_ctc';
}

/** Mirrors `engine::crispasr::can_translate`: the architectures whose backend translates. */
const CRISPASR_TRANSLATE_ARCHS = new Set([
  'canary', 'cohere', 'cohere-transcribe', 'voxtral', 'voxtral4b', 'voxtral-4b', 'voxtral_4b',
  'granitespeech', 'granite-speech', 'granite_speech',
]);

export function isCrispasrDefinition(d: ModelDefinition): boolean {
  return d.load?.task === 'crispasr';
}

/** Whether a CrispASR definition can translate; an unknown file or architecture cannot. */
export function crispasrCanTranslate(d: ModelDefinition, files: { path: string; architecture: string | null }[]): boolean {
  if (!isCrispasrDefinition(d) || d.base.kind !== 'local') return false;
  const path = d.base.path;
  const arch = files.find(f => f.path === path)?.architecture?.toLowerCase();
  return !!arch && CRISPASR_TRANSLATE_ARCHS.has(arch);
}

export function isSpeechDefinition(d: ModelDefinition): boolean {
  return d.load?.task === 'crispasr' || isOnnxCtcDefinition(d) || isSpeechFile(d.base.kind === 'local' ? d.base.path : d.base.file);
}

/** The engine family `model` transcribes with: sherpa-onnx for its recognition models, mlx-audio for
 *  the library's MLX ones (`engine`, aliases), else whisper.cpp. */
export function dictationFamily(model: string, sherpa: string[] = [], definitions: ModelDefinition[] = [], engine: string[] = []): string {
  if (definitions.some(d => d.alias === model && d.load?.task === 'crispasr')) return 'crispasr';
  if (definitions.some(d => d.alias === model && isOnnxCtcDefinition(d))) return 'onnxruntime';
  if (engine.includes(model)) return 'mlx-audio';
  return sherpa.includes(model) ? 'sherpa-onnx' : 'whisper.cpp';
}

/** Families whose environment the app manages itself: no runtime to register first. */
const MANAGED_FAMILIES = ['onnxruntime', 'mlx-audio'];

/** What is missing before `model` can transcribe; a sherpa-onnx model needs the sherpa-onnx runtime, a whisper one whisper.cpp. */
export function dictationRequirement(model: string, runtimes: RuntimeView[], gatewayRunning: boolean, sherpa: string[] = [], definitions: ModelDefinition[] = [], engine: string[] = []): 'model' | 'runtime' | 'gateway' | null {
  if (!model) return 'model';
  const family = dictationFamily(model, sherpa, definitions, engine);
  if (!MANAGED_FAMILIES.includes(family) && !runtimes.some(r => r.available && r.entry?.family === family)) return 'runtime';
  return gatewayRunning ? null : 'gateway';
}

