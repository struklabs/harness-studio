const turkishWords = new Set('bir bu şu o ve veya ile için ama gibi daha çok nasıl neden nedir ne nerede hangi kaç mı mi mu mü de da ki değil var yok olan olarak kadar sonra önce çünkü eğer bana ben sen biz siz benim senin bunu şunu yaz anlat açıkla lütfen merhaba'.split(' '));
const englishWords = new Set('a an the this that these those and or with for but as how why what which where when is are was were be to of in on at from my your you we it can do does please only write explain tell hello'.split(' '));
const greetings = new Set(['merhaba', 'selam', 'selamlar', 'günaydın', 'iyi günler', 'iyi akşamlar', 'hi', 'hello', 'hey', 'good morning', 'good evening']);
// Turkish case suffixes after an apostrophe (London'da, Python'un); English uses 's, 't, 'll, 're, 've, 'd, 'm.
const turkishSuffix = /\p{L}['’](?:[dt][ae]n?|[dt][ae]ki|n?[ıiuü]n|[yn]?[aeıiuü]|l[ae]r?)(?!\p{L})/iu;

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
  const tr = [...words].filter(word => turkishWords.has(word)).length + (/[çğıöşüİ]/i.test(plain) ? 2 : 0) +
    (turkishSuffix.test(plain) ? 1 : 0);
  const enWords = new Set(plain.toLowerCase().match(/\p{L}+/gu) ?? []);
  const en = [...enWords].filter(word => englishWords.has(word)).length;
  if (tr >= 2 && tr > en) return 'tr';
  if (en >= 2 && en > tr) return 'en';
  // A sentence with one function word and no sign of the other language is still clear enough.
  if (words.size >= 3 && tr === 1 && en === 0) return 'tr';
  if (words.size >= 3 && en === 1 && tr === 0) return 'en';
  return uiLanguage.toLowerCase().split(/[-_]/)[0];
}

export function buildTitleMessages(firstUser: string, firstAnswer: string, uiLanguage: string) {
  const language = detectTitleLanguage(firstUser, uiLanguage);
  let instruction = language === 'tr'
    ? 'Aşağıdaki konuşma için Türkçe kısa bir başlık yaz (en fazla 6 kelime ve 60 karakter). ' +
      'Yalnızca başlığı yaz; tırnak işareti veya sonda noktalama kullanma.'
    : 'Write a short title (at most 6 words and 60 characters) for the conversation below. ' +
      'Write the title in English. Reply with the title only: no quotes, no trailing punctuation.';
  if (language !== 'tr' && language !== 'en') {
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
  const clauses = plain.split(/[,!?;:。！？]|(?<!\d)\.(?:\s|$)|\s+[—–]\s+/u).map((c) => c.trim()).filter(Boolean);
  // "Merhaba, kısa bir şiir yaz" is about the poem, not the greeting.
  if (clauses.length > 1 && greetings.has(clauses[0].toLocaleLowerCase('tr'))) {
    clauses.shift();
    clauses[0] = clauses[0].charAt(0).toLocaleUpperCase('tr') + clauses[0].slice(1);
  }
  const clause = clauses[0] ?? '';
  return clause.split(/\s+/).slice(0, 6).join(' ').slice(0, 60)
    .replace(/[\p{P}\p{S}\s]+$/gu, '');
}

/**
 * Titles are a few words, too short for the detector alone: a Turkish chat rejects a title
 * without Turkish letters that has any English function word, an English chat one with Turkish
 * letters, a Turkish suffix after an apostrophe or Turkish words.
 */
function titleInOtherLanguage(title: string, language: string): boolean {
  if (!title) return false;
  const lower = title.toLocaleLowerCase('tr');
  const words = lower.match(/\p{L}+/gu) ?? [];
  const turkishLetters = /[çğıöşü]/.test(lower);
  if (language === 'tr') return !turkishLetters && words.some((w) => englishWords.has(w)) && !words.some((w) => turkishWords.has(w));
  if (language === 'en') return turkishLetters || turkishSuffix.test(title) || words.filter((w) => turkishWords.has(w)).length >= 2;
  return false;
}

export function resolveGeneratedTitle(raw: string, firstUser: string, uiLanguage: string): { title: string; source: 'auto' | 'fallback' } {
  const title = cleanGeneratedTitle(raw);
  const language = detectTitleLanguage(firstUser, uiLanguage);
  const mismatch = titleInOtherLanguage(title, language);
  return title && !mismatch ? { title, source: 'auto' } : { title: fallbackTitle(firstUser), source: 'fallback' };
}

/** Reject answer bodies rather than turning their first line into a title. */
export function cleanGeneratedTitle(raw: string): string {
  const withoutThinking = raw.replace(/<think>[\s\S]*?<\/think>/gi, '');
  if (/<\/?think>/i.test(withoutThinking)) return '';
  let text = withoutThinking.trim().replace(/^(title|başlık)\s*:\s*/i, '');
  if (!text || text.split(/\n/).filter(line => line.trim()).length > 1) return '';
  if (/\d{1,2}:\d{2}/.test(text) || /(?<!\b(?:vs|etc|vb|vd|örn))[.!?]\s+\S/i.test(text)) return '';
  text = text.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/[*_`#~]/g, '').replace(/^["'“”‘’\s]+|["'“”‘’\s]+$/g, '').trim();
  if (!text || text.length > 60 || text.split(/\s+/).length > 6 || /[.!?]$/.test(text)) return '';
  // A bare answer such as "14" is not a title.
  if (!/\p{L}{2,}/u.test(text)) return '';
  return text;
}
