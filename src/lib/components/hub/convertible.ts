import type { HfModelSearchResult } from '#lib/types.js';

/** Hub tasks whose safetensors repos the GGUF converter can take (text and image chat models). */
const CONVERTIBLE_TASKS = ['text-generation', 'image-text-to-text'];

/**
 * A safetensors chat model: the Hub offers "Convert to GGUF" next to (or instead of) running the
 * checkpoint as it is. mistral.rs runs safetensors too, so such a repo is reported as needing
 * (or runnable with) mistral.rs, or with the Python engine when that is not registered; the GGUF
 * route stays offered, since llama.cpp is the runtime
 * every install has. The architecture is only checked when the dialog opens (the converter's own
 * list), so this is a cheap, listing-only test.
 */
export function canConvert(
  repo: Pick<HfModelSearchResult, 'compatibility' | 'pipeline_tag' | 'inferred_task' | 'format'> | null | undefined
): boolean {
  if (!repo) return false;
  const c = repo.compatibility;
  const safetensors =
    c.status === 'unsupported' ? c.format === 'safetensors' : (c.runtime === 'mistral.rs' || c.runtime === 'python-hf') && repo.format === 'safetensors';
  if (!safetensors) return false;
  const task = repo.pipeline_tag ?? repo.inferred_task ?? null;
  return task !== null && CONVERTIBLE_TASKS.includes(task);
}

/** Source speech checkpoints need a supported exported package before Harness can serve them. */
export function speechNeedsConversion(
  repo: Pick<HfModelSearchResult, 'compatibility' | 'pipeline_tag' | 'inferred_task' | 'format'> | null | undefined,
): boolean {
  return !!repo && repo.compatibility.status === 'unsupported'
    && (repo.pipeline_tag ?? repo.inferred_task) === 'automatic-speech-recognition'
    && ['safetensors', 'pytorch'].includes(repo.format);
}
