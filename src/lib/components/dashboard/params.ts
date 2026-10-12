import type { AppSettings, LocalModel, ModelPreset, ParamValue, ParamValues } from '#lib/types.js';

/** UI tri-state for `--context-shift` / `--no-context-shift` (null in the preset = 'default'). */
export type ContextShiftMode = 'default' | 'enabled' | 'disabled';

/** Text fields that are `string | null` in the preset but plain strings ('' = unset) in the form. */
type TextField =
  | 'cache_type_k'
  | 'cache_type_v'
  | 'split_mode'
  | 'tensor_split'
  | 'rope_scaling'
  | 'chat_template'
  | 'api_key'
  | 'custom_args'
  | 'draft_model_path'
  | 'lora_path'
  | 'mmproj_path'
  | 'runtime';

/**
 * Editable launch parameters: the ModelPreset shape with every field present,
 * text fields as strings (bindable to inputs/selects) and context_shift as a tri-state.
 */
export type LaunchParamsForm = Omit<Required<ModelPreset>, TextField | 'context_shift' | 'api_key_ref' | 'params'> &
  Record<TextField, string> & {
    context_shift: ContextShiftMode;
    /** Keychain reference of the saved API key; set to null to remove the saved key. */
    api_key_ref: string | null;
    /** Catalog parameters (`ModelPreset.params`); absent id = server default. */
    catalog_params: ParamValues;
  };

/** Values shown before any model (and so any preset) has been selected. */
export function initialParams(): LaunchParamsForm {
  return buildParams(null, 33, 4096);
}

/** Builds the form from a saved preset, or defaults (for every field) when there is none. */
export function paramsFromPreset(
  preset: ModelPreset | null,
  model: LocalModel | undefined,
  settings: AppSettings
): LaunchParamsForm {
  const defaultCtx = settings.server.default_ctx || 32768; // 0 = automatic: the trained context, at most 32768
  return buildParams(
    preset,
    settings.server.default_ngl || 33,
    model?.context_length ? Math.min(Number(model.context_length), defaultCtx) : defaultCtx
  );
}

function buildParams(preset: ModelPreset | null, defaultNgl: number, defaultCtx: number): LaunchParamsForm {
  return {
    gpu_layers: preset?.gpu_layers ?? defaultNgl,
    context_size: preset?.context_size ?? defaultCtx,
    threads: preset?.threads ?? null,
    flash_attn: preset?.flash_attn ?? false,
    cache_type_k: preset?.cache_type_k ?? '',
    cache_type_v: preset?.cache_type_v ?? '',
    parallel_slots: preset?.parallel_slots ?? null,
    logical_batch: preset?.logical_batch ?? null,
    physical_batch: preset?.physical_batch ?? null,
    context_shift:
      preset?.context_shift === undefined || preset?.context_shift === null
        ? 'default'
        : preset.context_shift
          ? 'enabled'
          : 'disabled',
    split_mode: preset?.split_mode ?? '',
    main_gpu: preset?.main_gpu ?? null,
    tensor_split: preset?.tensor_split ?? '',
    rope_scaling: preset?.rope_scaling ?? '',
    rope_freq_base: preset?.rope_freq_base ?? null,
    rope_freq_scale: preset?.rope_freq_scale ?? null,
    chat_template: preset?.chat_template ?? '',
    api_key: '',
    api_key_ref: preset?.api_key_ref ?? null,
    custom_args: preset?.custom_args ?? '',
    draft_model_path: preset?.draft_model_path ?? '',
    draft_gpu_layers: preset?.draft_gpu_layers ?? null,
    lora_path: preset?.lora_path ?? '',
    lora_scale: preset?.lora_scale ?? null,
    mmproj_path: preset?.mmproj_path ?? '',
    mcp_enabled: preset?.mcp_enabled ?? false,
    catalog_params: { ...(preset?.params ?? {}) },
    runtime: preset?.runtime ?? '',
  };
}

/** A catalog value that means "not set" (server default). */
export function isUnsetParam(v: ParamValue | null | undefined): boolean {
  if (v === null || v === undefined) return true;
  if (typeof v === 'string') return v.trim() === '';
  if (typeof v === 'number') return !Number.isFinite(v);
  if (Array.isArray(v)) return v.every((x) => x.trim() === '');
  return false;
}

/** Drops unset values so presets only store what the user chose. */
export function cleanParams(values: ParamValues): ParamValues {
  const out: ParamValues = {};
  for (const [id, v] of Object.entries(values)) {
    if (isUnsetParam(v)) continue;
    out[id] = Array.isArray(v) ? v.map((x) => x.trim()).filter(Boolean) : v;
  }
  return out;
}

/** Same rule as the backend's `network::check_exposure`: llama-server's MCP on a non-loopback host needs an API key. */
export function toolsExposureRisk(
  host: string,
  apiKey: string,
  p: Pick<LaunchParamsForm, 'mcp_enabled' | 'api_key_ref'>
): boolean {
  const usesTools = p.mcp_enabled;
  const loopback = ['127.0.0.1', 'localhost', '::1', '[::1]'].includes(host.trim());
  return usesTools && !loopback && !apiKey.trim() && !p.api_key_ref;
}

/** Converts the form back to a preset (empty strings become null). */
export function presetFromParams(p: LaunchParamsForm): ModelPreset {
  return {
    gpu_layers: p.gpu_layers,
    context_size: p.context_size,
    threads: p.threads,
    flash_attn: p.flash_attn,
    cache_type_k: p.cache_type_k || null,
    cache_type_v: p.cache_type_v || null,
    parallel_slots: p.parallel_slots,
    logical_batch: p.logical_batch,
    physical_batch: p.physical_batch,
    context_shift: p.context_shift === 'enabled' ? true : p.context_shift === 'disabled' ? false : null,
    split_mode: p.split_mode || null,
    main_gpu: p.main_gpu,
    tensor_split: p.tensor_split || null,
    rope_scaling: p.rope_scaling || null,
    rope_freq_base: p.rope_freq_base,
    rope_freq_scale: p.rope_freq_scale,
    chat_template: p.chat_template || null,
    // New key when typed; null keeps the saved one; '' (reference removed) clears it.
    api_key: p.api_key || (p.api_key_ref ? null : ''),
    api_key_ref: p.api_key_ref,
    custom_args: p.custom_args || null,
    draft_model_path: p.draft_model_path || null,
    draft_gpu_layers: p.draft_gpu_layers,
    lora_path: p.lora_path || null,
    lora_scale: p.lora_scale,
    mmproj_path: p.mmproj_path || null,
    mcp_enabled: p.mcp_enabled,
    params: cleanParams(p.catalog_params),
    runtime: p.runtime && p.runtime !== 'official' ? p.runtime : null,
  };
}
