import type { HfCompatibility } from '#lib/types.js';

/** Cached search results can predate an engine install. */
export function engineCompatibility(compatibility: HfCompatibility, installed: (family: string) => boolean): HfCompatibility {
  return compatibility.status === 'needs_runtime' && installed(compatibility.runtime)
    ? { status: 'runnable', runtime: compatibility.runtime }
    : compatibility;
}
