// Image attachments of a Playground message (vision models). Pure helpers first; the browser decoding at the end only runs from the UI.

/** An image attached to a user message. `data` is a `data:image/...;base64,...` URL. */
export interface PgImage {
  name: string;
  mime: string;
  data: string;
}

/** Longest edge sent to the model; larger pictures are scaled down. */
export const MAX_IMAGE_EDGE = 2048;
/** Largest picture file accepted at all. */
export const MAX_IMAGE_FILE_BYTES = 25 * 1024 * 1024;
/** Largest encoded picture kept with a message (after scaling). */
export const MAX_IMAGE_BYTES = 4 * 1024 * 1024;
/** Pictures per message. */
export const MAX_IMAGES_PER_MESSAGE = 6;
/** Rough prompt cost of one picture, for the context meter only (the server counts exactly). */
export const IMAGE_TOKEN_ESTIMATE = 576;

/** Formats llama.cpp's image loader reads; anything else is re-encoded through a canvas. */
const PASS_THROUGH = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/bmp']);

export type ImageErrorCode = 'type' | 'file_too_large' | 'too_large' | 'decode' | 'limit';

export class ImageError extends Error {
  code: ImageErrorCode;
  file: string;
  constructor(code: ImageErrorCode, file: string) {
    super(`${code}: ${file}`);
    this.code = code;
    this.file = file;
  }
}

export function isImageFile(f: { type: string; name: string }): boolean {
  return f.type.startsWith('image/') || /\.(png|jpe?g|gif|bmp|webp|avif)$/i.test(f.name);
}

/** Size that fits `max` on the longest edge, keeping the aspect ratio (never enlarges). */
export function fitWithin(width: number, height: number, max = MAX_IMAGE_EDGE): { width: number; height: number; scaled: boolean } {
  const longest = Math.max(width, height);
  if (!(longest > max)) return { width, height, scaled: false };
  const k = max / longest;
  return { width: Math.max(1, Math.round(width * k)), height: Math.max(1, Math.round(height * k)), scaled: true };
}

/** Decoded size of a base64 data URL. */
export function dataUrlBytes(url: string): number {
  const i = url.indexOf(',');
  if (i < 0) return 0;
  const b64 = url.slice(i + 1);
  const pad = b64.endsWith('==') ? 2 : b64.endsWith('=') ? 1 : 0;
  return Math.max(0, Math.floor((b64.length * 3) / 4) - pad);
}

export function isImageDataUrl(v: unknown): v is string {
  return typeof v === 'string' && /^data:image\/[a-z0-9.+-]+;base64,/i.test(v);
}

/** One OpenAI `content` part list for a user message: the text first, then each picture. */
export function userContent(text: string, images?: PgImage[]): string | Record<string, unknown>[] {
  if (!images?.length) return text;
  const parts: Record<string, unknown>[] = [];
  if (text) parts.push({ type: 'text', text });
  for (const img of images) parts.push({ type: 'image_url', image_url: { url: img.data } });
  return parts;
}

/** The text of a `content` value (parts are joined, pictures dropped). */
export function contentText(content: unknown): string {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  return content
    .map((p) => (p && typeof p === 'object' && (p as { type?: unknown }).type === 'text' ? String((p as { text?: unknown }).text ?? '') : ''))
    .filter(Boolean)
    .join('\n');
}

/** Pictures of a request's messages (for the context estimate). */
export function countImageParts(messages: Record<string, unknown>[]): number {
  let n = 0;
  for (const m of messages) {
    if (!Array.isArray(m.content)) continue;
    for (const p of m.content) if (p && typeof p === 'object' && (p as { type?: unknown }).type === 'image_url') n++;
  }
  return n;
}

/**
 * The same messages without picture data: counting tokens or measuring the history must not
 * ship megabytes of base64 around. Pictures are counted separately (`IMAGE_TOKEN_ESTIMATE`).
 */
export function withoutImages(messages: Record<string, unknown>[]): Record<string, unknown>[] {
  return messages.map((m) => (Array.isArray(m.content) ? { ...m, content: contentText(m.content) } : m));
}

/** `meta.images` of a stored variant (invalid entries are dropped). */
export function imagesOfMeta(meta: unknown): PgImage[] | undefined {
  const raw = (meta as { images?: unknown } | null | undefined)?.images;
  if (!Array.isArray(raw)) return undefined;
  const out: PgImage[] = [];
  for (const r of raw) {
    const o = r as Partial<PgImage> | null;
    if (!o || !isImageDataUrl(o.data)) continue;
    out.push({ name: typeof o.name === 'string' ? o.name : 'image', mime: typeof o.mime === 'string' ? o.mime : 'image/png', data: o.data });
  }
  return out.length ? out : undefined;
}

/** `/props` of llama-server: whether the loaded model reads pictures (`null` = not reported). */
export function visionFromProps(props: unknown): boolean | null {
  const v = (props as { modalities?: { vision?: unknown } } | null | undefined)?.modalities?.vision;
  return typeof v === 'boolean' ? v : null;
}

// ---- Browser only: file -> PgImage ----

function readAsDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error);
    r.readAsDataURL(blob);
  });
}

/**
 * Reads a picture for sending: scaled to `MAX_IMAGE_EDGE`, re-encoded when the format is not
 * one llama.cpp reads (WebP, AVIF) or the file is big. Throws `ImageError` with a reason.
 */
export async function loadImage(file: File): Promise<PgImage> {
  const name = file.name || 'image';
  if (!isImageFile(file)) throw new ImageError('type', name);
  if (file.size > MAX_IMAGE_FILE_BYTES) throw new ImageError('file_too_large', name);
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file);
  } catch {
    throw new ImageError('decode', name);
  }
  try {
    const fit = fitWithin(bitmap.width, bitmap.height);
    if (!fit.scaled && PASS_THROUGH.has(file.type) && file.size <= MAX_IMAGE_BYTES) {
      return { name, mime: file.type, data: await readAsDataUrl(file) };
    }
    const canvas = document.createElement('canvas');
    canvas.width = fit.width;
    canvas.height = fit.height;
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new ImageError('decode', name);
    // JPEG has no alpha: paint white first so transparent areas do not turn black.
    ctx.fillStyle = '#fff';
    ctx.fillRect(0, 0, fit.width, fit.height);
    ctx.drawImage(bitmap, 0, 0, fit.width, fit.height);
    const toBlob = (type: string, q?: number) => new Promise<Blob | null>((res) => canvas.toBlob(res, type, q));
    for (const q of [0.9, 0.78, 0.62]) {
      const blob = await toBlob('image/jpeg', q);
      if (blob && blob.size <= MAX_IMAGE_BYTES) {
        return { name, mime: 'image/jpeg', data: await readAsDataUrl(blob) };
      }
    }
    throw new ImageError('too_large', name);
  } finally {
    bitmap.close();
  }
}
