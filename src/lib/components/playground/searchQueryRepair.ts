const words = /[\p{L}\p{M}\p{N}]+/gu;

function normalize(word: string): string {
  return word.toLowerCase().normalize('NFD').replace(/\p{M}/gu, '').replace(/ı/g, 'i');
}

function distanceOne(a: string, b: string): boolean {
  if (Math.abs(a.length - b.length) > 1) return false;
  let i = 0, j = 0, edits = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) { i++; j++; continue; }
    if (++edits > 1) return false;
    if (a.length >= b.length) i++;
    if (b.length >= a.length) j++;
  }
  return edits + Number(i < a.length || j < b.length) === 1;
}

/** Playground only: restore an unambiguous near-miss to the latest user's spelling. */
export function repairSearchQuery(query: string, userMessage: string): string {
  const originals = new Map<string, string>();
  for (const word of userMessage.match(words) ?? []) {
    const key = normalize(word);
    if (key.length >= 4 && !originals.has(key)) originals.set(key, word);
  }
  return query.replace(words, (word) => {
    const key = normalize(word);
    // A three-letter truncation of a four-letter name (Ene -> enes) is allowed;
    // arbitrary edits of short words are not.
    if (key.length < 3) return word;
    if (originals.has(key)) return originals.get(key)!;
    const matches = [...originals].filter(([candidate]) => {
      if (key.length === 3 && candidate.length !== 4) return false;
      return candidate.startsWith(key) || candidate.endsWith(key) ||
        key.startsWith(candidate) || key.endsWith(candidate) ||
        (key.length >= 4 && distanceOne(key, candidate));
    });
    return matches.length === 1 ? matches[0][1] : word;
  });
}
