// Text preparation for reading a chat message aloud (pure; no imports so a node test can load it).

/** The gateway refuses a /v1/audio/speech input longer than this many characters. */
export const MAX_SPEECH_CHARS = 4096;

/**
 * Markdown to what a person would say: code blocks are dropped, links keep their text, emphasis,
 * heading and list markers and raw HTML tags disappear. The model's own reasoning is not part of
 * the message content, so it is never read.
 */
export function plainSpeechText(markdown: string): string {
  return markdown
    .replace(/```[\s\S]*?(```|$)/g, ' ')
    .replace(/~~~[\s\S]*?(~~~|$)/g, ' ')
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/<[^>\n]+>/g, ' ')
    .replace(/`([^`]*)`/g, '$1')
    .replace(/^\s{0,3}#{1,6}\s+/gm, '')
    .replace(/^\s*>\s?/gm, '')
    .replace(/^\s*([-*+]|\d+[.)])\s+/gm, '')
    .replace(/^\s*(-{3,}|\*{3,}|_{3,})\s*$/gm, ' ')
    .replace(/(\*\*|__)(.+?)\1/g, '$2')
    .replace(/(^|[^\w*])[*_](?=\S)([^*_\n]*?\S)[*_](?=$|[^\w*])/g, '$1$2')
    .replace(/\|/g, ' ')
    .replace(/[ \t]+/g, ' ')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

/**
 * Splits text into pieces of at most `max` characters, at paragraph, sentence or word
 * boundaries (in that order of preference), so every piece is a valid /v1/audio/speech input.
 */
export function chunkSpeechText(text: string, max: number = MAX_SPEECH_CHARS): string[] {
  const limit = Math.max(1, max);
  const out: string[] = [];
  let rest = text.trim();
  while (rest.length > limit) {
    const window = rest.slice(0, limit);
    let cut = Math.max(window.lastIndexOf('\n\n'), -1);
    if (cut < limit * 0.4) {
      const sentence = [...window.matchAll(/[.!?…。！？]["')\]]?\s/g)].map((m) => m.index + m[0].length);
      cut = sentence.length ? sentence[sentence.length - 1] : -1;
    }
    if (cut < limit * 0.4) cut = window.lastIndexOf(' ');
    if (cut < 1) cut = limit;
    out.push(rest.slice(0, cut).trim());
    rest = rest.slice(cut).trim();
  }
  if (rest) out.push(rest);
  return out.filter((p) => p.length > 0);
}
