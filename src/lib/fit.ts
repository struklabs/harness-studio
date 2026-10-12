// Memory fit of a model on this machine. The estimate itself lives in Rust
// (`harness_core::vram`: sliding-window KV, projector, cache types, corrected with measured
// loads); this module batches requests into one IPC call and adds the UI labels.
import { invoke } from '$lib/ipc';
import { t, type TKey } from '$lib/i18n.svelte';
import type { FitCategory, FitInput, FitResult, HardwareFitResult, LaunchConfig } from '$lib/types';

interface Pending {
  input: FitInput;
  resolve: (r: FitResult) => void;
  reject: (e: unknown) => void;
}

let queue: Pending[] = [];
let scheduled = false;

async function flush() {
  const batch = queue;
  queue = [];
  scheduled = false;
  try {
    const results = await invoke<FitResult[]>('estimate_fit', { requests: batch.map((p) => p.input) });
    batch.forEach((p, i) => p.resolve(results[i]));
  } catch (e) {
    batch.forEach((p) => p.reject(e));
  }
}

/** One estimate; calls made in the same tick (a list rendering) share one IPC round trip. */
export function estimateFit(input: FitInput): Promise<FitResult> {
  return new Promise((resolve, reject) => {
    queue.push({ input, resolve, reject });
    if (!scheduled) {
      scheduled = true;
      queueMicrotask(flush);
    }
  });
}

/** Contexts a default load steps down through to keep the whole model on the GPU (largest first; mirrors `FULL_OFFLOAD_STEPS` in Rust). */
const FULL_OFFLOAD_CONTEXTS = [32768, 24576, 16384, 12288, 8192];
const FULL_OFFLOAD_MIN_CONTEXT = 8192;

/**
 * Whether a file runs here as a default load starts it: automatic KV precision and, when the
 * full offload does not fit, the GPU layers the estimate recommends (a partial offload, not
 * "out of memory"). Measured on a 4 GB RTX 3050 Ti: a fixed f16 cache at 32k context marked
 * every file of a 2B vision model, the 565 MB projector included, as not fitting. A smaller
 * context that keeps the whole model on the GPU wins over the requested one.
 */
export async function defaultLoadFit(input: FitInput): Promise<FitResult> {
  const requested = await estimateFit({ kv_auto: true, ...input });
  // Full GPU offload beats context on small cards (`full_offload_context` in the gateway
  // scheduler): when the requested context keeps layers on the CPU, the largest smaller
  // context from `FULL_OFFLOAD_CONTEXTS` that fits whole is what the default load starts with.
  const ctx = input.context_size ?? 0;
  const wholeOffload = (r: FitResult) => r.recommended_gpu_layers > r.estimate.total_layers;
  let full = requested;
  if (!wholeOffload(requested) && requested.budget.vram_bytes > 0 && ctx > FULL_OFFLOAD_MIN_CONTEXT) {
    for (const c of FULL_OFFLOAD_CONTEXTS.filter((c) => c < ctx)) {
      const r = await estimateFit({ kv_auto: true, ...input, context_size: c });
      if (wholeOffload(r)) {
        full = r;
        break;
      }
    }
  }
  const layers = full.recommended_gpu_layers;
  if (full.category !== 'oom_risk' || !layers || layers >= (input.gpu_layers ?? 999)) return full;
  const partial = await estimateFit({ kv_auto: true, ...input, gpu_layers: layers });
  return partial.category === 'oom_risk' ? full : partial;
}

/** Estimate an exact Dashboard launch through the launcher's core slot plan. */
export function estimateLaunchFit(config: LaunchConfig, serverId?: string | null): Promise<FitResult> {
  return invoke<FitResult>('estimate_launch_fit', { config, serverId });
}

const LABELS: Record<FitCategory, { label: TKey; desc: TKey; badge: string }> = {
  full_gpu: { label: 'fit.full_gpu', desc: 'fit.full_gpu_desc', badge: 'fit-full-gpu' },
  tight: { label: 'fit.tight', desc: 'fit.tight_desc', badge: 'fit-hybrid' },
  hybrid: { label: 'fit.hybrid', desc: 'fit.hybrid_desc', badge: 'fit-hybrid' },
  cpu_heavy: { label: 'fit.cpu_heavy', desc: 'fit.cpu_heavy_desc', badge: 'fit-cpu-heavy' },
  oom_risk: { label: 'fit.oom', desc: 'fit.oom_desc', badge: 'fit-oom-risk' },
};

/** The Rust result in the shape the fit badges and indicators render. */
// Integrated GPU (shared RAM): weights on the CPU is the recommended setup, not a warning.
const IGPU_LABELS: Partial<Record<FitCategory, { label: TKey; desc: TKey }>> = {
  cpu_heavy: { label: 'fit.igpu_cpu', desc: 'fit.igpu_cpu_desc' },
  full_gpu: { label: 'fit.igpu_full', desc: 'fit.igpu_full_desc' },
  tight: { label: 'fit.igpu_full', desc: 'fit.igpu_full_desc' },
};

export function toHardwareFit(r: FitResult): HardwareFitResult {
  const sharedIgpu = !!r.budget.shared_igpu;
  const l = { ...LABELS[r.category], ...(sharedIgpu ? IGPU_LABELS[r.category] : undefined) };
  return {
    category: r.category,
    label: t(l.label),
    description: t(l.desc),
    badgeClass: l.badge,
    emoji: '',
    requiredVramBytes: r.estimate.vram_bytes,
    requiredRamBytes: r.estimate.ram_bytes,
    vramPercentage: r.vram_percentage,
    ramPercentage: r.ram_percentage,
    kvCacheBytes: r.estimate.kv_bytes,
    offloadedLayers: r.estimate.offloaded_layers,
    totalLayers: r.estimate.total_layers,
    isEstimate: r.source !== 'measured',
    exact: r.estimate.exact,
    source: r.source,
    recommendedGpuLayers: r.recommended_gpu_layers,
    swaLayers: r.estimate.swa_layers,
    sharedIgpu,
    vramBudgetBytes: r.budget.vram_bytes,
    unifiedMemory: r.budget.unified,
    slots: r.slots ?? null,
    kv: r.kv ?? null,
    draftGpuBytes: r.estimate.draft_gpu_bytes ?? 0,
    draftHostBytes: r.estimate.draft_host_bytes ?? 0,
  };
}

/** [`defaultLoadFit`] + labels in one call. */
export async function hardwareFit(input: FitInput): Promise<HardwareFitResult> {
  return toHardwareFit(await defaultLoadFit(input));
}

/** Empty strings of the launch form mean "server default". */
export const orNull = (v: string | null | undefined): string | null => (v && v.trim() ? v : null);

/** The user's own arguments set a KV cache type (`kv_policy::custom_args_set_cache_type`). */
export function customArgsSetCacheType(custom: string | null | undefined): boolean {
  return (custom ?? '').split(/\s+/).some((t) => ['-ctk', '--cache-type-k', '-ctv', '--cache-type-v'].includes(t.split('=')[0]));
}

/** The user's own arguments turn flash attention off (`kv_policy::custom_args_disable_flash_attn`). */
export function customArgsDisableFlashAttn(custom: string | null | undefined): boolean {
  const tokens = (custom ?? '').split(/\s+/).filter(Boolean);
  return tokens.some((t, i) => {
    const [flag, inline] = t.includes('=') ? t.split('=', 2) : [t, undefined];
    const value = inline ?? tokens[i + 1];
    return (flag === '-fa' || flag === '--flash-attn') && !!value && ['off', '0', 'false'].includes(value.toLowerCase());
  });
}

/**
 * The launch's GPU part exceeds the dedicated VRAM: Windows drivers then spill the excess into
 * shared system memory (no OOM, but slow and it can fill RAM). Not applicable to integrated
 * GPUs / unified memory (same pool), CPU-only (`-ngl 0`) or an unknown budget.
 */
export function vramSpillRisk(fit: HardwareFitResult | null, gpuLayers: number): boolean {
  return (
    !!fit &&
    gpuLayers !== 0 &&
    fit.category === 'oom_risk' &&
    !fit.sharedIgpu &&
    !fit.unifiedMemory &&
    fit.vramBudgetBytes > 0 &&
    fit.requiredVramBytes > fit.vramBudgetBytes
  );
}

/** Shared GPU memory above this is a real spill, not host-visible staging (matches the backend's 256 MiB rule). */
export const SPILL_SHARED_MIN_BYTES = 256 * 1024 * 1024;

export interface SpillWarning {
  /** The running server moved GPU memory into system memory: replies are several times slower. */
  show: boolean;
  /** One click can help: the KV cache is not q8_0 yet and the user's own arguments do not fix its type. */
  canReloadQ8: boolean;
}

/**
 * Warning for a running server after load (the backend measured Windows' `GPU Process Memory`
 * counters; on Linux/macOS nothing is measured and `shared_memory_bytes` stays empty).
 */
export function spillWarning(
  values: { fit_failed?: boolean; shared_memory_slowdown?: boolean; shared_memory_bytes?: number | null } | null,
  kv: { k: string | null | undefined; v: string | null | undefined; customArgs?: string | null },
): SpillWarning {
  const shared = values?.shared_memory_bytes ?? 0;
  const show = !!values && shared >= SPILL_SHARED_MIN_BYTES && !!(values.fit_failed || values.shared_memory_slowdown);
  const alreadyQ8 = kv.k === 'q8_0' && kv.v === 'q8_0';
  return { show, canReloadQ8: show && !alreadyQ8 && !customArgsSetCacheType(kv.customArgs) };
}
