/** Fold Turkish casing and accents so both "İ/ı" and unaccented queries match. */
export function normalizeSearch(text: string): string {
  return text.toLocaleLowerCase('tr').replace(/ı/g, 'i').normalize('NFD').replace(/[\u0300-\u036f]/g, '');
}

/** Match every partial word; rank label prefixes above substrings and keyword-only hits. */
export function paletteFilter(value: string, search: string, keywords: string[] = []): number {
  const words = normalizeSearch(search).split(/\s+/).filter(Boolean);
  if (words.length === 0) return 1;
  const label = normalizeSearch(value);
  const all = `${label} ${normalizeSearch(keywords.join(' '))}`;
  if (!words.every((word) => all.includes(word))) return 0;
  const startsAll = (text: string) => {
    const starts = text.split(/[^\p{L}\p{N}]+/u);
    return words.every((word) => starts.some((part) => part.startsWith(word)));
  };
  if (!words.every((word) => label.includes(word))) {
    // One field's own text starting every query word ("oturum a" → "Oturum açılınca başlat")
    // ranks above words scattered over a page's fields.
    const fields = keywords.map(normalizeSearch);
    if (fields.some((k) => startsInOrder(k, words))) return 0.6;
    return fields.some(startsAll) ? 0.5 : 0.3;
  }
  return startsAll(label) ? 1 : 0.7;
}

/** Whether consecutive words of `text` start with the query words in their order. */
function startsInOrder(text: string, words: string[]): boolean {
  const parts = text.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  return parts.some((_, i) => words.every((word, j) => parts[i + j]?.startsWith(word)));
}

/** Pick one field; section-name matches keep the existing section-only navigation. */
export function bestSettingsField<T>(
  query: string,
  sectionLabels: string[],
  fields: { value: T; texts: string[] }[],
): T | null {
  const words = normalizeSearch(query).split(/\s+/).filter(Boolean);
  if (!words.length || sectionLabels.some((label) => paletteFilter(label, query) > 0)) return null;
  let best: T | null = null;
  let bestScore = 0;
  for (const field of fields) {
    const score = Math.max(paletteFilter('', query, field.texts), ...field.texts.map((text) => {
      const normalized = normalizeSearch(text);
      if (startsInOrder(normalized, words)) return 3;
      const match = paletteFilter(text, query);
      return match === 1 ? 2 : match;
    }));
    if (score > bestScore) {
      best = field.value;
      bestScore = score;
    }
  }
  return best;
}
