const turkishWords = new Set('bir bu şu o ve veya ile için ama gibi daha çok nasıl neden nedir ne nerede hangi kaç mı mi mu mü de da ki değil var yok olan olarak kadar sonra önce çünkü eğer bana ben sen biz siz benim senin bunu şunu yaz anlat açıkla lütfen merhaba'.split(' '));
const englishWords = new Set('a an the this that these those and or with for but as how why what which where when is are was were be to of in on at from my your you we it can do does please only write explain tell hello'.split(' '));
const englishContentWords = new Set('sorting methods sort list lists travel tips history computing number numbers error errors programming guide introduction learning search data file files module modules setup installation troubleshooting'.split(' '));
const otherLanguageWords = new Set('el la los las un una y para por que como cómo qué es en del der die das ein eine und für mit von ist wie was des les le une et pour dans est comment explique'.split(' '));
const greetings = new Set(['merhaba', 'selam', 'selamlar', 'günaydın', 'iyi günler', 'iyi akşamlar', 'hi', 'hello', 'hey', 'good morning', 'good evening']);
// Turkish case suffixes after an apostrophe (London'da, Python'un); English uses 's, 't, 'll, 're, 've, 'd, 'm.
const turkishSuffix = /\p{L}['’](?:[dt][ae]n?|[dt][ae]ki|n?[ıiuü]n|[yn]?[aeıiuü]|l[ae]r?)(?!\p{L})/iu;

function hasTurkishLetters(text: string, hasTurkishWord: boolean): boolean {
  return /[ıĞğŞş]/u.test(text) || (hasTurkishWord && /[çÇöÖüÜ]/u.test(text));
}

/** Keep the UTF-16 limit without cutting a grapheme (or a code point on older runtimes). */
function truncateTitle(text: string, limit: number): string {
  const segments = typeof Intl.Segmenter === 'function'
    ? Array.from(new Intl.Segmenter(undefined, { granularity: 'grapheme' }).segment(text), part => part.segment)
    : Array.from(text);
  let result = '';
  for (const segment of segments) {
    if (result.length + segment.length > limit) break;
    result += segment;
  }
  return result;
}

function plainTitleText(text: string): string {
  return text.replace(/```[\s\S]*?(?:```|$)|~~~[\s\S]*?(?:~~~|$)/g, ' ')
    .replace(/`[^`]*`/g, ' ')
    .replace(/!\[[^\]]*\]\([^)]*\)/g, ' ')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/(?:https?:\/\/|www\.)\S+/gi, ' ')
    .replace(/<[^>]*>/g, ' ')
    .replace(/[*_#~>|]/g, ' ')
    .replace(/\s+/g, ' ').trim();
}

/** Ambiguous text inherits the UI language; an empty default exposes uncertainty for validation. */
export function detectTitleLanguage(text: string, uiLanguage: string): string {
  const plain = plainTitleText(text);
  const words = new Set(plain.toLocaleLowerCase('tr').match(/\p{L}+/gu) ?? []);
  const trWords = [...words].filter(word => turkishWords.has(word)).length;
  const tr = trWords + (hasTurkishLetters(plain, trWords > 0) ? 2 : 0) +
    (turkishSuffix.test(plain) ? 1 : 0);
  const enWords = new Set(plain.toLowerCase().match(/\p{L}+/gu) ?? []);
  const en = [...enWords].filter(word => englishWords.has(word)).length;
  const other = [...enWords].filter(word => otherLanguageWords.has(word)).length;
  if (words.size >= 3 && other > tr && other > en && !hasTurkishLetters(plain, trWords > 0) && !turkishSuffix.test(plain)) return 'other';
  if (tr >= 2 && tr > en) return 'tr';
  if (en >= 2 && en > tr) return 'en';
  // A sentence with one function word and no sign of the other language is still clear enough.
  if (words.size >= 3 && tr === 1 && en === 0) return 'tr';
  if (words.size >= 3 && en === 1 && tr === 0) return 'en';
  // Script evidence is useful even for short messages; unknown Latin prose needs more context.
  if (tr === 0 && en === 0 && (/(?=\p{L})\P{Script=Latin}/u.test(plain) ||
      (words.size >= 3 && !/[{}=;]|=>|\b(?:const|let|var|function|return|import|class)\b/u.test(plain)))) return 'other';
  return uiLanguage.toLowerCase().split(/[-_]/)[0];
}

export function buildTitleMessages(firstUser: string, firstAnswer: string, uiLanguage: string) {
  const language = detectTitleLanguage(firstUser, uiLanguage);
  let instruction = language === 'tr'
    ? 'Aşağıdaki konuşma için Türkçe kısa bir başlık yaz (en fazla 6 kelime ve 60 karakter). ' +
      'Yalnızca başlığı yaz; tırnak işareti veya sonda noktalama kullanma.'
    : 'Write a short title (at most 6 words and 60 characters) for the conversation below. ' +
      'Write the title in English. Reply with the title only: no quotes, no trailing punctuation.';
  if (language === 'other') {
    instruction = 'Write a short title (at most 6 words and 60 characters) for the conversation below. ' +
      "Write the title in the same language as the user's message. Reply with the title only: no quotes, no trailing punctuation.";
  } else if (language !== 'tr' && language !== 'en') {
    let name = language;
    try {
      name = new Intl.DisplayNames(['en'], { type: 'language' }).of(language) ?? language;
    } catch {
      // Preserve unknown UI language identifiers.
    }
    instruction = 'Write a short title (at most 6 words and 60 characters) for the conversation below. ' +
      `Write the title in ${name}. Reply with the title only: no quotes, no trailing punctuation.`;
  }
  return [
    { role: 'system', content: instruction },
    { role: 'user', content: `User: ${firstUser.slice(0, 2000)}\n\nAssistant: ${firstAnswer.slice(0, 1000)}` },
  ];
}

/** Provisional title from the first user message, shared by creation and rejected model replies. */
export function fallbackTitle(text: string): string {
  let plain = plainTitleText(text);
  const withoutNumber = plain.replace(/^\d+[.)]\s*(?:(?:tur|round|turn)\s*:\s*)?/i, '');
  if (/\p{L}{2,}/u.test(withoutNumber)) plain = withoutNumber;
  // A one-word label such as "Error:" belongs to its description.
  const colon = plain.indexOf(':');
  if (colon >= 0 && plain.slice(0, colon).trim().split(/\s+/).length > 1) plain = plain.slice(0, colon);
  const clauses = plain.split(/[,!?;。！？]|(?<!\d)\.(?:\s|$)|\s+[—–]\s+/u).map((c) => c.trim()).filter(Boolean);
  // "Merhaba, kısa bir şiir yaz" is about the poem, not the greeting.
  if (clauses.length > 1 && greetings.has(clauses[0].toLocaleLowerCase('tr')) &&
      /^[^,!]+[,!]/u.test(plain) && clauses[1].split(/\s+/).length >= 3 && !/^world\b/i.test(clauses[1])) {
    clauses.shift();
    clauses[0] = clauses[0].charAt(0).toLocaleUpperCase('tr') + clauses[0].slice(1);
  }
  const clause = clauses[0] ?? '';
  return truncateTitle(clause.split(/\s+/).slice(0, 6).join(' '), 60)
    .replace(/(?:[.,;:!?…。．，；：！？\s]|[0-9]\ufe0f?\u20e3|\p{Extended_Pictographic}|\p{Regional_Indicator}|[\u200d\ufe0f\u20e3]|\p{Emoji_Modifier})+$/gu, '');
}

/**
 * Titles are a few words, too short for the detector alone: a Turkish chat rejects a title
 * without Turkish evidence that has an English function word or multiple common content words.
 * English chats reject strong Turkish letters, suffixes or multiple Turkish words.
 */
function titleInOtherLanguage(title: string, language: string): boolean {
  if (!title) return false;
  const lower = title.toLocaleLowerCase('tr');
  const words = lower.match(/\p{L}+/gu) ?? [];
  const trWords = words.filter(w => turkishWords.has(w)).length;
  const turkishLetters = hasTurkishLetters(title, trWords > 0);
  const enWords = title.toLowerCase().match(/\p{L}+/gu) ?? [];
  if (language === 'tr') return !turkishLetters && !turkishSuffix.test(title) && trWords === 0 &&
    (enWords.some(w => englishWords.has(w)) || new Set(enWords.filter(w => englishContentWords.has(w))).size >= 2);
  if (language === 'en') return turkishLetters || turkishSuffix.test(title) || words.filter((w) => turkishWords.has(w)).length >= 2;
  return false;
}

export function resolveGeneratedTitle(raw: string, firstUser: string, uiLanguage: string): { title: string; source: 'auto' | 'fallback' } {
  const title = cleanGeneratedTitle(raw);
  const language = detectTitleLanguage(firstUser, uiLanguage);
  const mismatch = titleInOtherLanguage(title, language);
  return title && !mismatch ? { title, source: 'auto' } : { title: fallbackTitle(firstUser), source: 'fallback' };
}

/** Some chat templates accept only a user role. Both attempts share the caller's abort signal. */
export async function generateTitleWithRetry(
  firstUser: string, firstAnswer: string, uiLanguage: string,
  request: (messages: { role: string; content: string }[]) => Promise<Response>,
): Promise<{ title: string; source: 'auto' | 'fallback' }> {
  const messages = buildTitleMessages(firstUser, firstAnswer, uiLanguage);
  const attempts = [messages, [{ role: 'user', content: `${messages[0].content}\n\n${messages[1].content}` }]];
  for (const attempt of attempts) {
    try {
      const response = await request(attempt);
      if (!response.ok) continue;
      const data = await response.json();
      return resolveGeneratedTitle(String(data?.choices?.[0]?.message?.content ?? ''), firstUser, uiLanguage);
    } catch {
      // Transport failures and rejected templates still leave a usable fallback title.
    }
  }
  return { title: fallbackTitle(firstUser), source: 'fallback' };
}

/** Reject answer bodies rather than turning their first line into a title. */
export function cleanGeneratedTitle(raw: string): string {
  const withoutThinking = raw.replace(/<think>[\s\S]*?<\/think>/gi, '');
  if (/<\/?think>/i.test(withoutThinking)) return '';
  let text = withoutThinking.trim().replace(/^(title|başlık)\s*:\s*/i, '');
  if (!text || text.split(/\n/).filter(line => line.trim()).length > 1) return '';
  if (/\d{1,2}:\d{2}/.test(text) || /(?<!(?<![\p{L}\p{N}])(?:vs|etc|vb|vd|örn))[.!?]\s+\S/iu.test(text)) return '';
  text = text.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/[*_`#~]/g, '').replace(/^["'“”‘’\s]+|["'“”‘’\s]+$/g, '').trim();
  if (!text || text.length > 60 || text.split(/\s+/).length > 6 || /[.!?]$/.test(text)) return '';
  // A bare answer such as "14" is not a title.
  if (!/\p{L}{2,}/u.test(text)) return '';
  return text;
}
