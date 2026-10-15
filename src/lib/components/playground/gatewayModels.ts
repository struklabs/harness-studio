// Playground model list while the gateway runs: the managed endpoint's /v1/models lists every
// model definition (alias as `id`) with the scheduler's state under `harness`.
import { isRemoteModel } from './remoteModels.ts';

export type ModelLoadState = 'loaded' | 'loading' | 'unloaded';

export interface GatewayModelEntry {
  id?: string;
  owned_by?: string;
  harness?: { definition_id?: string; state?: 'unloaded' | 'loading' | 'ready' | 'draining' };
}

/**
 * alias -> load state. The scheduler only tracks what it loaded or routed to; a server the user
 * started for a definition counts as loaded too (the gateway routes to it).
 */
export function gatewayModelStates(list: GatewayModelEntry[], runState: (id: string) => string | undefined = () => undefined): Record<string, ModelLoadState> {
  const out: Record<string, ModelLoadState> = {};
  for (const m of list) {
    if (typeof m?.id !== 'string') continue;
    if (m.owned_by === 'harness-link' || isRemoteModel(m.id)) continue;
    const slot = m.harness?.state;
    const run = m.harness?.definition_id ? runState(m.harness.definition_id) : undefined;
    out[m.id] =
      slot === 'ready' || slot === 'draining' || run === 'running'
        ? 'loaded'
        : slot === 'loading' || run === 'loading'
          ? 'loading'
          : 'unloaded';
  }
  return out;
}
