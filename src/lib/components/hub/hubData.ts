// Model Hub data: Hugging Face listing pages with a short in-memory cache (returning to the Hub
// is instant and the list doesn't re-hit the API), plus the remembered list/filter choices.
import { invoke } from '#lib/ipc.js';
import type { HfSearchOptions, HfSearchPage, HfSortKey } from '#lib/types.js';

const TTL_MS = 5 * 60_000;
const MAX_ENTRIES = 80;

const cache = new Map<string, { at: number; page: HfSearchPage }>();
const inflight = new Map<string, Promise<HfSearchPage>>();

function keyOf(o: HfSearchOptions): string {
  return JSON.stringify([
    o.query.trim(),
    o.pipeline_tag ?? '',
    o.license ?? '',
    o.author?.trim() ?? '',
    o.include_non_gguf ? 1 : 0,
    o.sort ?? '',
    o.limit ?? '',
    o.cursor ?? '',
  ]);
}

/** A fresh cached page, or null. */
export function peekPage(o: HfSearchOptions): HfSearchPage | null {
  const hit = cache.get(keyOf(o));
  return hit && Date.now() - hit.at < TTL_MS ? hit.page : null;
}

/** Forgets the cached pages (what a page says about installed runtimes changed). */
export function clearPageCache() {
  cache.clear();
}

/** One listing page (cached for a few minutes; concurrent identical requests share one call). */
export function fetchPage(o: HfSearchOptions): Promise<HfSearchPage> {
  const key = keyOf(o);
  const fresh = peekPage(o);
  if (fresh) return Promise.resolve(fresh);
  const running = inflight.get(key);
  if (running) return running;
  const p = invoke<HfSearchPage>('search_hf_models_page', { options: o })
    .then((page) => {
      cache.delete(key);
      cache.set(key, { at: Date.now(), page });
      while (cache.size > MAX_ENTRIES) cache.delete(cache.keys().next().value as string);
      return page;
    })
    .finally(() => inflight.delete(key));
  inflight.set(key, p);
  return p;
}

/** What the Hub remembers between visits. */
export interface HubPrefs {
  query: string;
  task: string;
  license: string;
  author: string;
  includeNonGguf: boolean;
  sort: HfSortKey;
  viewMode: 'grid' | 'list';
  quant: string;
  size: string;
  showFilters: boolean;
  bestHardware: boolean;
}

const PREFS_KEY = 'harness_hub_prefs_v2';
const LEGACY_PREFS_KEY = 'harness_hub_prefs_v1';
const SORTS: HfSortKey[] = ['downloads', 'likes', 'trendingScore', 'lastModified', 'createdAt'];

export const DEFAULT_PREFS: HubPrefs = {
  query: '',
  task: '',
  license: '',
  author: '',
  includeNonGguf: true,
  sort: 'trendingScore',
  viewMode: 'grid',
  quant: 'ALL',
  size: 'ALL',
  showFilters: false,
  bestHardware: false,
};

export function loadPrefs(): HubPrefs {
  try {
    const saved = localStorage.getItem(PREFS_KEY);
    const raw = JSON.parse(saved ?? localStorage.getItem(LEGACY_PREFS_KEY) ?? '{}') as Partial<HubPrefs>;
    // v1's off switch still injected runnable non-GGUF suggestions. Start the new strict
    // format filter at all formats; keep the other remembered search/task/sort choices.
    if (saved === null) raw.includeNonGguf = DEFAULT_PREFS.includeNonGguf;
    const str = (v: unknown, d: string) => (typeof v === 'string' ? v : d);
    return {
      query: str(raw.query, ''),
      task: str(raw.task, ''),
      license: str(raw.license, ''),
      author: str(raw.author, ''),
      includeNonGguf: typeof raw.includeNonGguf === 'boolean' ? raw.includeNonGguf : DEFAULT_PREFS.includeNonGguf,
      sort: SORTS.includes(raw.sort as HfSortKey) ? (raw.sort as HfSortKey) : DEFAULT_PREFS.sort,
      viewMode: raw.viewMode === 'list' ? 'list' : 'grid',
      quant: str(raw.quant, 'ALL'),
      size: str(raw.size, 'ALL'),
      showFilters: raw.showFilters === true,
      bestHardware: raw.bestHardware === true,
    };
  } catch {
    return { ...DEFAULT_PREFS };
  }
}

export function savePrefs(p: HubPrefs) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(p));
  } catch {
    // storage unavailable: the Hub just starts from the defaults next time
  }
}
