// Display texts of inference presets. The built-ins keep stable English names and
// descriptions in harness-core (CLI/API); the GUI shows them from locale keys by id.

import type { InferencePreset } from '#lib/types.js';

type Translate = (key: never) => string;
type PresetLike = Pick<InferencePreset, 'id' | 'name' | 'builtin'> & { description?: string };

const BUILTIN_KEYS = new Set(['default', 'coding', 'reasoning', 'creative', 'deterministic']);

function builtinKey(p: PresetLike): string | null {
  if (!p.builtin || !p.id.startsWith('builtin:')) return null;
  const key = p.id.slice('builtin:'.length);
  return BUILTIN_KEYS.has(key) ? key : null;
}

/** Localized name for a built-in preset (by id); the stored name for user presets. */
export function presetNameWith(p: PresetLike, tr: Translate): string {
  const key = builtinKey(p);
  return key ? tr(`ip.preset_name_${key}` as never) : p.name;
}

/** Localized description for a built-in preset (by id); the stored one for user presets. */
export function presetDescriptionWith(p: PresetLike, tr: Translate): string {
  const key = builtinKey(p);
  return key ? tr(`ip.preset_desc_${key}` as never) : (p.description ?? '');
}
