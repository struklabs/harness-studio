import type { FitCategory, HfModelVariant } from '#lib/types.js';

/** Leading number of a quant name ("Q4_K_M" -> 4, "IQ3_XS" -> 3, "BF16" -> 16). */
export function quantBits(q: string): number {
  const u = q.toUpperCase();
  if (/^(BF16|F16|FP16)/.test(u)) return 16;
  if (/^(F32|FP32)/.test(u)) return 32;
  const m = u.match(/(\d+)/);
  return m ? parseInt(m[1], 10) : 0;
}

const TIERS = ['full_gpu', 'tight', 'hybrid', 'cpu_heavy'] as const;

/**
 * The variant that suits this machine best, from the same fit estimate as the variant table
 * (`fits`: variant name -> category of the Rust estimate). Prefers the largest (best quality)
 * quantized file that fully fits in VRAM; when nothing fits fully, the smallest
 * 4-bit-or-better file of the best tier still possible. Null when sizes or fits are unknown.
 */
export function recommendVariant(variants: HfModelVariant[], fits: Map<string, FitCategory>): HfModelVariant | null {
  const sized = variants.filter((v) => v.total_size_bytes > 0 && quantBits(v.quantization) <= 8 && fits.has(v.name));
  if (sized.length === 0) return null;
  for (const tier of TIERS) {
    const group = sized.filter((v) => fits.get(v.name) === tier);
    if (group.length === 0) continue;
    if (tier === 'full_gpu' || tier === 'tight') {
      return group.reduce((a, b) => (b.total_size_bytes > a.total_size_bytes ? b : a));
    }
    const decent = group.filter((v) => quantBits(v.quantization) >= 4);
    const pool = decent.length ? decent : group;
    return pool.reduce((a, b) => (b.total_size_bytes < a.total_size_bytes ? b : a));
  }
  return null;
}

/** A whisper.cpp ggml model file (`ggml-base.en.bin`), as opposed to a GGUF. */
export function isWhisperVariant(v: HfModelVariant): boolean {
  const file = v.shards[0]?.filename ?? v.name;
  const base = (file.split('/').pop() ?? file).toLowerCase();
  return base.startsWith('ggml-') && (base.endsWith('.bin') || !base.includes('.'));
}

/** Whisper needs roughly this many times the file size in memory (weights + compute buffers). */
export const WHISPER_MEMORY_FACTOR = 1.5;

/** Whether a whisper file fits in memory; null when the size or the RAM figure is unknown. */
export function whisperFitsMemory(sizeBytes: number, availableRamBytes: number | null | undefined): boolean | null {
  if (sizeBytes <= 0 || !availableRamBytes) return null;
  return sizeBytes * WHISPER_MEMORY_FACTOR <= availableRamBytes;
}

/** Multilingual quantized models, best quality / speed trade-off first. */
const WHISPER_PREFERENCE = [
  'ggml-large-v3-turbo-q8_0',
  'ggml-large-v3-turbo-q5_0',
  'ggml-small-q8_0',
  'ggml-small-q5_1',
  'ggml-base-q8_0',
  'ggml-base-q5_1',
];

/**
 * Default whisper model: the first of a fixed preference list (turbo large, then small, then
 * base class) that fits in memory. VRAM fit and GGUF quant ranking mean nothing here.
 */
export function recommendWhisperVariant(variants: HfModelVariant[], availableRamBytes: number | null | undefined): HfModelVariant | null {
  for (const name of WHISPER_PREFERENCE) {
    const v = variants.find((x) => x.name.toLowerCase() === name);
    if (v && whisperFitsMemory(v.total_size_bytes, availableRamBytes) === true) return v;
  }
  return null;
}
