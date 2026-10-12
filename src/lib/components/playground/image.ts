// Playground Image tab: request helper for the gateway's /v1/images/generations (a real sd-server
// behind it; nothing is simulated).
import { describeErrorBody } from './sse';
import type { LocalModel, ModelDefinition } from '$lib/types';

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
}

export interface ImageResult {
  /** Base64 of the encoded picture. */
  data: string;
  format: string;
}

/** A definition served by stable-diffusion.cpp: the file is a checkpoint, or the library says image. */
export function isImageDefinition(d: ModelDefinition, files: LocalModel[] = []): boolean {
  const path = d.base.kind === 'local' ? d.base.path : d.base.file;
  if (/\.(safetensors|ckpt)$/i.test(path)) return true;
  const norm = (v: string) => v.replace(/\\/g, '/').toLowerCase();
  return files.some((f) => f.kind === 'image' && norm(f.path) === norm(path));
}

/** Keep the user's selection while it is still available. */
export function imageModel(current: string, definitions: ModelDefinition[], files: LocalModel[] = []): string {
  const models = definitions.filter((d) => isImageDefinition(d, files));
  return models.some((d) => d.alias === current) ? current : models[0]?.alias ?? '';
}

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
  return body;
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

/** POST {baseUrl}/images/generations. Throws the gateway's message on a failed request. */
export async function fetchImages(
  baseUrl: string,
  model: string,
  params: ImageParams,
  headers: Record<string, string>,
  signal?: AbortSignal,
): Promise<{ images: ImageResult[]; ms: number }> {
  const started = performance.now();
  const res = await fetch(`${baseUrl}/images/generations`, {
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
