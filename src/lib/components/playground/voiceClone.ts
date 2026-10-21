// Voice cloning in the Speech tab: what the gateway accepts as a reference recording
// (`voice_ref`, see crates/harness-gateway/src/voice_clone.rs) and when a clone request may go.
import type { TtsModelInfo } from './tts';

/** Largest sample the gateway takes (as sent). */
export const MAX_SAMPLE_BYTES = 10 * 1024 * 1024;
/** Longest sample in seconds; a recording stops there. */
export const MAX_SAMPLE_SECONDS = 30;
/** Sample rate microphone samples are made at (what Pocket TTS works at). */
export const SAMPLE_RATE = 24000;

export interface VoiceRef {
  /** Base64 of the sample file. */
  data: string;
  /** Transcript of the sample (optional). */
  text: string;
}

/** The selected model can clone a voice. */
export function canClone(models: TtsModelInfo[], name: string): boolean {
  return models.find((m) => m.name === name)?.voice_cloning === true;
}

/** The selected model runs on the experimental audio.cpp engine while that engine is turned off:
 *  the clone section explains how to enable it, and Run waits. */
export function engineDisabled(models: TtsModelInfo[], name: string, audioCppEnabled: boolean): boolean {
  return models.find((m) => m.name === name)?.engine === 'audio.cpp' && !audioCppEnabled;
}

/** A clone request to the selected model must carry the sample's transcript. */
export function cloneNeedsText(models: TtsModelInfo[], name: string): boolean {
  return models.find((m) => m.name === name)?.voice_clone_needs_text === true;
}

/** What still stops a clone request: the sample, the size limit, the transcript (for a model
 *  that needs it), then the consent box. */
export function cloneBlocker(
  file: File | null,
  consent: boolean,
  needsText = false,
  text = '',
): 'sample' | 'size' | 'text' | 'consent' | null {
  if (!file) return 'sample';
  if (file.size > MAX_SAMPLE_BYTES) return 'size';
  if (needsText && !text.trim()) return 'text';
  if (!consent) return 'consent';
  return null;
}

/** The reference the request carries; the sample is read into memory for this request only. */
export async function voiceRefOf(file: File, text: string): Promise<VoiceRef> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return { data: btoa(binary), text: text.trim() };
}

/** The JSON fields of a clone request. The consent is always true here: the request is only built after the box was ticked. */
export function cloneFields(ref: VoiceRef): Record<string, unknown> {
  const fields: Record<string, unknown> = { voice_ref: { type: 'base64', data: ref.data }, voice_consent: true };
  if (ref.text) fields.voice_ref_text = ref.text;
  return fields;
}
