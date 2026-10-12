/** A new chat keeps a loaded choice; otherwise follows the selected running chat server. */
export function selectChatModel(
  previous: string,
  available: string[],
  states: Record<string, string>,
  running: string | null,
): string {
  if (available.includes(previous) && (states[previous] === 'loaded' || previous === running)) return previous;
  if (running && available.includes(running)) return running;
  return available.find(alias => states[alias] === 'loaded') ?? (available.includes(previous) ? previous : available[0] ?? '');
}
