/**
 * Shares one in-flight promise between callers that ask for the same thing. Boot mounts several
 * components that each want the same read (model scan, storage directories, feature status...):
 * with this they cost one backend call. The entry is dropped when the call settles, so a later
 * call always reads fresh data.
 */
export function createInflight() {
  const pending = new Map<string, Promise<unknown>>();
  return function shared<T>(key: string, run: () => Promise<T>): Promise<T> {
    const running = pending.get(key);
    if (running) return running as Promise<T>;
    const promise = run().finally(() => {
      if (pending.get(key) === promise) pending.delete(key);
    });
    pending.set(key, promise);
    return promise;
  };
}

/** Stable cache key for a command and its (JSON) arguments. */
export function inflightKey(cmd: string, args?: unknown, epoch?: string | number): string {
  return `${cmd}|${args === undefined ? '' : JSON.stringify(args)}|${epoch ?? ''}`;
}

/** Commands that change model definitions. A definition read must not join one started before them. */
const DEFINITION_WRITES = new Set([
  'save_definition_form',
  'save_definition_load',
  'save_model_definition',
  'delete_model_definition',
  'duplicate_model_definition',
  'import_model_definition',
  'ensure_library_definitions',
  'sync_image_definitions',
]);

export function isDefinitionWrite(cmd: string): boolean {
  return DEFINITION_WRITES.has(cmd);
}
