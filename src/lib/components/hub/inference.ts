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
  foreign_speech_gguf: 'hub.not_runnable_foreign_speech_gguf',
  no_gguf: 'hub.not_runnable_no_gguf',
  vibevoice_layout: 'hub.vibevoice_layout',
  audio_cpp_incomplete_package: 'hub.audio_cpp_incomplete_package',
  sdxl_refiner_folder_required: 'hub.sdxl_refiner_folder_required',
  needs_sea_g2p: 'hub.not_runnable_needs_sea_g2p',
  needs_librosa: 'hub.not_runnable_needs_librosa',
  adapter: 'hub.not_runnable_adapter',
  crispasr_pocket_english_only: 'hub.crispasr_pocket_english_only',
  audio_cpp_espeak_missing: 'hub.audio_cpp_espeak_missing',
  audio_cpp_unsupported: 'hub.not_runnable_audio_cpp',
  not_language_model: 'hub.not_runnable_non_llm',
  needs_own_package: 'hub.not_runnable_needs_own_package',
  unsupported_architecture: 'hub.not_runnable_unsupported_architecture',
  safetensors_only: 'hub.not_runnable_safetensors_only',
  ctranslate2_export: 'hub.not_runnable_ctranslate2',
  apple_only: 'hub.not_runnable_apple_only',
  pipeline_config_only: 'hub.not_runnable_pipeline_config',
  component_only: 'hub.not_runnable_component_only'
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

/** Short card labels of the precise refusals; the full sentence stays in the tooltip and drawer. */
const SHORT_REFUSAL_KEYS: Partial<Record<HfNotRunnable, TKey>> = {
  foreign_speech_gguf: 'hub.refusal_short_foreign_speech_gguf',
  needs_own_package: 'hub.refusal_short_needs_own_package',
  unsupported_architecture: 'hub.refusal_short_unsupported_architecture',
  safetensors_only: 'hub.refusal_short_safetensors_only',
  ctranslate2_export: 'hub.refusal_short_ctranslate2_export',
  apple_only: 'hub.refusal_short_apple_only',
  pipeline_config_only: 'hub.refusal_short_pipeline_config_only',
  component_only: 'hub.refusal_short_component_only',
  vibevoice_layout: 'hub.refusal_short_vibevoice_layout',
  audio_cpp_incomplete_package: 'hub.refusal_short_audio_cpp_incomplete_package',
  sdxl_refiner_folder_required: 'hub.refusal_short_sdxl_refiner_folder_required',
  needs_librosa: 'hub.refusal_short_needs_librosa',
  needs_sea_g2p: 'hub.refusal_short_needs_sea_g2p'
};

/** The card's short label for a refusal (the full text when there is none). */
export function notRunnableShortKey(reason: HfNotRunnable): TKey {
  return SHORT_REFUSAL_KEYS[reason] ?? NOT_RUNNABLE_KEYS[reason];
}

const PRECISE_REFUSALS: ReadonlySet<HfNotRunnable> = new Set<HfNotRunnable>([
  'foreign_speech_gguf',
  'needs_own_package',
  'unsupported_architecture',
  'safetensors_only',
  'ctranslate2_export',
  'apple_only',
  'pipeline_config_only',
  'component_only',
  'vibevoice_layout',
  'audio_cpp_incomplete_package',
  'sdxl_refiner_folder_required',
  'needs_librosa',
  'needs_sea_g2p'
]);

/** A refusal that already says why (and what to do), so it replaces the generic format line. */
export function isPreciseRefusal(reason: HfNotRunnable | null | undefined): reason is HfNotRunnable {
  return !!reason && PRECISE_REFUSALS.has(reason);
}
