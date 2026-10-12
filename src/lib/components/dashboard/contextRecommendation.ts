/** The core prepare plan already accounts for trained context and available GPU/RAM. */
export function recommendedChatContext(trained: number | null | undefined, fitting: number | null | undefined, fallback: number): number {
  const ceiling = trained && trained > 0 ? trained : 32_768;
  return Math.max(512, Math.min(ceiling, fitting && fitting > 0 ? fitting : fallback));
}
