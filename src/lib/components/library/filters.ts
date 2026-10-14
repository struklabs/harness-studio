// My Models: tab membership, search, sorting and small formatters (pure; no Tauri calls).
import type { LibraryEntry, ModelDefinition, ParamInfo } from '#lib/types.js';

export type LibraryTab = 'all' | 'llm' | 'embedding' | 'vlm' | 'reranker' | 'asr' | 'tts' | 'image' | 'draft' | 'adapter' | 'mmproj';
export type LibrarySort = 'name' | 'size' | 'last_used' | 'added';

export const TABS: LibraryTab[] = ['all', 'llm', 'embedding', 'vlm', 'reranker', 'asr', 'tts', 'image', 'draft', 'adapter', 'mmproj'];

export function inTab(e: LibraryEntry, tab: LibraryTab): boolean {
  const kind = e.model.kind ?? 'unknown';
  switch (tab) {
    case 'all':
      return true;
    case 'llm':
      return kind === 'llm';
    case 'embedding':
      return kind === 'embedding';
    case 'vlm':
      return e.multimodal;
    case 'reranker':
      return kind === 'reranker';
    case 'asr':
      return kind === 'asr';
    case 'image':
      return kind === 'image';
    case 'tts':
      return kind === 'tts';
    case 'draft':
      return e.draft_for.length > 0;
    case 'adapter':
      return kind === 'adapter';
    case 'mmproj':
      return kind === 'mmproj';
  }
}

export function tabCounts(entries: LibraryEntry[]): Record<LibraryTab, number> {
  const out = Object.fromEntries(TABS.map((t) => [t, 0])) as Record<LibraryTab, number>;
  for (const e of entries) for (const tab of TABS) if (inTab(e, tab)) out[tab]++;
  return out;
}

/** Case-insensitive match on name, file, architecture, quant, path and the file's aliases. */
export function matchesQuery(e: LibraryEntry, query: string, aliases: string[]): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const hay = [e.model.name, e.model.filename, e.model.architecture ?? '', e.model.quantization, e.model.path, e.params.label ?? '', ...aliases]
    .join('\n')
    .toLowerCase();
  return q.split(/\s+/).every((w) => hay.includes(w));
}

export function sortEntries(list: LibraryEntry[], sort: LibrarySort): LibraryEntry[] {
  const byName = (a: LibraryEntry, b: LibraryEntry) => a.model.name.localeCompare(b.model.name);
  const desc = (f: (e: LibraryEntry) => number | null) => (a: LibraryEntry, b: LibraryEntry) =>
    (f(b) ?? -1) - (f(a) ?? -1) || byName(a, b);
  const cmp = {
    name: byName,
    size: desc((e) => e.model.size_bytes),
    last_used: desc((e) => e.last_used_at),
    added: desc((e) => e.added_at),
  }[sort];
  return [...list].sort(cmp);
}

/** "14.8B", "≈4.0B", "567M"; the size label when there is no count. */
export function formatParams(p: ParamInfo): string | null {
  if (p.count == null) return p.label ?? null;
  const n = p.count;
  const [v, unit] = n >= 1e12 ? [n / 1e12, 'T'] : n >= 1e9 ? [n / 1e9, 'B'] : n >= 1e6 ? [n / 1e6, 'M'] : [n / 1e3, 'K'];
  const text = `${v >= 100 ? v.toFixed(0) : v.toFixed(1)}${unit}`;
  return p.estimated ? `≈${text}` : text;
}

/** "3 gün önce" / "3 days ago". */
export function relativeTime(ms: number | null, loc: string, now = Date.now()): string | null {
  if (!ms) return null;
  const s = Math.round((ms - now) / 1000);
  const abs = Math.abs(s);
  const rtf = new Intl.RelativeTimeFormat(loc, { numeric: 'auto' });
  if (abs < 60) return rtf.format(s, 'second');
  if (abs < 3600) return rtf.format(Math.round(s / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(s / 3600), 'hour');
  if (abs < 86400 * 30) return rtf.format(Math.round(s / 86400), 'day');
  if (abs < 86400 * 365) return rtf.format(Math.round(s / (86400 * 30)), 'month');
  return rtf.format(Math.round(s / (86400 * 365)), 'year');
}

/** Comparable path (Windows paths are case-insensitive; either separator). */
function norm(p: string): string {
  return p.replace(/\\/g, '/').toLowerCase();
}

/** Definitions whose base is this file. */
export function definitionsOf(e: LibraryEntry, defs: ModelDefinition[]): ModelDefinition[] {
  const file = norm(e.model.path);
  return defs.filter((d) => d.base.kind === 'local' && norm(d.base.path) === file);
}

/** A free alias derived from the file name (same rule as the Dashboard's "new definition"). */
export function suggestAlias(name: string, defs: ModelDefinition[]): string {
  const taken = new Set(defs.map((d) => d.alias.toLowerCase()));
  const stem = name.replace(/[^A-Za-z0-9._:-]+/g, '-').replace(/^-+|-+$/g, '') || 'model';
  let alias = stem;
  for (let n = 2; taken.has(alias.toLowerCase()); n++) alias = `${stem}-${n}`;
  return alias;
}
