export function purposeFooterAction(count: number, busy: boolean, done: boolean, downloading: number): 'install' | 'continue' | 'skip' {
  if (busy || done || downloading > 0) return 'continue';
  return count > 0 ? 'install' : 'skip';
}
