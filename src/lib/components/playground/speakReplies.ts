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
