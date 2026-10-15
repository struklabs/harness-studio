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

/** A library file as the speech helpers read it: its path and GGUF header. */
export type HeaderFile = { path: string; gguf?: { has_audio_encoder?: boolean; audio_projector?: string | null } | null };

const normalPath = (p: string) => p.replace(/[\\/]+/g, '/').toLowerCase();
const samePath = (a: string, b: string) => normalPath(a) === normalPath(b);

/** The audio projector of a llama.cpp chat model that hears (LFM2.5-Audio, Qwen3-ASR, Voxtral...): its
 *  `mmproj` has an audio encoder. Mirrors `asr::llm::projector_of`; the gateway transcribes with it. */
export function audioLlmProjector(d: ModelDefinition, files: HeaderFile[] = []): string | null {
  const mmproj = d.mmproj;
  // Like the gateway: a local GGUF model with a projector file.
  if (!mmproj || d.base.kind !== 'local' || !d.base.path.toLowerCase().endsWith('.gguf')) return null;
  const gguf = files.find(f => samePath(f.path, mmproj))?.gguf;
  const projector = gguf?.has_audio_encoder ? gguf.audio_projector || 'audio' : null;
  // A speaker encoder (Qwen3-TTS's `qwen3tts_spkenc`) reads a voice sample, it does not hear speech.
  return projector && !projector.endsWith('_spkenc') ? projector : null;
}

export function isAudioLlmDefinition(d: ModelDefinition, files: HeaderFile[] = []): boolean {
  return audioLlmProjector(d, files) !== null;
}

/** Mirrors `asr::llm::translates`: LFM2.5-Audio and Qwen3-ASR only transcribe. */
export function audioLlmTranslates(d: ModelDefinition, files: HeaderFile[] = []): boolean {
  const projector = audioLlmProjector(d, files);
  return projector !== null && projector !== 'lfm2a' && projector !== 'qwen3asr';
}

/** Definitions the Transcribe panel and dictation offer: speech models, and chat models that hear. */
export function isTranscriptionDefinition(d: ModelDefinition, files: HeaderFile[] = []): boolean {
  return isSpeechDefinition(d) || isAudioLlmDefinition(d, files);
}

export function isSpeechDefinition(d: ModelDefinition): boolean {
  return d.load?.task === 'crispasr' || isOnnxCtcDefinition(d) || isSpeechFile(d.base.kind === 'local' ? d.base.path : d.base.file);
}

/** The engine family `model` transcribes with: sherpa-onnx for its recognition models, mlx-audio for
 *  the library's MLX ones (`engine`, aliases), llama.cpp for chat models that hear, else whisper.cpp. */
export function dictationFamily(model: string, sherpa: string[] = [], definitions: ModelDefinition[] = [], engine: string[] = [], files: HeaderFile[] = []): string {
  if (definitions.some(d => d.alias === model && d.load?.task === 'crispasr')) return 'crispasr';
  if (definitions.some(d => d.alias === model && isAudioLlmDefinition(d, files))) return 'llama.cpp';
  if (definitions.some(d => d.alias === model && isOnnxCtcDefinition(d))) return 'onnxruntime';
  if (engine.includes(model)) return 'mlx-audio';
  return sherpa.includes(model) ? 'sherpa-onnx' : 'whisper.cpp';
}

/** Families whose environment the app manages itself: no runtime to register first. */
const MANAGED_FAMILIES = ['onnxruntime', 'mlx-audio'];

/** What is missing before `model` can transcribe; a sherpa-onnx model needs the sherpa-onnx runtime, a whisper one whisper.cpp. */
export function dictationRequirement(model: string, runtimes: RuntimeView[], gatewayRunning: boolean, sherpa: string[] = [], definitions: ModelDefinition[] = [], engine: string[] = [], files: HeaderFile[] = []): 'model' | 'runtime' | 'gateway' | null {
  if (!model) return 'model';
  const family = dictationFamily(model, sherpa, definitions, engine, files);
  if (!MANAGED_FAMILIES.includes(family) && !runtimes.some(r => r.available && r.entry?.family === family)) return 'runtime';
  return gatewayRunning ? null : 'gateway';
}

