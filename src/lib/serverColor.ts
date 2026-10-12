// One colour per running server, the same everywhere it shows up: the telemetry card, its
// chip, the log tag and the segment of the memory bars. Colours are the theme's data tokens
// (`--chart-1..5`, defined by every theme); from the sixth server on they repeat with a hatch,
// so two servers never differ by colour alone.

const CHART_COLORS = 5;
export const SLOT_COUNT = CHART_COLORS * 2;

/**
 * Keeps the slot of every server that still runs and gives each new one the lowest free slot
 * (past `SLOT_COUNT` running servers they share, starting over from 0).
 */
export function assignSlots(runningIds: string[], prev: Record<string, number>): Record<string, number> {
  const next: Record<string, number> = {};
  const used = new Set<number>();
  for (const id of runningIds) {
    const slot = prev[id];
    if (slot !== undefined && !used.has(slot)) {
      next[id] = slot;
      used.add(slot);
    }
  }
  let overflow = 0;
  for (const id of runningIds) {
    if (next[id] !== undefined) continue;
    let slot = 0;
    while (used.has(slot) && slot < SLOT_COUNT) slot++;
    if (slot >= SLOT_COUNT) slot = overflow++ % SLOT_COUNT;
    next[id] = slot;
    used.add(slot);
  }
  return next;
}

/** CSS colour of a slot: a theme token, so it follows dark / light / OLED / high contrast. */
export function slotColor(slot: number): string {
  return `var(--chart-${(slot % CHART_COLORS) + 1})`;
}

/** Slots from the sixth on carry a hatch on top of the repeated colour. */
export function slotHatched(slot: number): boolean {
  return slot >= CHART_COLORS;
}
