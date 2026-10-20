import type { ParamValues } from './types.js';

/** Leave saved settings intact until the user explicitly chooses a resource. */
export function resourceOverrides(choice: string, deviceIndices: number[]): ParamValues | null {
  if (choice === 'saved') return {};
  if (choice === 'cpu') return { gpu_layers: 0 };
  if (choice === 'auto') return { gpu_layers: '', main_gpu: '', split_mode: '', tensor_split: '' };
  if (!/^\d+$/.test(choice) || !deviceIndices.includes(Number(choice))) return null;
  return { gpu_layers: '', main_gpu: Number(choice), split_mode: 'none', tensor_split: '' };
}

/** Remote IDs are stable even when the link cache is unavailable. */
export function executionLocation(model: string): { key: 'setup.this_pc' | 'setup.remote'; device?: string } {
  const remote = /^remote:([a-z0-9](?:[a-z0-9-]*[a-z0-9])?)\/.+/.exec(model);
  return remote ? { key: 'setup.remote', device: remote[1] } : { key: 'setup.this_pc' };
}
