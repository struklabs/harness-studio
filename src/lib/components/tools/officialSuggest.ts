import type { RegistryEntry, RegistryKind } from './registryCatalog.ts';

export type SuggestKind = RegistryKind | 'connector';

const STOP = new Set(['the', 'and', 'for', 'with', 'that', 'this', 'from', 'into', 'tool', 'skill', 'create', 'make', 'build', 'bir', 'ile', 'için', 'olan', 'ben', 'bana', 'yap', 'oluştur']);

/** Lower-case word tokens of three or more letters, without filler words. */
export function tokens(text: string): string[] {
  return [...new Set(text.toLocaleLowerCase().split(/[^\p{L}\p{N}]+/u).filter(w => w.length >= 3 && !STOP.has(w)))];
}

/** Official catalog entries of `kind` that already do what `text` asks for, best first. */
export function suggestOfficial(entries: RegistryEntry[], kind: RegistryKind, text: string, limit = 3): RegistryEntry[] {
  const wanted = tokens(text);
  if (!wanted.length) return [];
  return entries
    .filter(e => e.type === kind)
    .map(entry => {
      const key = new Set(tokens(`${entry.name} ${entry.id} ${entry.tags.join(' ')}`));
      const body = new Set(tokens(`${entry.description.en} ${entry.description.tr}`));
      const strong = wanted.filter(w => key.has(w)).length;
      const weak = wanted.filter(w => body.has(w)).length;
      return { entry, strong, score: strong * 3 + weak };
    })
    .filter(m => m.strong > 0 || m.score >= 2)
    .toSorted((a, b) => b.score - a.score || a.entry.name.localeCompare(b.entry.name))
    .slice(0, limit)
    .map(m => m.entry);
}

/** Words that point at one of Harness's own connectors. */
const CONNECTOR_WORDS: Record<string, string[]> = {
  gmail: ['gmail', 'email', 'mail', 'inbox', 'eposta', 'e-posta'],
  drive: ['drive', 'docs', 'dosya', 'dosyalar', 'documents'],
  calendar: ['calendar', 'takvim', 'event', 'events', 'meeting', 'toplantı', 'randevu'],
  excel: ['excel', 'spreadsheet', 'xlsx', 'workbook', 'worksheet', 'tablo', 'sheets'],
};

/** Ids of the built-in connectors `text` is about. */
export function suggestConnectors(text: string): string[] {
  const wanted = tokens(text);
  // Turkish suffixes ('takvime', 'tablosunu') keep the stem, so longer keywords match as prefixes.
  const hit = (w: string) => wanted.some(token => token === w || (w.length >= 5 && token.startsWith(w)));
  return Object.entries(CONNECTOR_WORDS).filter(([, words]) => words.some(hit)).map(([id]) => id);
}
