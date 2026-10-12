import { normalizeTranscriptionText } from './transcriptionText';
// Playground Speech tab: request helper for the gateway's /v1/audio/transcriptions and
// /v1/audio/translations (a real whisper-server behind it; nothing is simulated).
import { describeErrorBody } from './sse';
import type { ModelDefinition } from '$lib/types';

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
}

/** Models a speech request can name: definitions whose file is a whisper ggml `.bin`. */
export function isSpeechFile(name: string): boolean {
  return name.toLowerCase().endsWith('.bin');
}

/** A whisper definition: served by whisper-server, never by llama-server (chat, embedding, Dashboard). */
export function isSpeechDefinition(d: ModelDefinition): boolean {
  return isSpeechFile(d.base.kind === 'local' ? d.base.path : d.base.file);
}

/** whisper starts every segment line with a space; plain text drops it. */
export function tidySegments(text: string): string {
  return normalizeTranscriptionText(text.replace(/^[ \t]+/gm, ''));
}

/** POST {baseUrl}/audio/{task} as multipart form data. */
export async function fetchTranscription(
  baseUrl: string,
  task: AudioTask,
  file: File,
  model: string,
  opts: { language: string; prompt: string; format: AudioFormat },
  headers: Record<string, string>,
  signal?: AbortSignal,
): Promise<TranscribeResult> {
  const form = new FormData();
  form.append('file', file, file.name);
  form.append('model', model);
  form.append('response_format', opts.format === 'text' || opts.format === 'srt' || opts.format === 'vtt' ? opts.format : 'verbose_json');
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
  const json = JSON.parse(body) as { text?: unknown; language?: unknown; duration?: unknown };
  return {
    text: typeof json.text === 'string' ? tidySegments(json.text) : '',
    format: opts.format,
    language: typeof json.language === 'string' ? json.language : null,
    duration: typeof json.duration === 'number' ? json.duration : null,
    ms,
  };
}
