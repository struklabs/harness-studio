// Playground Image tab: request helper for the gateway's /v1/images/generations and, with a start
// image, /v1/images/edits (sd-server or a Python engine diffusers worker behind it; nothing is simulated).
import { describeErrorBody } from './sse';

/** Largest side the gateway accepts (`images::SIDE_RANGE`). */
export const MAX_SIDE = 4096;
export const MIN_SIDE = 64;

export interface ImageParams {
  prompt: string;
  negative: string;
  /** `WIDTHxHEIGHT`; empty = the model's own size. */
  size: string;
  /** Empty = the model's own. */
  steps: string;
  cfgScale: string;
  /** Empty or -1 = random. */
  seed: string;
  count: number;
  /** Start image (image-to-image); null = text to image. */
  init?: ImageResult | null;
  /** How much of the start image changes, 0..1; empty = the engine's default (sd-server 0.75, diffusers 0.8). */
  strength?: string;
}

export interface ImageResult {
  /** Base64 of the encoded picture. */
  data: string;
  format: string;
}

export { DIFFUSERS_IMAGE_TASKS, imageModel, isImageDefinition, needsStartImage, takesStrength } from './imageModels';

/** `512x768` -> [512, 768]; null when it is not a size the gateway takes (multiple of 8, 64..4096). */
export function parseSize(size: string): [number, number] | null {
  const m = /^\s*(\d+)\s*[xX]\s*(\d+)\s*$/.exec(size);
  if (!m) return null;
  const w = Number(m[1]);
  const h = Number(m[2]);
  const ok = (v: number) => v >= MIN_SIDE && v <= MAX_SIDE && v % 8 === 0;
  return ok(w) && ok(h) ? [w, h] : null;
}

/** The request body of the gateway's images API for the form values (empty fields are left out). */
export function requestBody(model: string, p: ImageParams): Record<string, unknown> {
  const body: Record<string, unknown> = {
    model,
    prompt: p.prompt.trim(),
    n: p.count,
    response_format: 'b64_json',
  };
  if (p.negative.trim()) body.negative_prompt = p.negative.trim();
  if (p.size.trim()) body.size = p.size.trim().toLowerCase();
  const steps = Number(p.steps);
  if (p.steps.trim() && Number.isInteger(steps) && steps > 0) body.steps = steps;
  const cfg = Number(p.cfgScale);
  if (p.cfgScale.trim() && Number.isFinite(cfg) && cfg >= 0) body.cfg_scale = cfg;
  const seed = Number(p.seed);
  if (p.seed.trim() && Number.isInteger(seed)) body.seed = seed;
  if (p.init) {
    body.image = dataUrl(p.init);
    const strength = Number(p.strength ?? '');
    if (p.strength?.trim() && strength >= 0 && strength <= 1) body.strength = strength;
  }
  return body;
}

/** Whether a strength field is empty or a number from 0 to 1. */
export function strengthOk(strength: string): boolean {
  const v = Number(strength);
  return strength.trim() === '' || (Number.isFinite(v) && v >= 0 && v <= 1);
}

/** Image types the start image may be (what sd-server decodes and the gateway sizes). */
export const START_IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/webp'];

/** Largest start image file (a larger one is a camera original; the gateway takes up to 4096 px a side). */
export const MAX_START_IMAGE_BYTES = 20 * 1024 * 1024;

/** A picked file as a start image: its base64 and format, or why it was refused. */
export async function readStartImage(file: File): Promise<ImageResult | 'type' | 'size'> {
  if (!START_IMAGE_TYPES.includes(file.type)) return 'type';
  if (file.size > MAX_START_IMAGE_BYTES) return 'size';
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return { data: btoa(binary), format: file.type.slice('image/'.length) };
}

/** The image did not fit in GPU memory at the requested size (gateway `image_out_of_memory`). */
export class ImageOutOfMemory extends Error {
  constructor(
    message: string,
    readonly needMb: number,
    readonly availableMb: number,
  ) {
    super(message);
  }
}

/** The out-of-memory error of a failed answer's body, or null. */
export function outOfMemory(body: string): ImageOutOfMemory | null {
  try {
    const e = JSON.parse(body)?.error;
    if (e?.code !== 'image_out_of_memory') return null;
    const need = Number(e.harness?.need_mb);
    const available = Number(e.harness?.available_mb);
    if (!Number.isFinite(need) || !Number.isFinite(available)) return null;
    return new ImageOutOfMemory(String(e.message ?? ''), need, available);
  } catch {
    return null;
  }
}

/** POST {baseUrl}/images/generations, or /images/edits with a start image. Throws the gateway's message on a failed request. */
export async function fetchImages(
  baseUrl: string,
  model: string,
  params: ImageParams,
  headers: Record<string, string>,
  signal?: AbortSignal,
): Promise<{ images: ImageResult[]; ms: number }> {
  const started = performance.now();
  const res = await fetch(`${baseUrl}/images/${params.init ? 'edits' : 'generations'}`, {
    method: 'POST',
    headers: { ...headers, 'Content-Type': 'application/json' },
    body: JSON.stringify(requestBody(model, params)),
    signal,
  });
  const text = await res.text();
  const ms = performance.now() - started;
  if (!res.ok) throw outOfMemory(text) ?? new Error(`HTTP ${res.status}: ${describeErrorBody(text) || res.statusText}`);
  const json = JSON.parse(text) as { data?: { b64_json?: string }[]; output_format?: string };
  const format = typeof json.output_format === 'string' ? json.output_format : 'png';
  const images = (json.data ?? []).flatMap((d) => (typeof d.b64_json === 'string' ? [{ data: d.b64_json, format }] : []));
  if (images.length === 0) throw new Error('The gateway answered without an image.');
  return { images, ms };
}

/** `WIDTHxHEIGHT` of a PNG from its header (IHDR); null for any other format or a broken header. */
export function pngSize(image: ImageResult): string | null {
  try {
    const head = atob(image.data.slice(0, 32));
    if (!head.startsWith('PNG')) return null;
    const u32 = (at: number) =>
      ((head.charCodeAt(at) << 24) | (head.charCodeAt(at + 1) << 16) | (head.charCodeAt(at + 2) << 8) | head.charCodeAt(at + 3)) >>> 0;
    const [w, h] = [u32(16), u32(20)];
    return w > 0 && h > 0 ? `${w}x${h}` : null;
  } catch {
    return null;
  }
}

export function dataUrl(image: ImageResult): string {
  return `data:image/${image.format};base64,${image.data}`;
}

/** The sizes the form offers (SD1.x / Turbo like 512, SDXL / SD3 / FLUX like 1024). */
export const SIZE_PRESETS = ['512x512', '768x768', '1024x1024', '512x768', '768x512', '1024x768', '768x1024'] as const;
