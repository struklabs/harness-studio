// Pure logic of the first-run Setup window (src/lib/components/SetupWindow.svelte): the hardware
// profile, which items are offered and ticked by default, and the run state of the install screen.
// No app imports, so node can test it (scripts/test-setup.mjs).

export type SetupItemId = 'engine' | 'model' | 'agents' | 'cli' | 'opencode' | 'zed' | 'vscode';
export type SetupCategoryId = 'engine' | 'model' | 'agents' | 'server' | 'integrations';

export const CATEGORY_ITEMS: Record<SetupCategoryId, SetupItemId[]> = {
  engine: ['engine'],
  model: ['model'],
  agents: ['agents'],
  server: ['cli'],
  integrations: ['opencode', 'zed', 'vscode'],
};
export const CATEGORY_ORDER: SetupCategoryId[] = ['engine', 'model', 'agents', 'server', 'integrations'];
/** Install order: the engine and the model first, integrations last (they point at the model). */
export const ITEM_ORDER: SetupItemId[] = ['engine', 'model', 'agents', 'cli', 'opencode', 'zed', 'vscode'];

/** The Setup id of an integration and the id of the same tool in the integrations registry. */
export const INTEGRATION_REGISTRY_ID: Partial<Record<SetupItemId, string>> = {
  opencode: 'opencode',
  zed: 'zed',
  vscode: 'vscode-copilot',
};

// ---- hardware ----

export type Accelerator = 'apple' | 'nvidia' | 'amd' | 'intel' | 'cpu';
export type Tier = 'low' | 'mid' | 'high';

/** The parts of `SystemHardwareInfo` the profile reads. */
export interface HardwareLike {
  os?: string;
  total_ram_bytes: number;
  available_ram_bytes?: number;
  gpus: { vendor: unknown; vram_bytes: number | null; is_discrete: boolean }[];
  primary_gpu_index?: number | null;
  unified_memory_budget_bytes?: number | null;
  recommended_backend: 'Cuda12' | 'Vulkan' | 'Metal' | 'Cpu' | 'Rocm' | 'Sycl';
}

export interface HardwareProfile {
  accelerator: Accelerator;
  /** Token of the llama.cpp build for this machine (`cuda`, `vulkan`, `metal`, `cpu`, `rocm`, `sycl`). */
  backend: string;
  ramBytes: number;
  /** Memory a model can use on the accelerator (VRAM, or the unified-memory budget); 0 on a CPU. */
  acceleratorBytes: number;
  tier: Tier;
}

const GIB = 1024 ** 3;
const BACKEND_OF: Record<HardwareLike['recommended_backend'], string> = {
  Cuda12: 'cuda',
  Vulkan: 'vulkan',
  Metal: 'metal',
  Cpu: 'cpu',
  Rocm: 'rocm',
  Sycl: 'sycl',
};

function vendorOf(vendor: unknown): 'nvidia' | 'amd' | 'intel' | 'apple' | 'other' {
  if (vendor === 'Nvidia') return 'nvidia';
  if (vendor === 'Amd') return 'amd';
  if (vendor === 'Intel') return 'intel';
  if (vendor === 'Apple') return 'apple';
  return 'other';
}

/** Hardware that could not be read is treated as a small CPU-only machine: the safest choice. */
export function profileOf(hw: HardwareLike | null | undefined): HardwareProfile {
  if (!hw) return { accelerator: 'cpu', backend: 'cpu', ramBytes: 0, acceleratorBytes: 0, tier: 'low' };
  const ram = hw.total_ram_bytes;
  const primary = hw.gpus[hw.primary_gpu_index ?? 0] ?? hw.gpus.find((g) => g.is_discrete) ?? hw.gpus[0];
  const backend = BACKEND_OF[hw.recommended_backend] ?? 'cpu';
  const apple = backend === 'metal' || vendorOf(primary?.vendor) === 'apple';
  let accelerator: Accelerator = 'cpu';
  let accel = 0;
  if (apple) {
    accelerator = 'apple';
    accel = hw.unified_memory_budget_bytes ?? Math.floor(ram * 0.66);
  } else if (backend !== 'cpu' && primary) {
    const v = vendorOf(primary.vendor);
    accelerator = v === 'nvidia' ? 'nvidia' : v === 'amd' ? 'amd' : v === 'intel' ? 'intel' : 'cpu';
    // An integrated GPU shares system memory: only a discrete card counts as its own budget.
    accel = primary.is_discrete ? (primary.vram_bytes ?? 0) : 0;
    if (accelerator === 'cpu') accel = 0;
  }
  const tier: Tier =
    accel >= 12 * GIB || (accelerator === 'apple' && ram >= 24 * GIB) ? 'high'
    : accel >= 6 * GIB || ram >= 16 * GIB ? 'mid'
    : 'low';
  return { accelerator, backend, ramBytes: ram, acceleratorBytes: accel, tier };
}

// ---- first model ----

export type ModelTierId = 'tiny' | 'small' | 'standard';

export interface ModelTier {
  id: ModelTierId;
  repo: string;
  /** Preferred quantization of the file; the smallest 4-bit file when the repo has none like it. */
  quant: string;
  /** Rough file size, shown until the real size of the file is read. */
  approxBytes: number;
}

export const MODEL_TIERS: ModelTier[] = [
  { id: 'tiny', repo: 'unsloth/Qwen3-0.6B-GGUF', quant: 'Q4_K_M', approxBytes: 0.4 * GIB },
  { id: 'small', repo: 'unsloth/Qwen3-1.7B-GGUF', quant: 'Q4_K_M', approxBytes: 1.1 * GIB },
  { id: 'standard', repo: 'unsloth/Qwen3-4B-Instruct-2507-GGUF', quant: 'Q4_K_M', approxBytes: 2.5 * GIB },
];

export function modelTier(id: ModelTierId): ModelTier {
  return MODEL_TIERS.find((t) => t.id === id) ?? MODEL_TIERS[0];
}

/**
 * The first model that fits: the 4B instruct model only where the accelerator (or enough RAM)
 * holds it comfortably, a 1.7B model on a middle machine, else the tiny one.
 */
export function recommendedModelTier(profile: HardwareProfile): ModelTierId {
  if (profile.accelerator !== 'cpu' && profile.acceleratorBytes >= 6 * GIB && profile.ramBytes >= 12 * GIB) return 'standard';
  if (profile.accelerator === 'cpu' && profile.ramBytes >= 24 * GIB) return 'standard';
  if (profile.ramBytes >= 8 * GIB) return 'small';
  return 'tiny';
}

// ---- what is offered and ticked ----

/** What the window found on this machine. */
export interface SetupFacts {
  /** Already set up: the item is shown as done and not ticked. */
  installed: Partial<Record<SetupItemId, boolean>>;
  /** Cannot be installed here (platform or memory): shown disabled with a reason. */
  unavailable: Partial<Record<SetupItemId, boolean>>;
  /** Free space of the models folder; null when unknown. */
  freeDiskBytes: number | null;
}

/** Free space the first model needs on top of its own size (temporary files, the engine). */
export const DISK_HEADROOM = 2 * GIB;
/** Below this much free space nothing is ticked by default (the engine alone needs a few hundred MB). */
export const MIN_FREE_DISK = 1 * GIB;

/**
 * Minimal and safe: the engine and one small first model, and only where the machine and the disk
 * fit. Agents, the command-line tool and the integrations change more than the app itself and are
 * never ticked on their own.
 */
export function defaultSelection(profile: HardwareProfile, facts: SetupFacts, tier?: ModelTierId): Set<SetupItemId> {
  const selected = new Set<SetupItemId>();
  const open = (id: SetupItemId) => !facts.installed[id] && !facts.unavailable[id];
  // Almost no free space: tick nothing, the user decides what still fits.
  if (facts.freeDiskBytes !== null && facts.freeDiskBytes < MIN_FREE_DISK) return selected;
  if (open('engine')) selected.add('engine');
  if (open('model') && profile.ramBytes >= 4 * GIB) {
    const size = modelTier(tier ?? recommendedModelTier(profile)).approxBytes;
    if (facts.freeDiskBytes === null || facts.freeDiskBytes >= size + DISK_HEADROOM) selected.add('model');
  }
  return selected;
}

/** Whether a Hermes-class agent is worth offering: it builds a Python environment (about 1.5 GB). */
export function agentsFit(profile: HardwareProfile): boolean {
  return profile.ramBytes >= 8 * GIB;
}

export interface Sized {
  id: SetupItemId;
  bytes: number;
}

/** Total download of the ticked items (items without a known size count as 0). */
export function totalDownload(selected: ReadonlySet<SetupItemId>, sizes: Partial<Record<SetupItemId, number>>): number {
  let sum = 0;
  for (const id of selected) sum += sizes[id] ?? 0;
  return sum;
}

export function orderedSelection(selected: ReadonlySet<SetupItemId>): SetupItemId[] {
  return ITEM_ORDER.filter((id) => selected.has(id));
}

// ---- the install screen ----

export type RunState = 'pending' | 'running' | 'done' | 'failed' | 'skipped' | 'cancelled';

export interface RunItem {
  id: SetupItemId;
  state: RunState;
  /** 0 to 1 while the item reports progress, else null. */
  fraction: number | null;
  /** Localized detail line or error text. */
  detail: string | null;
}

export function initialRun(ids: SetupItemId[]): RunItem[] {
  return ids.map((id) => ({ id, state: 'pending', fraction: null, detail: null }));
}

export function patchRun(items: RunItem[], id: SetupItemId, patch: Partial<Omit<RunItem, 'id'>>): RunItem[] {
  return items.map((item) => (item.id === id ? { ...item, ...patch } : item));
}

/** Overall progress: finished items count fully, the running one by its own fraction. */
export function runProgress(items: RunItem[]): number {
  if (items.length === 0) return 1;
  let sum = 0;
  for (const item of items) {
    if (item.state === 'done' || item.state === 'skipped' || item.state === 'failed' || item.state === 'cancelled') sum += 1;
    else if (item.state === 'running') sum += item.fraction ?? 0;
  }
  return sum / items.length;
}

export const failedItems = (items: RunItem[]) => items.filter((i) => i.state === 'failed');
export const isFinished = (items: RunItem[]) => items.every((i) => i.state !== 'pending' && i.state !== 'running');

/** Ids Retry runs again: the ones that failed or were cancelled. */
export function retryIds(items: RunItem[]): SetupItemId[] {
  return items.filter((i) => i.state === 'failed' || i.state === 'cancelled').map((i) => i.id);
}

/** Marks everything that has not started as cancelled (the running item is stopped by its own cancel). */
export function cancelPending(items: RunItem[]): RunItem[] {
  return items.map((i) => (i.state === 'pending' ? { ...i, state: 'cancelled' as const } : i));
}
