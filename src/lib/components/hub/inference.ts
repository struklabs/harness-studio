// Locale keys for the backend's task inference (`harness_core::hub_tasks`): where a guessed task
// came from, and why a repo cannot run at all.
import { t, type TKey } from '#lib/i18n.svelte.js';
import type { HfModelFormat, HfNotRunnable, HfTaskSource } from '#lib/types.js';

const SOURCE_KEYS: Record<HfTaskSource, TKey> = {
  tags: 'hub.task_source_tags',
  name: 'hub.task_source_name',
  files: 'hub.task_source_files',
  library: 'hub.task_source_library',
  gguf: 'hub.task_source_gguf'
};

const NOT_RUNNABLE_KEYS: Record<HfNotRunnable, TKey> = {
  no_gguf: 'hub.not_runnable_no_gguf',
  adapter: 'hub.not_runnable_adapter',
  audio_cpp_unsupported: 'hub.not_runnable_audio_cpp',
  not_language_model: 'hub.not_runnable_non_llm'
};

/** Tooltip key for "Görev: … (tahmin)"; the generic one when the source is unknown. */
export function taskSourceKey(source: HfTaskSource | null | undefined): TKey {
  return (source && SOURCE_KEYS[source]) || 'hub.card_task_inferred_title';
}

const FORMAT_NAMES: Record<Exclude<HfModelFormat, 'unknown'>, string> = {
  gguf: 'GGUF',
  whisper_ggml: 'whisper ggml',
  safetensors: 'safetensors',
  onnx: 'ONNX',
  diffusers: 'diffusers',
  pytorch: 'PyTorch',
  mlx: 'MLX',
  ctranslate2: 'CTranslate2',
  core_ml: 'Core ML'
};

/** Display name of a model format ("safetensors", "ONNX", ...). */
export function formatName(format: HfModelFormat): string {
  return format === 'unknown' ? t('hub.format_unknown') : FORMAT_NAMES[format];
}

export function notRunnableKey(reason: HfNotRunnable): TKey {
  return NOT_RUNNABLE_KEYS[reason];
}
