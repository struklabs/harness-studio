import { invoke as tauriInvoke, type InvokeArgs, type InvokeOptions } from '@tauri-apps/api/core';
import { BackendError } from '#lib/errors.js';

/**
 * Tauri's `invoke`, but a string rejection (a backend error key) becomes a `BackendError`
 * whose message is in the current UI language. Import `invoke` from here, not from
 * `@tauri-apps/api/core` (Tauri's internals object is read-only, so it can't be patched globally).
 */
export function invoke<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  return tauriInvoke<T>(cmd, args, options).catch((e: unknown) => {
    throw typeof e === 'string' ? new BackendError(e) : e;
  });
}
