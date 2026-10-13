export type AudioFormat = 'wav' | 'mp3' | 'flac';
export interface PgAudio { name: string; format: AudioFormat; data: string }
export const MAX_AUDIO_BYTES = 4 * 1024 * 1024;
export const MAX_AUDIO_PER_MESSAGE = 2;

/** Trust the file signature, rather than its extension or declared MIME type. */
export function sniffAudioFormat(bytes: Uint8Array): AudioFormat | null {
  const at = (offset: number, text: string) => [...text].every((c, i) => bytes[offset + i] === c.charCodeAt(0));
  if (bytes.length >= 12 && at(0, 'RIFF') && at(8, 'WAVE')) return 'wav';
  if (bytes.length >= 3 && at(0, 'ID3')) return 'mp3';
  if (bytes.length >= 2 && bytes[0] === 0xff && (bytes[1] & 0xfe) === 0xfa) return 'mp3';
  if (bytes.length >= 4 && at(0, 'fLaC')) return 'flac';
  return null;
}
export function audioPart(audio: PgAudio): Record<string, unknown> {
  return { type: 'input_audio', input_audio: { data: audio.data, format: audio.format } };
}
export function audioUrl(audio: PgAudio): string {
  return `data:audio/${audio.format === 'mp3' ? 'mpeg' : audio.format};base64,${audio.data}`;
}
export class AudioError extends Error {
  code: 'type' | 'too_large' | 'limit';
  file: string;
  constructor(code: 'type' | 'too_large' | 'limit', file: string) { super(code); this.code = code; this.file = file; }
}
export async function loadAudio(file: File): Promise<PgAudio> {
  if (file.size > MAX_AUDIO_BYTES) throw new AudioError('too_large', file.name);
  const bytes = new Uint8Array(await file.arrayBuffer());
  const format = sniffAudioFormat(bytes);
  if (!format) throw new AudioError('type', file.name);
  let binary = '';
  for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return { name: file.name, format, data: btoa(binary) };
}
export function audioOfMeta(meta: unknown): PgAudio[] | undefined {
  const raw = (meta as { audio?: unknown } | null)?.audio;
  if (!Array.isArray(raw)) return undefined;
  const valid = raw.filter((a): a is PgAudio => !!a && typeof a.name === 'string' &&
    ['wav', 'mp3', 'flac'].includes(a.format) && typeof a.data === 'string' &&
    a.data.length <= Math.ceil(MAX_AUDIO_BYTES / 3) * 4 && /^[A-Za-z0-9+/]+={0,2}$/.test(a.data));
  return valid.length ? valid.slice(0, MAX_AUDIO_PER_MESSAGE) : undefined;
}
