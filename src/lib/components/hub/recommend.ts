import type { FitCategory, HfModelVariant } from '#lib/types.js';

/** Leading number of a quant name ("Q4_K_M" -> 4, "IQ3_XS" -> 3, "BF16" -> 16). */
export function quantBits(q: string): number {
  const u = q.toUpperCase();
  if (/^(BF16|F16|FP16)/.test(u)) return 16;
  if (/^(F32|FP32)/.test(u)) return 32;
  const m = u.match(/(\d+)/);
  return m ? parseInt(m[1], 10) : 0;
}

/** Vision projectors, speculative-decoding drafts, image VAEs / text encoders / LoRAs: never the main model. */
const COMPANION_QUANTS = new Set(['mmproj', 'component', 'lora']);
const COMPANION_PATH = /mmproj|projector|(?:^|[/\\_. -])(?:mtp|draft|assistant|eagle\d*|dflash|dspark)(?:[/\\_. -]|$)/i;

/** A companion file (projector, draft model, VAE, text encoder, LoRA) rather than a standalone model. */
export function isCompanionVariant(v: HfModelVariant): boolean {
  return COMPANION_QUANTS.has(v.quantization.toLowerCase()) || [v.name, ...v.shards.map((s) => s.filename)].some((path) => COMPANION_PATH.test(path));
}

/** Whether the precision label says anything ("SAFETENSORS", "Unknown", "component" do not). */
export function hasKnownPrecision(q: string): boolean {
  const u = q.toUpperCase();
  return !u.startsWith('TYPE_') && /\d/.test(u);
}

const TIERS = ['full_gpu', 'tight', 'hybrid', 'cpu_heavy'] as const;

function currentVariants(variants: HfModelVariant[]): HfModelVariant[] {
  const current = variants.filter((v) => ![v.name, ...v.shards.map((s) => s.filename)].some((path) =>
    /(?:^|[/\\])(?:old|legacy|deprecated)[/\\]/i.test(path)));
  return current.length ? current : variants;
}

/**
 * The variant that suits this machine best, from the same fit estimate as the variant table
 * (`fits`: variant name -> category of the Rust estimate). Prefers the largest (best quality)
 * quantized file that fully fits in VRAM; when nothing fits fully, the smallest
 * 4-bit-or-better file of the best tier still possible. Null when sizes or fits are unknown.
 */
export function recommendVariant(variants: HfModelVariant[], fits: Map<string, FitCategory>): HfModelVariant | null {
  // An unparsed precision reads as 0 bits: it must not beat a file whose precision is known.
  const mains = variants.filter((v) => !isCompanionVariant(v));
  const anyKnown = mains.some((v) => hasKnownPrecision(v.quantization));
  const sized = currentVariants(mains.filter((v) => v.total_size_bytes > 0 && quantBits(v.quantization) <= 8 && fits.has(v.name)
    && (!anyKnown || hasKnownPrecision(v.quantization))));
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

/**
 * Fallback for a repo that has no quantized file, only floats (diffusers/safetensors checkpoints):
 * the best fit tier, and in it FP16/BF16 over FP32 (half the size, same quality in practice),
 * then the smaller file. Files with an unparsed precision never qualify. Kept apart from
 * `recommendVariant` so onboarding, which wants quantized files only, is unchanged.
 */
export function recommendFloatVariant(variants: HfModelVariant[], fits: Map<string, FitCategory>): HfModelVariant | null {
  const floats = currentVariants(variants.filter((v) => !isCompanionVariant(v) && v.total_size_bytes > 0 && fits.has(v.name) && /^(BF16|F16|FP16|F32|FP32)/i.test(v.quantization)));
  for (const tier of TIERS) {
    const group = floats.filter((v) => fits.get(v.name) === tier);
    if (group.length === 0) continue;
    return group.reduce((a, b) => {
      const d = quantBits(a.quantization) - quantBits(b.quantization);
      return d > 0 || (d === 0 && b.total_size_bytes < a.total_size_bytes) ? b : a;
    });
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

/** On a CPU the encoder dominates: large-v3-turbo's takes about four times small's, so small first. */
const WHISPER_CPU_PREFERENCE = [
  'ggml-small-q8_0',
  'ggml-small-q5_1',
  'ggml-large-v3-turbo-q8_0',
  'ggml-large-v3-turbo-q5_0',
  'ggml-base-q8_0',
  'ggml-base-q5_1',
];

/** Whether whisper.cpp runs on the CPU here: upstream publishes no GPU build for Linux. */
export function whisperOnCpu(os: string | null | undefined): boolean {
  return os === 'linux';
}

/**
 * Default whisper model: the first of a fixed preference list (turbo large, then small, then
 * base class; small first when whisper.cpp runs on the CPU) that fits in memory. VRAM fit and
 * GGUF quant ranking mean nothing here.
 */
export function recommendWhisperVariant(variants: HfModelVariant[], availableRamBytes: number | null | undefined, cpu = false): HfModelVariant | null {
  for (const name of cpu ? WHISPER_CPU_PREFERENCE : WHISPER_PREFERENCE) {
    const v = variants.find((x) => x.name.toLowerCase() === name);
    if (v && whisperFitsMemory(v.total_size_bytes, availableRamBytes) === true) return v;
  }
  return null;
}

/** An ONNX CTC speech package (wav2vec2, HuBERT, WavLM): a graph plus `preprocessor_config.json`. */
export function isCtcVariant(v: HfModelVariant): boolean {
  const files = v.shards.map((s) => s.filename.toLowerCase());
  return files.some((f) => f.endsWith('.onnx')) && files.some((f) => f.endsWith('preprocessor_config.json'));
}

/**
 * CTC speech models are small, and their INT8 exports lose words (and cannot run on DirectML):
 * prefer the most precise file that fits in memory, FP32 first.
 */
export function recommendCtcVariant(variants: HfModelVariant[], availableRamBytes: number | null | undefined): HfModelVariant | null {
  const fitting = variants.filter((v) => v.total_size_bytes > 0 && (!availableRamBytes || v.total_size_bytes * 2 <= availableRamBytes));
  if (fitting.length === 0) return null;
  return fitting.reduce((a, b) => {
    const diff = quantBits(b.quantization) - quantBits(a.quantization);
    return diff > 0 || (diff === 0 && b.total_size_bytes < a.total_size_bytes) ? b : a;
  });
}

/** Native ASR GGUF: prefer practical 4/5-bit variants that fit the available memory. */
export function recommendCrispasrVariant(variants: HfModelVariant[], availableRamBytes?: number | null, availableVramBytes?: number | null): HfModelVariant | null {
  const budget = Math.max(availableRamBytes ?? 0, availableVramBytes ?? 0);
  if (!budget) return null;
  const fits = variants.filter(v => !v.not_runnable && v.total_size_bytes > 0 && v.total_size_bytes * 1.5 <= budget);
  const preference = ['Q5_K_M', 'Q4_K_M', 'Q4_K', 'Q8_0', 'F16'];
  for (const quant of preference) {
    const choice = fits.find(v => v.quantization.toUpperCase() === quant);
    if (choice) return choice;
  }
  return fits.sort((a, b) => a.total_size_bytes - b.total_size_bytes)[0] ?? null;
}

/** Projector precision order: F16 first (most widely supported), then BF16, F32, anything else. */
function projectorRank(v: HfModelVariant): number {
  const name = `${v.name} ${v.shards[0]?.filename ?? ''}`.toLowerCase();
  if (/(^|[^a-z0-9])f16/.test(name) || name.includes('fp16')) return 0;
  if (/bf16/.test(name)) return 1;
  if (/(^|[^a-z0-9])f32/.test(name) || name.includes('fp32')) return 2;
  return 3;
}

/**
 * The vision projector (mmproj) that goes with a downloaded main GGUF: from the same folder of
 * the repo as `main`, F16 -> BF16 -> F32 -> any (smallest). Null when `main` is itself a
 * projector or not a GGUF, or the repo has no projector file.
 */
export function pickProjectorVariant(variants: HfModelVariant[], main: HfModelVariant): HfModelVariant | null {
  const mainFile = main.shards[0]?.filename ?? '';
  if (main.quantization === 'mmproj' || main.quantization === 'component' || !mainFile.toLowerCase().endsWith('.gguf')) return null;
  const dirOf = (f: string) => (f.includes('/') ? f.slice(0, f.lastIndexOf('/')) : '').toLowerCase();
  const projectors = variants.filter((v) => v.quantization === 'mmproj' && v.shards.length > 0 && v.shards[0].filename.toLowerCase().endsWith('.gguf'));
  const sameDir = projectors.filter((v) => dirOf(v.shards[0].filename) === dirOf(mainFile));
  const pool = sameDir.length ? sameDir : projectors.filter((v) => dirOf(v.shards[0].filename) === '');
  if (pool.length === 0) return null;
  return pool.reduce((a, b) => {
    const d = projectorRank(b) - projectorRank(a);
    return d < 0 || (d === 0 && b.total_size_bytes < a.total_size_bytes) ? b : a;
  });
}
