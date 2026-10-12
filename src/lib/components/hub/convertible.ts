import type { HfModelSearchResult } from '$lib/types';

/** Hub tasks whose safetensors repos the GGUF converter can take (text and image chat models). */
const CONVERTIBLE_TASKS = ['text-generation', 'image-text-to-text'];

/**
 * A safetensors chat model: the Hub offers "Convert to GGUF" next to (or instead of) running the
 * checkpoint as it is. mistral.rs runs safetensors too, so such a repo is reported as needing
 * (or runnable with) mistral.rs; the GGUF route stays offered, since llama.cpp is the runtime
 * every install has. The architecture is only checked when the dialog opens (the converter's own
 * list), so this is a cheap, listing-only test.
 */
export function canConvert(
  repo: Pick<HfModelSearchResult, 'compatibility' | 'pipeline_tag' | 'inferred_task' | 'format'> | null | undefined
): boolean {
  if (!repo) return false;
  const c = repo.compatibility;
  const safetensors =
    c.status === 'unsupported' ? c.format === 'safetensors' : c.runtime === 'mistral.rs' && repo.format === 'safetensors';
  if (!safetensors) return false;
  const task = repo.pipeline_tag ?? repo.inferred_task ?? null;
  return task !== null && CONVERTIBLE_TASKS.includes(task);
}
