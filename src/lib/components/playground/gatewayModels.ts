// Playground model list while the gateway runs: the managed endpoint's /v1/models lists every
// model definition (alias as `id`) with the scheduler's state under `harness`.
import { domain } from '#lib/stores/domain.svelte.js';

export type ModelLoadState = 'loaded' | 'loading' | 'unloaded';

export interface GatewayModelEntry {
  id?: string;
  harness?: { definition_id?: string; state?: 'unloaded' | 'loading' | 'ready' | 'draining' };
}

/**
 * alias -> load state. The scheduler only tracks what it loaded or routed to; a server the user
 * started for a definition counts as loaded too (the gateway routes to it).
 */
export function gatewayModelStates(list: GatewayModelEntry[]): Record<string, ModelLoadState> {
  const out: Record<string, ModelLoadState> = {};
  for (const m of list) {
    if (typeof m?.id !== 'string') continue;
    const slot = m.harness?.state;
    const run = m.harness?.definition_id ? domain.state(m.harness.definition_id)?.state : undefined;
    out[m.id] =
      slot === 'ready' || slot === 'draining' || run === 'running'
        ? 'loaded'
        : slot === 'loading' || run === 'loading'
          ? 'loading'
          : 'unloaded';
  }
  return out;
}
