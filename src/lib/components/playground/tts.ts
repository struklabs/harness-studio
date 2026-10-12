// Playground text to speech: request helper for the gateway's /v1/audio/speech (a real
// sherpa-onnx run behind it; nothing is simulated) and the shapes of the Tauri commands.
import { describeErrorBody } from './sse';

export interface TtsVoice {
  id: string;
  sid: number;
  language: string;
  gender: string;
}

/** An installed voice model (`harness_core::tts::TtsModel`). */
export interface TtsModelInfo {
  id: string;
  name: string;
  family: 'kokoro' | 'vits';
  languages: string[];
  quantization: string;
  sample_rate: number | null;
  size_bytes: number;
  voices: TtsVoice[];
}

/** A recommended download (`harness_core::tts::catalog::CatalogEntry` plus `installed`). */
export interface TtsCatalogRow {
  id: string;
  title: string;
  family: 'kokoro' | 'vits';
  languages: string[];
  voices: number;
  size_bytes: number;
  license: string;
  url: string;
  sha256: string;
  recommended: boolean;
  installed: boolean;
}

export interface TtsOverview {
  models: TtsModelInfo[];
  catalog: TtsCatalogRow[];
  runtime_installed: boolean;
}

export interface SpeechResult {
  blob: Blob;
  /** Model and voice the gateway used (`x-harness-model` / `x-harness-voice`). */
  model: string | null;
  voice: string | null;
  sampleRate: number | null;
  ms: number;
}

export interface SpeechRequest {
  /** Installed model name; empty lets the gateway pick the default one. */
  model: string;
  input: string;
  voice: string;
  speed: number;
}

/** POST {baseUrl}/audio/speech; answers a WAV (the one format that needs no ffmpeg). */
export async function fetchSpeech(
  baseUrl: string,
  req: SpeechRequest,
  headers: Record<string, string>,
  signal?: AbortSignal,
): Promise<SpeechResult> {
  const body: Record<string, unknown> = { input: req.input, response_format: 'wav', speed: req.speed };
  if (req.model) body.model = req.model;
  if (req.voice) body.voice = req.voice;
  const started = performance.now();
  const res = await fetch(`${baseUrl}/audio/speech`, {
    method: 'POST',
    headers: { ...headers, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
    signal,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${describeErrorBody(text) || res.statusText}`);
  }
  const blob = await res.blob();
  const rate = Number(res.headers.get('x-harness-sample-rate'));
  return {
    blob,
    model: res.headers.get('x-harness-model'),
    voice: res.headers.get('x-harness-voice'),
    sampleRate: Number.isFinite(rate) && rate > 0 ? rate : null,
    ms: performance.now() - started,
  };
}

/** The voices to offer for a model: its own list, `Default` first when it has several. */
export function voicesOf(models: TtsModelInfo[], name: string): TtsVoice[] {
  return models.find((m) => m.name === name)?.voices ?? [];
}

/** Keep the user's selection while the model is still installed, else the first one. */
export function pickTtsModel(current: string, models: TtsModelInfo[]): string {
  return models.some((m) => m.name === current) ? current : (models[0]?.name ?? '');
}

/** Keep the voice while the model has it (an empty voice means the model's default). */
export function pickTtsVoice(current: string, models: TtsModelInfo[], model: string): string {
  return current === '' || voicesOf(models, model).some((v) => v.id === current) ? current : '';
}
