/** Frontend limits mirror harness-gateway images.rs/videos.rs; no model timings are invented. */
export type Quality = 'fast' | 'balanced' | 'quality';
export const QUALITIES: Quality[] = ['fast', 'balanced', 'quality'];
export type GenerationEngine = 'sdcpp' | 'diffusers' | 'mflux' | 'comfyui' | 'unknown';
export function generationPreset(kind: 'image' | 'video', engine: GenerationEngine, quality: Quality, model = '') {
  const index = QUALITIES.indexOf(quality);
  const distilled = /turbo|schnell|klein(?!.*base)/i.test(model);
  const steps = (distilled ? [4, 6, 8] : [8, 20, 30])[index];
  const video = kind === 'video';
  return { steps, size: (video ? ['256x256', '416x240', '640x384'] : engine === 'mflux' || /xl|flux|sd3|qwen/i.test(model) ? ['512x512', '768x768', '1024x1024'] : ['512x512', '512x768', '768x768'])[index], frames: [9, 33, 65][index] };
}
export function effectiveFrames(frames: number, ltx = false): number {
  if (!Number.isInteger(frames) || frames < 1 || frames > 257) return 0;
  const group = ltx ? 8 : 4;
  return Math.floor((frames - 1) / group) * group + 1;
}
/** Encoded playback duration (frame count / fps), including the initial frame. */
export function videoDuration(frames: number, fps: number): number | null {
  return Number.isInteger(frames) && frames > 0 && Number.isInteger(fps) && fps >= 1 && fps <= 60 ? frames / fps : null;
}
export function videoFormats(engine: GenerationEngine): readonly string[] {
  return engine === 'sdcpp' ? ['webm', 'webp', 'avi'] : engine === 'diffusers' ? ['mp4', 'webm'] : ['webm'];
}
export function inRange(value: unknown, min: number, max: number, integer = false): boolean {
  return typeof value === 'number' && Number.isFinite(value) && value >= min && value <= max && (!integer || Number.isSafeInteger(value));
}
export function optionalNumber(value: string, min: number, max: number, integer = false): boolean {
  return value.trim() === '' || inRange(Number(value), min, max, integer);
}
export function validSize(value: string, engine: GenerationEngine, optional = true): boolean {
  if (!value.trim()) return optional;
  const m = /^\s*(\d+)\s*[xX]\s*(\d+)\s*$/.exec(value);
  if (!m) return false;
  const multiple = engine === 'mflux' ? 16 : 8;
  return m.slice(1).every(v => inRange(Number(v), 64, engine === 'mflux' ? 2048 : engine === 'comfyui' ? 8192 : 4096, true) && Number(v) % multiple === 0);
}
export function imageParameterErrors(p: { size: string; steps: string; cfgScale: string; seed: string; count: number }, engine: GenerationEngine) {
  return {
    size: !validSize(p.size, engine),
    steps: !optionalNumber(p.steps, 1, engine === 'mflux' ? 50 : engine === 'comfyui' ? 200 : 500, true),
    cfg: engine !== 'mflux' && !optionalNumber(p.cfgScale, 0, 100),
    seed: !optionalNumber(p.seed, -1, engine === 'mflux' ? 4294967295 : Number.MAX_SAFE_INTEGER, true),
    count: !inRange(p.count, 1, engine === 'mflux' ? 4 : 8, true),
  };
}
/** Only compare the same model and exact settings; different devices/workloads have no reliable scaling. */
export function matchingEstimate(samples: { model: string; signature: string; ms: number }[], model: string, signature: string): number | null {
  const sample = samples.findLast(s => s.model === model && s.signature === signature && s.ms > 0 && Number.isFinite(s.ms));
  return sample ? sample.ms / 1000 : null;
}

export function trainingPreset(quality: Quality) {
  const [steps, rank, sequenceLength] = { fast: [50, 8, 256], balanced: [100, 16, 512], quality: [300, 32, 1024] }[quality];
  return { steps, rank, sequenceLength };
}
export function audioPresetSteps(quality: Quality, min: number, max: number): number {
  return Math.max(min, Math.min(max, { fast: 8, balanced: 20, quality: 30 }[quality]));
}
