// Playground model picker, "Load Settings": the editable subset of a definition's load
// settings (context, GPU layers, KV cache type, flash attention, parallel slots) as a form,
// and the one-time override layer it becomes (`load_definition` `overrides`, merged by the
// backend over the definition: `domain::launch`). Pure functions; no Tauri or store access.
import type { ParamValue, ParamValues } from '$lib/types';

export type UiLevel = 'simple' | 'advanced';
/** `auto` = the launch decides (KV: f16, smaller types only when the context does not fit);
 *  `definition` = the definition carries a mixed K/V pair the picker does not edit. */
export type KvChoice = 'auto' | 'f16' | 'q8_0' | 'q4_0' | 'definition';
export type LoadField = 'context' | 'gpuLayers' | 'kv' | 'flashAttn' | 'slots';

export interface LoadForm {
  /** Digits or '' (automatic: the model's trained context, at most 32768). */
  context: string;
  /** Digits or '' (automatic: the VRAM fit recommends the layer count). */
  gpuLayers: string;
  kv: KvChoice;
  flashAttn: boolean;
  /** Digits or '' (automatic). */
  slots: string;
}

/** KV types the picker offers, in the order of the Dashboard's selector. */
export const KV_CHOICES = ['f16', 'q8_0', 'q4_0'] as const;

const ALL_FIELDS: LoadField[] = ['context', 'gpuLayers', 'kv', 'flashAttn', 'slots'];

/** Fields shown at a UI level: the Simple level only edits the context. */
export function visibleFields(level: UiLevel): LoadField[] {
  return level === 'advanced' ? ALL_FIELDS : ['context'];
}

/** A whole number >= 0 from a form text; '' = automatic (null); anything else = invalid (undefined). */
export function parseCount(text: string): number | null | undefined {
  const s = text.trim();
  if (s === '') return null;
  if (!/^\d+$/.test(s)) return undefined;
  const n = Number(s);
  return Number.isSafeInteger(n) && n <= 0xffffffff ? n : undefined;
}

function countText(v: ParamValue | undefined): string {
  if (v === undefined || v === '') return '';
  const n = typeof v === 'number' ? v : typeof v === 'string' ? Number(v) : NaN;
  return Number.isFinite(n) ? String(n) : '';
}

function kvText(v: ParamValue | undefined): string {
  return typeof v === 'string' ? v.trim() : '';
}

/** The form for a definition: its `load` settings, absent ones are automatic. */
export function formFromDefinition(load: ParamValues): LoadForm {
  const context = countText(load.context_size);
  const k = kvText(load.cache_type_k);
  const v = kvText(load.cache_type_v);
  let kv: KvChoice;
  if (!k && !v) kv = 'auto';
  else if ((k || 'f16') === (v || 'f16') && (KV_CHOICES as readonly string[]).includes(k || 'f16')) kv = (k || 'f16') as KvChoice;
  else kv = 'definition';
  return {
    context: context === '0' ? '' : context,
    gpuLayers: countText(load.gpu_layers),
    kv,
    flashAttn: load.flash_attn === true,
    slots: countText(load.parallel_slots),
  };
}

/** Every visible field parses (an automatic one is valid). */
export function formValid(form: LoadForm, level: UiLevel): boolean {
  const text: Record<'context' | 'gpuLayers' | 'slots', string> = {
    context: form.context,
    gpuLayers: form.gpuLayers,
    slots: form.slots,
  };
  return visibleFields(level).every((f) => {
    if (f !== 'context' && f !== 'gpuLayers' && f !== 'slots') return true;
    const n = parseCount(text[f]);
    return n !== undefined && (f !== 'slots' || n !== 0);
  });
}

export function sameForm(a: LoadForm, b: LoadForm): boolean {
  return a.context.trim() === b.context.trim()
    && a.gpuLayers.trim() === b.gpuLayers.trim()
    && a.kv === b.kv
    && a.flashAttn === b.flashAttn
    && a.slots.trim() === b.slots.trim();
}

/**
 * The override layer for `form` against the definition's own form `base`: only what the user
 * changed (the rest stays the definition's), only fields the level shows. An automatic value
 * is sent as its reset marker (context 0, '' for the others) so it also undoes a value the
 * definition sets. Empty = nothing to override.
 */
export function overridesFromForm(form: LoadForm, base: LoadForm, level: UiLevel): ParamValues {
  const out: ParamValues = {};
  const shown = visibleFields(level);
  if (shown.includes('context') && form.context.trim() !== base.context.trim()) {
    out.context_size = parseCount(form.context) ?? 0;
  }
  if (shown.includes('gpuLayers') && form.gpuLayers.trim() !== base.gpuLayers.trim()) {
    out.gpu_layers = parseCount(form.gpuLayers) ?? '';
  }
  if (shown.includes('kv') && form.kv !== base.kv && form.kv !== 'definition') {
    const type = form.kv === 'auto' ? '' : form.kv;
    out.cache_type_k = type;
    out.cache_type_v = type;
  }
  if (shown.includes('flashAttn') && form.flashAttn !== base.flashAttn) {
    out.flash_attn = form.flashAttn;
  }
  if (shown.includes('slots') && form.slots.trim() !== base.slots.trim()) {
    out.parallel_slots = parseCount(form.slots) ?? '';
  }
  return out;
}

/** What a running server was started with (`get_server_launch_config`). */
export interface RunningLoad {
  context_size: number;
  gpu_layers: number;
  cache_type_k?: string | null;
  cache_type_v?: string | null;
  flash_attn?: boolean;
  parallel_slots?: number | null;
}

/**
 * The model runs with settings other than the form's explicit ones (offer "Reload"). An
 * automatic field has no value to compare, so it never counts as a difference.
 */
export function differsFromRunning(form: LoadForm, running: RunningLoad | null, level: UiLevel): boolean {
  if (!running) return false;
  const shown = visibleFields(level);
  const ctx = parseCount(form.context);
  if (shown.includes('context') && ctx != null && ctx !== running.context_size) return true;
  const ngl = parseCount(form.gpuLayers);
  if (shown.includes('gpuLayers') && ngl != null && ngl !== running.gpu_layers) return true;
  if (shown.includes('kv') && form.kv !== 'auto' && form.kv !== 'definition') {
    if ((running.cache_type_k || 'f16') !== form.kv || (running.cache_type_v || 'f16') !== form.kv) return true;
  }
  if (shown.includes('flashAttn') && form.flashAttn && !running.flash_attn) return true;
  const slots = parseCount(form.slots);
  if (shown.includes('slots') && slots != null && slots !== (running.parallel_slots ?? null)) return true;
  return false;
}

/** KV choices the runtime lists (all of them while its capabilities are unknown). */
export function kvOptions(allowed: string[]): (typeof KV_CHOICES)[number][] {
  const list = KV_CHOICES.filter((k) => allowed.includes(k));
  return allowed.length && list.length ? list : [...KV_CHOICES];
}
