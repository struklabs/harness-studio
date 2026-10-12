// Backend tokens of the managed llama.cpp build and the note the setup wizard leaves when it
// installs a different build than the recommended one (so Settings can say why).
import type { RecommendedBackend } from '#lib/types.js';
import { errorCode, errorText, rawError } from '#lib/errors.js';
import { t } from '#lib/i18n.svelte.js';

export const BACKEND_TOKEN: Record<RecommendedBackend, string> = {
  Cuda12: 'cuda',
  Vulkan: 'vulkan',
  Metal: 'metal',
  Cpu: 'cpu',
  Rocm: 'rocm',
  Sycl: 'sycl',
};

const BACKEND_NAME: Record<string, string> = {
  cuda: 'CUDA',
  vulkan: 'Vulkan',
  metal: 'Metal',
  cpu: 'CPU',
  rocm: 'ROCm/HIP',
  sycl: 'SYCL',
};

/** `sycl-fp16` (Linux asset) and `sycl` are the same backend. */
export const normBackend = (b: string | null | undefined): string | null =>
  b?.startsWith('sycl') ? 'sycl' : (b ?? null);

export const backendName = (b: string | null | undefined): string =>
  b ? (BACKEND_NAME[normBackend(b)!] ?? b) : '?';

/**
 * Why the installed build differs from the recommended one:
 * `no_device` = the recommended build was downloaded but saw no GPU (driver/ROCm/oneAPI stack);
 * `unavailable` = the release has no build of the recommended backend.
 */
export interface BackendFallbackNote {
  recommended: string;
  installed: string;
  version: string;
  reason: 'no_device' | 'unavailable';
  missingLibraries?: string;
}

const FALLBACK_KEY = 'harness_runtime_backend_fallback';

export function saveBackendFallback(note: BackendFallbackNote | null) {
  try {
    if (note) localStorage.setItem(FALLBACK_KEY, JSON.stringify(note));
    else localStorage.removeItem(FALLBACK_KEY);
  } catch {
    // storage unavailable: Settings shows the general explanation instead
  }
}

export function loadBackendFallback(): BackendFallbackNote | null {
  try {
    const raw = localStorage.getItem(FALLBACK_KEY);
    return raw ? (JSON.parse(raw) as BackendFallbackNote) : null;
  } catch {
    return null;
  }
}

/**
 * Text of an install error. `runtime.backend_no_device` carries the DLLs the backend library
 * could not find (`rocm (missing: hipblas.dll)`); they are named in a sentence, with the HIP
 * SDK the official Windows ROCm build needs, instead of the raw suffix.
 */
export function runtimeInstallErrorText(e: unknown): string {
  if (errorCode(e) !== 'runtime.backend_no_device') return errorText(e);
  const text = t('runtime.backend_no_device');
  const missing = missingBackendLibraries(e);
  if (!missing) return text;
  return `${text} ${t(/hip/i.test(missing) ? 'runtime.backend_missing_hip' : 'runtime.backend_missing_libs', { libs: missing })}`;
}

/** Missing dependencies reported by the staged backend probe. */
export function missingBackendLibraries(e: unknown): string | undefined {
  return /\(missing: ([^)]+)\)/.exec(rawError(e))?.[1];
}

export function backendFallbackText(note: BackendFallbackNote, compact = false): string {
  const key = compact
    ? note.reason === 'no_device' ? 'onboarding.wizard_fallback_no_device' : 'onboarding.wizard_fallback_unavailable'
    : note.reason === 'no_device' ? 'settings.backend_fallback_no_device' : 'settings.backend_fallback_unavailable';
  const text = t(key, {
    recommended: backendName(note.recommended), installed: backendName(note.installed), version: note.version,
  });
  return note.missingLibraries
    ? `${text} ${t(/hip/i.test(note.missingLibraries) ? 'runtime.backend_missing_hip' : 'runtime.backend_missing_libs', { libs: note.missingLibraries })}`
    : text;
}
