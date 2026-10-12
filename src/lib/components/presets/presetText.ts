import { t } from '$lib/i18n.svelte';
import type { InferencePreset } from '$lib/types';
import { presetDescriptionWith, presetNameWith } from './names';

/** Preset name in the current UI language (built-ins are translated by id). */
export function presetName(p: Pick<InferencePreset, 'id' | 'name' | 'builtin'>): string {
  return presetNameWith(p, t as never);
}

/** Preset description in the current UI language. */
export function presetDescription(p: Pick<InferencePreset, 'id' | 'name' | 'builtin' | 'description'>): string {
  return presetDescriptionWith(p, t as never);
}
