// "Speak Replies": a finished chat answer is chained into /v1/audio/speech and played, so
// audio in -> audio-capable chat model -> text-to-speech out is a spoken conversation.
export const SPEAK_REPLIES_KEY = 'harness.pg.speakReplies';

export interface ReplyState {
  content: string;
  error?: string;
  stopped?: boolean;
  toolCalls?: unknown[];
}

/** The text to speak for a finished reply, or null when it must stay silent (switched off, a
 *  failed, stopped or tool-calling turn, or nothing but whitespace). */
export function replySpeechSource(enabled: boolean, reply: ReplyState): string | null {
  if (!enabled || reply.error || reply.stopped || reply.toolCalls?.length) return null;
  return reply.content.trim() ? reply.content : null;
}

/** The recording of a streamed `delta.audio` / `message.audio` (OpenAI: base64 WAV in `data`), or
 *  null when the value carries none. */
export function replyAudio(value: unknown, mime = 'audio/wav'): Blob | null {
  const data = (value as { data?: unknown } | null | undefined)?.data;
  if (typeof data !== 'string' || data === '') return null;
  try {
    const raw = atob(data);
    const bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
    return new Blob([bytes], { type: mime });
  } catch {
    return null;
  }
}

export function readSpeakReplies(storage: Pick<Storage, 'getItem'> | undefined): boolean {
  try {
    return storage?.getItem(SPEAK_REPLIES_KEY) === '1';
  } catch {
    return false;
  }
}

export function writeSpeakReplies(storage: Pick<Storage, 'setItem' | 'removeItem'> | undefined, on: boolean): void {
  try {
    if (on) storage?.setItem(SPEAK_REPLIES_KEY, '1');
    else storage?.removeItem(SPEAK_REPLIES_KEY);
  } catch {
    /* a per-viewer convenience; storage may be blocked */
  }
}

export type SpeakBlocker = 'no_model' | 'engine' | 'gateway' | null;

/** What stops a reply from being read aloud, most basic first: a voice model, its engine, then the
 *  gateway. Telling the user to start the gateway is useless while no voice model exists. */
export function speakBlocker(hasModel: boolean, engineReady: boolean, gatewayRunning: boolean): SpeakBlocker {
  if (!hasModel) return 'no_model';
  if (!engineReady) return 'engine';
  return gatewayRunning ? null : 'gateway';
}
