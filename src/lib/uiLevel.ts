export type UiLevel = 'simple' | 'advanced';

export function isVisible(level: 'advanced' | undefined, current: UiLevel): boolean {
  return level !== 'advanced' || current === 'advanced';
}

/** Keep the original data intact: hidden pages remain valid navigation targets. */
export function visibleGroups<P extends { level?: 'advanced' }, G extends { level?: 'advanced'; pages: P[] }>(groups: G[], current: UiLevel): G[] {
  return groups.filter(group => isVisible(group.level, current))
    .map(group => ({ ...group, pages: group.pages.filter(page => isVisible(page.level, current)) }))
    .filter(group => group.pages.length > 0);
}

export function pageLevel(groups: { level?: 'advanced'; pages: { id: string; level?: 'advanced' }[] }[], id: string): 'advanced' | undefined {
  const group = groups.find(group => group.pages.some(page => page.id === id));
  return group?.level ?? group?.pages.find(page => page.id === id)?.level;
}

/** Restore a visible page without writing over the user's saved preference. */
export function initialPage(groups: { level?: 'advanced'; pages: { id: string; level?: 'advanced' }[] }[], saved: string, current: UiLevel): string {
  const pages = visibleGroups(groups, current).flatMap(group => group.pages);
  return pages.find(page => page.id === saved)?.id ?? pages[0]?.id ?? saved;
}

export const advancedTabs = ['servers', 'create', 'performance', 'analytics'] as const;
export function tabLevel(tab: string): 'advanced' | undefined {
  return advancedTabs.some(id => id === tab) ? 'advanced' : undefined;
}
