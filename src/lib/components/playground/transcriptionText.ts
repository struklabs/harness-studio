/** Words whisper uses inside non-speech annotations (`[BLANK_AUDIO]`, `[Music]`, `(wind blowing)`,
 * `[Motor]`, `[Ambient]`; Turkish `[Müzik]`). A bracketed tag is an annotation when one of its
 * words is one of these (with a plural or -ing ending). */
const SOUND_WORDS =
  /^(?:blank|audio|music|silence|silent|inaudible|applause|laugh|laughter|noise|motor|ambient|engine|wind|static|buzz|hum|beep|typing|footstep|click|breath|breathe|cough|fan|sigh|sound|whir|rumble|tapping|chirp|ring|bell|sniff|exhale|inhale|müzik|sessizlik|gürültü|alkış|kahkaha)(?:s|es|ing|ming|ring|ling)?$/i;

function isAnnotation(inner: string): boolean {
  return /^\s*no[_ ]speech\s*$/i.test(inner) || inner.split(/[\s_]+/).some((w) => SOUND_WORDS.test(w));
}

/** Whisper non-speech annotations alone are not dictated words.
 * Keep mixed speech and annotations, and ordinary bracketed prose, intact. */
export function normalizeTranscriptionText(text: string): string {
  const rest = text
    .replace(/\[([^\[\]]*)\]|\(([^()]*)\)|\*([^*]*)\*/g, (tag, square?: string, round?: string, star?: string) =>
      isAnnotation(square ?? round ?? star ?? '') ? '' : tag)
    .replace(/[♪♫*\s]+/g, '');
  return rest ? text : '';
}
