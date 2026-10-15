// Speech answer headers the Playground reads (kept free of imports so `node --test` loads it).

/** Why a speech answer is partial: `time` (the server's time limit stopped it), `frames` (the
 *  model made all the audio it may make for the text) or `tokens` (a transcription model wrote
 *  the longest text it may write for a part of the audio); `null` when it is whole. */
export type Truncation = 'time' | 'frames' | 'tokens' | null;

/** `x-harness-truncated` as a reason; an unknown value reads as `time`. */
export function truncationOf(header: string | null): Truncation {
  if (header === null) return null;
  return header === 'frames' || header === 'tokens' ? header : 'time';
}
