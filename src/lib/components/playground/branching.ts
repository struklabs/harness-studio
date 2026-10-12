// Pure helpers for the conversation tree in the Playground: which message can be regenerated,
// edited or forked, and what the visible list looks like while that happens. No imports of the
// app (testable with plain node).

/** The fields of a UI message these helpers read. */
export interface BranchMessage {
  id: string;
  role: string;
  variantId?: string;
  variantIds?: string[];
  variantIndex?: number;
}

/** Index of the first assistant message of the turn that contains `index`. */
export function turnRoot(messages: { role: string }[], index: number): number {
  let i = index;
  while (i > 0 && messages[i - 1].role === 'assistant') i--;
  return i;
}

/** Last assistant message of a turn (anywhere in the history, not only the end of the chat). */
export function isTurnEnd(messages: { role: string }[], i: number): boolean {
  return messages[i]?.role === 'assistant' && (i === messages.length - 1 || messages[i + 1].role !== 'assistant');
}

/** Variant arrows (< 1/3 >) of the turn that ends at `i`; the variants belong to the turn's first message. */
export function variantNavOf(messages: BranchMessage[], i: number): { index: number; count: number } | null {
  if (!isTurnEnd(messages, i)) return null;
  const root = messages[turnRoot(messages, i)];
  const count = root.variantIds?.length ?? 1;
  return count > 1 ? { index: root.variantIndex ?? 0, count } : null;
}

/** The variant to show after pressing an arrow, or null at the ends. */
export function variantTarget(root: BranchMessage, dir: -1 | 1): string | null {
  return root.variantIds?.[(root.variantIndex ?? 0) + dir] ?? null;
}

export interface RegeneratePlan {
  /** How many messages of the visible list stay (everything after them leaves the path). */
  keep: number;
  /** Store the new answer as another variant of this message (undefined: a new message). */
  variantOf?: string;
}

/**
 * Regenerating an assistant message anywhere in the history: the history up to the parent of
 * the turn's first assistant message stays, the answer becomes a new variant of that message;
 * the messages after it belong to the old variant (switching variants brings them back).
 * Regenerating a user message re-sends it: the history up to and including it stays.
 * `stored` tells whether a variant is already in the database (else there is nothing to vary).
 */
export function regeneratePlan(
  messages: BranchMessage[],
  index: number,
  stored: (variantId: string) => boolean,
): RegeneratePlan {
  const m = messages[index];
  if (m.role === 'user') return { keep: index + 1 };
  const root = turnRoot(messages, index);
  const rootMsg = messages[root];
  const isStored = !!rootMsg.variantId && stored(rootMsg.variantId);
  return { keep: root, variantOf: isStored ? rootMsg.id : undefined };
}
