/** Reserved non-filesystem URI: explicit chat opt-out. Empty/missing follows the default. */
export const NO_WORKSPACE = 'harness://no-workspace';

/** Windows paths ignore case and separator spelling; POSIX paths remain case-sensitive. */
export function folderKey(path: string): string {
  const trimmed = path.trim();
  if (!trimmed) return '';
  return /^(?:[a-z]:[\\/]|\\\\|\/\/)/i.test(trimmed)
    ? trimmed.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
    : trimmed.replace(/\/+$/, '') || '/';
}

export function sameFolder(a: string, b: string): boolean {
  return folderKey(a) === folderKey(b);
}

/** MRU order, retaining the first saved spelling of an existing folder. */
export function recentFolders(folders: readonly string[], path: string): string[] {
  const folder = path.trim();
  const existing = folders.map(p => p.trim()).filter(Boolean);
  const selected = existing.find(p => sameFolder(p, folder)) ?? folder;
  const seen = new Set<string>();
  return [selected, ...existing].filter(p => {
    const key = folderKey(p);
    if (!p || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function folderName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  return trimmed.split(/[\\/]/).at(-1) || path;
}
