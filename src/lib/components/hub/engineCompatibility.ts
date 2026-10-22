import type { HfCompatibility } from '#lib/types.js';

/** Cached search results can predate an engine install. */
export function engineCompatibility(compatibility: HfCompatibility, installed: (family: string) => boolean): HfCompatibility {
  return compatibility.status === 'needs_runtime' && installed(compatibility.runtime)
    ? { status: 'runnable', runtime: compatibility.runtime }
    : compatibility;
}

/** A model's engine verdict takes precedence over the general task baseline. */
export function capabilitiesBlocked(compatibility?: HfCompatibility | null, task?: { baseline: string; runtime_missing: boolean } | null): boolean {
  if (compatibility) return compatibility.status === 'unsupported' || compatibility.status === 'unsupported_platform';
  return task?.baseline === 'no' || task?.runtime_missing === true;
}
