// Installs the managed llama.cpp runtime: the build recommended for this machine first (ROCm/HIP
// or SYCL on some GPUs depend on the local driver stack; when the staged build sees no device,
// the portable Vulkan build is used), then verified, staged and activated. Used by the Setup
// window; the same commands back Settings > Runtime.
import { invoke } from '#lib/ipc.js';
import { rawError } from '#lib/errors.js';
import { downloadRelease } from '#lib/stores/security.svelte.js';
import {
  BACKEND_TOKEN,
  missingBackendLibraries,
  normBackend,
  saveBackendFallback,
  type BackendFallbackNote,
} from '#lib/runtimeBackend.js';
import type { BackendStatus, ReleaseAssetInfo, RuntimeDownloadProgress, SystemHardwareInfo } from '#lib/types.js';

export type RuntimePhase =
  | { kind: 'checking' }
  | { kind: 'downloading'; version: string; backend: string | null }
  | { kind: 'fallback'; reason: 'unavailable' | 'no_device'; recommended: string | null; installed: string | null }
  | { kind: 'activating'; version: string };

export interface RuntimeInstallHooks {
  onPhase?: (phase: RuntimePhase) => void;
  onProgress?: (progress: RuntimeDownloadProgress | null) => void;
}

export interface RuntimeInstallResult {
  version: string;
  backend: string | null;
  fallback: BackendFallbackNote | null;
}

/** The release the recommended backend maps to, with its download size (nothing is downloaded). */
export async function previewManagedRuntime(): Promise<ReleaseAssetInfo> {
  return invoke<ReleaseAssetInfo>('check_runtime_update', { backend: null });
}

/** Resolves with the installed build; null when the user declined an unverified archive. */
export async function installManagedRuntime(
  hardware: SystemHardwareInfo | null,
  hooks: RuntimeInstallHooks = {},
): Promise<RuntimeInstallResult | null> {
  hooks.onPhase?.({ kind: 'checking' });
  let release = await previewManagedRuntime();
  // The recommendation, to tell later (Settings > Runtime) why another build got installed.
  const recommended = await invoke<BackendStatus>('get_backend_status')
    .then((s) => BACKEND_TOKEN[s.advice.recommended])
    .catch(() => BACKEND_TOKEN[hardware?.recommended_backend ?? 'Cpu']);
  let reason: BackendFallbackNote['reason'] | null =
    recommended && normBackend(release.backend) !== recommended ? 'unavailable' : null;
  let missingLibraries: string | undefined;
  const note = (): BackendFallbackNote | null => {
    const installed = normBackend(release.backend);
    return recommended && reason && installed
      ? { recommended, installed, version: release.tag_name, reason, missingLibraries }
      : null;
  };

  const timer = setInterval(async () => {
    try {
      hooks.onProgress?.(await invoke<RuntimeDownloadProgress | null>('get_runtime_download_progress'));
    } catch {
      // progress is best-effort
    }
  }, 500);
  try {
    let staged: string | null;
    try {
      if (reason) hooks.onPhase?.({ kind: 'fallback', reason, recommended, installed: normBackend(release.backend) });
      else hooks.onPhase?.({ kind: 'downloading', version: release.tag_name, backend: release.backend ?? null });
      staged = await downloadRelease(release);
    } catch (e) {
      if (!rawError(e).includes('runtime.backend_no_device') || release.backend === 'vulkan') throw e;
      missingLibraries = missingBackendLibraries(e);
      const attempted = normBackend(release.backend);
      release = await invoke<ReleaseAssetInfo>('check_runtime_update', { backend: 'vulkan' });
      reason = 'no_device';
      hooks.onPhase?.({ kind: 'fallback', reason, recommended: attempted, installed: normBackend(release.backend) });
      staged = await downloadRelease(release);
    }
    if (staged === null) return null;
    hooks.onPhase?.({ kind: 'activating', version: release.tag_name });
    await invoke('activate_managed_runtime', { version: release.tag_name });
    const fallback = note();
    saveBackendFallback(fallback);
    return { version: release.tag_name, backend: release.backend ?? null, fallback };
  } finally {
    clearInterval(timer);
    hooks.onProgress?.(null);
  }
}
