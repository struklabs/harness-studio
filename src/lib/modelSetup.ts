import type { GpuInfo, ParamValues, RuntimeDeviceLive } from './types.js';

/** Leave saved settings intact until the user explicitly chooses a resource. */
export function resourceOverrides(
  choice: string,
  devices: Pick<RuntimeDeviceLive, 'index' | 'name'>[],
  adapters: Pick<GpuInfo, 'index' | 'name'>[],
): ParamValues | null {
  if (choice === 'saved') return {};
  if (choice === 'cpu') return { gpu_layers: 0 };
  if (choice === 'auto') return { gpu_layers: '', main_gpu: '', split_mode: '', tensor_split: '' };
  if (!/^\d+$/.test(choice)) return null;
  // Match the backend's pair_adapters: normalized names, adapter order, each device once.
  // Runtime indices (CUDA0 on a hybrid laptop) are not LaunchConfig adapter indices.
  const normalize = (name: string) => name.toLowerCase().replace(/\(tm\)|\(r\)/g, ' ')
    .split(/[^a-z0-9]+/).filter(Boolean).join(' ');
  const taken = new Set<number>();
  for (const adapter of [...adapters].sort((a, b) => a.index - b.index)) {
    const name = normalize(adapter.name);
    if (!name) continue;
    const position = devices.findIndex((device, i) => {
      const runtimeName = normalize(device.name);
      return !taken.has(i) && runtimeName !== '' && (name.includes(runtimeName) || runtimeName.includes(name));
    });
    if (position < 0) continue;
    taken.add(position);
    if (devices[position].index === Number(choice)) {
      return { gpu_layers: '', main_gpu: adapter.index, split_mode: 'none', tensor_split: '' };
    }
  }
  return null;
}

/** Remote and cloud IDs are stable even when the link or provider cache is unavailable. */
export function executionLocation(model: string): { key: 'setup.this_pc' | 'setup.remote' | 'setup.cloud'; device?: string } {
  const remote = /^remote:([a-z0-9](?:[a-z0-9-]*[a-z0-9])?)\/.+/.exec(model);
  if (remote) return { key: 'setup.remote', device: remote[1] };
  const cloud = /^cloud:([a-z0-9](?:[a-z0-9-]*[a-z0-9])?)\/.+/.exec(model);
  return cloud ? { key: 'setup.cloud', device: cloud[1] } : { key: 'setup.this_pc' };
}
