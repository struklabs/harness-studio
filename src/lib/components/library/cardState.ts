import type { HardwareFitResult } from '#lib/types.js';

export interface CardState {
  busy: Record<string, boolean>;
  repairing: boolean;
  repairUrl: string;
  startingChat: boolean;
  openingPlayground: boolean;
  detailsOpen: boolean;
  fit: HardwareFitResult | null;
}

export function createCardState(): CardState {
  return { busy: {}, repairing: false, repairUrl: '', startingChat: false, openingPlayground: false, detailsOpen: false, fit: null };
}

/** Returns false for a second invocation, even through a newly mounted card. */
export function beginDefinitionAction(state: CardState, id: string): boolean {
  if (state.busy[id]) return false;
  state.busy[id] = true;
  return true;
}

/** Library-owned cache shares both pending requests and completed results across remounts. */
export class FitCache {
  private paths = new Map<string, { key: string; result: Promise<HardwareFitResult> }>();

  get(path: string, key: string, load: () => Promise<HardwareFitResult>): Promise<HardwareFitResult> {
    const cached = this.paths.get(path);
    if (cached?.key === key) return cached.result;
    const result = load();
    this.paths.set(path, { key, result });
    void result.catch(() => {
      if (this.paths.get(path)?.result === result) this.paths.delete(path);
    });
    return result;
  }
}
