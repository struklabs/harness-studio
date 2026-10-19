export type RegistryKind = 'tool' | 'skill' | 'mcp';
export interface RegistryEntry {
  id: string; type: RegistryKind; name: string; description: { en: string; tr: string };
  version: string; license: string; source: string; homepage: string; tags: string[];
  artifact: string; min_harness_version: string; publisher: string; platforms: string[];
  permissions: { network: string[]; fs: { read: string[]; write: string[] }; shell: boolean };
}

export function filterCatalog(entries: RegistryEntry[], query: string, kind: string, sort: string, language: 'en' | 'tr'): RegistryEntry[] {
  const term = query.trim().toLocaleLowerCase(language);
  return entries.filter(e => (kind === 'all' || e.type === kind) && [e.name, e.description.en, e.description.tr, ...e.tags].join(' ').toLocaleLowerCase(language).includes(term))
    .toSorted((a, b) => (sort === 'type' ? a.type.localeCompare(b.type) : 0) || a.name.localeCompare(b.name, language));
}

/** Use the existing saved definition, retaining unrelated servers and keychain markers. */
export function mergeRegistryServers<T>(current: Record<string, T>, incoming: Record<string, T>): Record<string, T> | null {
  if (Object.keys(incoming).some(name => Object.hasOwn(current, name))) return null;
  return { ...current, ...incoming };
}
