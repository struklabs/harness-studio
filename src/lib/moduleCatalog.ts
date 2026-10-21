// Pure catalog logic of the Modules page: normalizing manifests, fuzzy search, filters,
// sorting, facets and windowing. No framework imports so Node can test it directly.
import type { ModuleView, LocaleKeys } from './modules.ts';

export type Origin = 'official' | 'community';
export type StatusFilter = 'all' | 'installed' | 'available' | 'updates';
export type EnabledFilter = 'all' | 'enabled' | 'disabled';
export type OriginFilter = 'all' | Origin;
export type SortKey = 'recommended' | 'name' | 'updated' | 'popular';
export type Translate = (keys: LocaleKeys) => string;

export interface CatalogFilters {
  query: string;
  status: StatusFilter;
  enabled: EnabledFilter;
  origin: OriginFilter;
  category: string | null;
  capability: string | null;
  /** Only modules that run on this platform id (for example `windows-x86_64`). */
  platform: string | null;
  /** Hide entries this machine cannot install. */
  compatibleOnly: boolean;
}
export const DEFAULT_FILTERS: CatalogFilters = {
  query: '', status: 'all', enabled: 'all', origin: 'all', category: null, capability: null, platform: null, compatibleOnly: false,
};

export interface CatalogEntry {
  id: string;
  view: ModuleView;
  name: string;
  description: string;
  category: string;
  tags: string[];
  capabilities: string[];
  author: string | null;
  origin: Origin;
  installed: boolean;
  enabled: boolean;
  updateAvailable: boolean;
  compatible: boolean;
  /** Lowercased, accent-folded text per search field, built once per language. */
  fields: { text: string; weight: number }[];
}

export const OTHER_CATEGORY = 'other';

/** Lowercase and fold accents; Turkish dotted and dotless i collapse to `i`. */
export function fold(text: string): string {
  return text
    .replace(/İ/g, 'i').replace(/ı/g, 'i')
    .normalize('NFD').replace(/[̀-ͯ]/g, '')
    .toLowerCase();
}

export function toEntry(view: ModuleView, translate: Translate): CatalogEntry {
  const m = view.manifest;
  const c = view.catalog ?? {};
  const status = view.state.status;
  const installed = status === 'installed' || status === 'disabled';
  const name = translate(m.name);
  const description = translate(m.description);
  const category = c.category || OTHER_CATEGORY;
  const tags = c.tags ?? [];
  const capabilities = c.capabilities ?? [];
  const author = c.author ?? null;
  return {
    id: m.id, view, name, description, category, tags, capabilities, author,
    origin: c.community ? 'community' : 'official',
    installed, enabled: status === 'installed', updateAvailable: installed && view.update_available,
    compatible: !view.incompatibility && !m.planned,
    fields: [
      { text: fold(name), weight: 4 },
      { text: fold(tags.join(' ')), weight: 3 },
      { text: fold(`${category} ${capabilities.join(' ')}`), weight: 2 },
      { text: fold(author ?? ''), weight: 2 },
      { text: fold(m.functions.map(translate).join(' ')), weight: 1.5 },
      { text: fold(description), weight: 1 },
    ],
  };
}

function editDistanceAtMostOne(a: string, b: string): boolean {
  if (Math.abs(a.length - b.length) > 1) return false;
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  if (i === a.length || i === b.length) return true;
  if (a.length === b.length) {
    if (a.slice(i + 1) === b.slice(i + 1)) return true;
    return a[i] === b[i + 1] && a[i + 1] === b[i] && a.slice(i + 2) === b.slice(i + 2);
  }
  return a.length > b.length ? a.slice(i + 1) === b.slice(i) : b.slice(i + 1) === a.slice(i);
}

/** How well one query token matches one folded field, 0 when it does not. */
function tokenScore(token: string, text: string): number {
  if (!text) return 0;
  const at = text.indexOf(token);
  if (at >= 0) return at === 0 ? 100 : text[at - 1] === ' ' ? 90 : 60;
  // One typo (insertion, deletion, substitution, swap) against a whole word.
  if (token.length >= 4 && text.split(' ').some(w => editDistanceAtMostOne(token, w))) return 35;
  if (token.length >= 3) {
    // Subsequence: every letter in order; tighter spans score higher.
    let from = 0;
    let first = -1;
    let last = -1;
    for (const ch of token) {
      const next = text.indexOf(ch, from);
      if (next < 0) return 0;
      if (first < 0) first = next;
      last = next;
      from = next + 1;
    }
    const span = last - first + 1;
    return span <= token.length * 2 ? 20 : span <= token.length * 4 ? 10 : 0;
  }
  return 0;
}

/** Fuzzy score of an entry for a query; 0 means no match. Every token must match somewhere. */
export function searchScore(entry: Pick<CatalogEntry, 'fields'>, query: string): number {
  const tokens = fold(query).split(/\s+/).filter(Boolean);
  if (!tokens.length) return 1;
  let total = 0;
  for (const token of tokens) {
    let best = 0;
    for (const field of entry.fields) best = Math.max(best, tokenScore(token, field.text) * field.weight);
    if (best === 0) return 0;
    total += best;
  }
  return total;
}

export function matchesFilters(entry: CatalogEntry, f: CatalogFilters): boolean {
  if (f.status === 'installed' && !entry.installed) return false;
  if (f.status === 'available' && entry.installed) return false;
  if (f.status === 'updates' && !entry.updateAvailable) return false;
  if (f.enabled === 'enabled' && !(entry.installed && entry.enabled)) return false;
  if (f.enabled === 'disabled' && !(entry.installed && !entry.enabled)) return false;
  if (f.origin !== 'all' && entry.origin !== f.origin) return false;
  if (f.category && entry.category !== f.category) return false;
  if (f.capability && !entry.capabilities.includes(f.capability)) return false;
  if (f.platform && !entry.view.manifest.platforms.includes(f.platform)) return false;
  if (f.compatibleOnly && !entry.compatible) return false;
  return true;
}

const byName = (a: CatalogEntry, b: CatalogEntry) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) || a.id.localeCompare(b.id);
const updated = (e: CatalogEntry) => e.view.catalog?.updated_at ?? '';
const popularity = (e: CatalogEntry) => e.view.catalog?.popularity ?? 0;

export function compareEntries(sort: SortKey): (a: CatalogEntry, b: CatalogEntry) => number {
  switch (sort) {
    case 'name': return byName;
    case 'updated': return (a, b) => updated(b).localeCompare(updated(a)) || byName(a, b);
    case 'popular': return (a, b) => popularity(b) - popularity(a) || byName(a, b);
    default: return (a, b) => {
      // Recommended: updates first, then featured, then what is usable now, then popularity.
      const rank = (e: CatalogEntry) => (e.updateAvailable ? 4 : 0) + (e.view.catalog?.featured ? 2 : 0) + (e.compatible ? 1 : 0);
      return rank(b) - rank(a) || popularity(b) - popularity(a) || byName(a, b);
    };
  }
}

/** Filter, then order: with a query, by relevance (the chosen sort breaks ties). */
export function filterAndSort(entries: readonly CatalogEntry[], filters: CatalogFilters, sort: SortKey): CatalogEntry[] {
  const compare = compareEntries(sort);
  const scored: { entry: CatalogEntry; score: number }[] = [];
  for (const entry of entries) {
    if (!matchesFilters(entry, filters)) continue;
    const score = searchScore(entry, filters.query);
    if (score > 0) scored.push({ entry, score });
  }
  const searching = filters.query.trim().length > 0;
  scored.sort((a, b) => (searching && b.score !== a.score ? b.score - a.score : compare(a.entry, b.entry)));
  return scored.map(s => s.entry);
}

export interface Facets {
  categories: Map<string, number>;
  capabilities: Map<string, number>;
  platforms: Map<string, number>;
  status: Record<StatusFilter, number>;
  origin: Record<OriginFilter, number>;
}
/** Counts for the sidebar. Each facet ignores its own filter so the counts show what a click would give. */
export function facets(entries: readonly CatalogEntry[], filters: CatalogFilters): Facets {
  const out: Facets = {
    categories: new Map(), capabilities: new Map(), platforms: new Map(),
    status: { all: 0, installed: 0, available: 0, updates: 0 },
    origin: { all: 0, official: 0, community: 0 },
  };
  const bump = (map: Map<string, number>, key: string) => map.set(key, (map.get(key) ?? 0) + 1);
  for (const entry of entries) {
    if (searchScore(entry, filters.query) === 0) continue;
    if (matchesFilters(entry, { ...filters, category: null })) bump(out.categories, entry.category);
    if (matchesFilters(entry, { ...filters, capability: null })) for (const c of entry.capabilities) bump(out.capabilities, c);
    if (matchesFilters(entry, { ...filters, platform: null })) for (const p of entry.view.manifest.platforms) bump(out.platforms, p);
    if (matchesFilters(entry, { ...filters, status: 'all' })) {
      out.status.all++;
      if (entry.installed) out.status.installed++; else out.status.available++;
      if (entry.updateAvailable) out.status.updates++;
    }
    if (matchesFilters(entry, { ...filters, origin: 'all' })) {
      out.origin.all++;
      out.origin[entry.origin]++;
    }
  }
  return out;
}

export function activeFilterCount(f: CatalogFilters): number {
  return (f.status !== 'all' ? 1 : 0) + (f.enabled !== 'all' ? 1 : 0) + (f.origin !== 'all' ? 1 : 0)
    + (f.category ? 1 : 0) + (f.capability ? 1 : 0) + (f.platform ? 1 : 0) + (f.compatibleOnly ? 1 : 0);
}

/** Modules "update all" would touch: installed, update available, usable. */
export function updatable(entries: readonly CatalogEntry[]): CatalogEntry[] {
  return entries.filter(e => e.updateAvailable && e.compatible && e.view.state.status !== 'installing');
}

export interface Window { first: number; last: number; before: number; after: number }
/**
 * Fixed-height row windowing: `rows` rows of `rowHeight` px (gap included). Returns the rows
 * to mount and the spacer heights above and below them.
 */
export function rowWindow(rows: number, rowHeight: number, scrollTop: number, viewport: number, overscan = 3): Window {
  if (rows <= 0 || rowHeight <= 0) return { first: 0, last: -1, before: 0, after: 0 };
  const first = Math.max(0, Math.min(rows - 1, Math.floor(scrollTop / rowHeight) - overscan));
  const last = Math.min(rows - 1, Math.ceil((scrollTop + viewport) / rowHeight) + overscan);
  return { first, last, before: first * rowHeight, after: Math.max(0, (rows - 1 - last) * rowHeight) };
}

/** Next index for arrow-key navigation in a grid with `columns` columns; -1 for other keys. */
export function navigate(index: number, key: string, count: number, columns: number, page = 1): number {
  if (count === 0) return -1;
  const clamp = (i: number) => Math.max(0, Math.min(count - 1, i));
  switch (key) {
    case 'ArrowRight': return clamp(index + 1);
    case 'ArrowLeft': return clamp(index - 1);
    case 'ArrowDown': return index + columns < count ? index + columns : index;
    case 'ArrowUp': return index - columns >= 0 ? index - columns : index;
    case 'PageDown': return clamp(index + columns * page);
    case 'PageUp': return clamp(index - columns * page);
    case 'Home': return 0;
    case 'End': return count - 1;
    default: return -1;
  }
}

/** Short human size; zero and unknown read as an em dash. */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '—';
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GiB`;
  return `${Math.max(1, Math.ceil(n / 1024 ** 2))} MiB`;
}
