import { normalizeTranscriptionText } from './transcriptionText';
// Playground Speech tab: request helper for the gateway's /v1/audio/transcriptions and
// /v1/audio/translations (a real whisper-server behind it; nothing is simulated).
import { describeErrorBody } from './sse';

export type AudioTask = 'transcriptions' | 'translations';
export type AudioFormat = 'json' | 'text' | 'srt' | 'vtt' | 'verbose_json';

export interface TranscribeResult {
  /** The text (or subtitle file) as the server answered it. */
  text: string;
  format: AudioFormat;
  /** `language` and `duration` of a verbose_json answer. */
  language: string | null;
  duration: number | null;
  ms: number;
  segments?: { start: number; end: number; text: string }[];
}

export { isSpeechFile, isSpeechDefinition, isOnnxCtcDefinition, isCrispasrDefinition, crispasrCanTranslate } from './speechModels';

/** whisper starts every segment line with a space; plain text drops it. */
export function tidySegments(text: string): string {
  return normalizeTranscriptionText(text.replace(/^[ \t]+/gm, ''));
}

/** POST {baseUrl}/audio/{task} as multipart form data. `json` asks for verbose_json (language and
 *  duration) unless `textOnly`: a model whose worker answers the text only (mlx-audio) takes
 *  plain json. */
export async function fetchTranscription(
  baseUrl: string,
  task: AudioTask,
  file: File,
  model: string,
  opts: { language: string; prompt: string; format: AudioFormat; textOnly?: boolean },
  headers: Record<string, string>,
  signal?: AbortSignal,
): Promise<TranscribeResult> {
  const form = new FormData();
  form.append('file', file, file.name);
  form.append('model', model);
  const asked = opts.format === 'json' && !opts.textOnly ? 'verbose_json' : opts.format;
  form.append('response_format', asked);
  if (opts.language.trim()) form.append('language', opts.language.trim());
  if (opts.prompt.trim()) form.append('prompt', opts.prompt.trim());
  const started = performance.now();
  const res = await fetch(`${baseUrl}/audio/${task}`, { method: 'POST', headers, body: form, signal });
  const body = await res.text();
  const ms = performance.now() - started;
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${describeErrorBody(body) || res.statusText}`);
  if (opts.format === 'srt' || opts.format === 'vtt') {
    return { text: body, format: opts.format, language: null, duration: null, ms };
  }
  if (opts.format === 'text') return { text: tidySegments(body), format: opts.format, language: null, duration: null, ms };
  const json = JSON.parse(body) as { text?: unknown; language?: unknown; duration?: unknown; segments?: unknown };
  return {
    text: typeof json.text === 'string' ? tidySegments(json.text) : '',
    format: opts.format,
    language: typeof json.language === 'string' ? json.language : null,
    duration: typeof json.duration === 'number' ? json.duration : null,
    ms,
    segments: Array.isArray(json.segments) ? json.segments.filter((s): s is { start: number; end: number; text: string } =>
      s && typeof s.start === 'number' && typeof s.end === 'number' && typeof s.text === 'string') : [],
  };
}
