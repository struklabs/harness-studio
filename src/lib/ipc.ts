import { invoke as tauriInvoke, type InvokeArgs, type InvokeOptions } from '@tauri-apps/api/core';
import { BackendError } from '#lib/errors.js';
import { createInflight, inflightKey, isDefinitionWrite } from '#lib/inflight.js';

/**
 * Tauri's `invoke`, but a string rejection (a backend error key) becomes a `BackendError`
 * whose message is in the current UI language. Import `invoke` from here, not from
 * `@tauri-apps/api/core` (Tauri's internals object is read-only, so it can't be patched globally).
 */
export function invoke<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  const call = tauriInvoke<T>(cmd, args, options).catch((e: unknown) => {
    throw typeof e === 'string' ? new BackendError(e) : e;
  });
  if (!isDefinitionWrite(cmd)) return call;
  // Bumped on start and on settle, so a read during or after the write never joins an older one.
  definitionWrites++;
  return call.finally(() => definitionWrites++);
}

let definitionWrites = 0;

/** Epoch for shared definition reads (`get_definition_form`): changes whenever a definition is written. */
export function definitionsEpoch(): number {
  return definitionWrites;
}

const sharedInflight = createInflight();

/**
 * `invoke` for read-only commands several components ask for at once (boot): concurrent calls
 * with the same arguments share one backend call. Each caller gets its own copy of the result,
 * so one component mutating it cannot affect another. `epoch` separates calls that must not
 * share a result (e.g. a library version that changed while a scan was running).
 */
export function invokeShared<T>(cmd: string, args?: InvokeArgs, epoch?: string | number): Promise<T> {
  return sharedInflight(inflightKey(cmd, args, epoch), () => invoke<T>(cmd, args)).then((v) => structuredClone(v));
}
