export function linkWarning(link: { state: string; warning: string | null } | null): string {
  return link?.state === 'online' ? link.warning ?? '' : '';
}
