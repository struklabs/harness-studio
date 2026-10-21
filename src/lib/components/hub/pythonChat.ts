import type { HfModelSearchResult } from '#lib/types.js';

/** Memory torch and transformers take before the first tensor. */
export const PYTHON_CHAT_BASE_BYTES = 700 * 1024 * 1024;

/** Share of the free memory a model may take before loading it is called risky. */
export const PYTHON_CHAT_HEADROOM = 0.9;

/**
 * A safetensors chat repo that the Python engine (transformers) runs: no GGUF and no registered
 * mistral.rs path applies. A diffusers or task repo of the same engine is not one.
 */
export function isPythonChat(repo: Pick<HfModelSearchResult, 'format' | 'capabilities' | 'compatibility'>): boolean {
  return (
    repo.format === 'safetensors' &&
    'runtime' in repo.compatibility &&
    repo.compatibility.runtime === 'python-hf' &&
    !!repo.capabilities?.includes('chat')
  );
}

/**
 * Memory the Python engine needs for a chat checkpoint of `weightsBytes`: 16-bit weights are
 * loaded as 32-bit on a CPU without native bfloat16 (twice the size), plus the libraries'
 * own share. `fits` is null when the weights size or the free memory is unknown.
 */
export function pythonChatMemory(
  weightsBytes: number,
  availableRamBytes: number | null | undefined,
): { needBytes: number; fits: boolean | null } {
  const needBytes = weightsBytes > 0 ? weightsBytes * 2 + PYTHON_CHAT_BASE_BYTES : 0;
  if (weightsBytes <= 0 || !availableRamBytes) return { needBytes, fits: null };
  return { needBytes, fits: needBytes <= availableRamBytes * PYTHON_CHAT_HEADROOM };
}
