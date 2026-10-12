// Rendered diagram (Mermaid SVG) → PNG, for "export as PNG" next to "export as SVG". The SVG
// is drawn on a canvas over an opaque background (a transparent PNG would be unreadable on a
// viewer's dark page when the diagram uses dark text) and saved through the Tauri save dialog.
// Pure helpers first (no DOM, testable); the canvas part is at the end.

export interface SvgSize {
  width: number;
  height: number;
}

/** Root `<svg ...>` start tag of a document (an XML declaration or comments may precede it). */
function rootTag(svg: string): RegExpExecArray | null {
  return /<svg\b[^>]*>/i.exec(svg);
}

function attr(tag: string, name: string): string | null {
  const m = new RegExp(`\\s${name}\\s*=\\s*("([^"]*)"|'([^']*)')`, 'i').exec(tag);
  return m ? (m[2] ?? m[3] ?? '') : null;
}

/** Plain pixel number of an attribute (`"640"`, `"640px"`); `null` for percentages and units. */
function pixels(value: string | null): number | null {
  if (!value) return null;
  const m = /^\s*(\d+(?:\.\d+)?)(?:px)?\s*$/.exec(value);
  return m ? Number(m[1]) : null;
}

/** Size of an SVG in user units: the `viewBox`, else numeric `width`/`height`. */
export function svgSize(svg: string): SvgSize | null {
  const m = rootTag(svg);
  if (!m) return null;
  const vb = attr(m[0], 'viewBox');
  if (vb) {
    const n = vb.trim().split(/[\s,]+/).map(Number);
    if (n.length === 4 && n.every(Number.isFinite) && n[2] > 0 && n[3] > 0) return { width: n[2], height: n[3] };
  }
  const width = pixels(attr(m[0], 'width'));
  const height = pixels(attr(m[0], 'height'));
  return width && height ? { width, height } : null;
}

/** Largest side and pixel count of the exported image (a canvas above that fails or blanks). */
export const MAX_SIDE = 8192;
export const MAX_PIXELS = 36_000_000;

/** Scale of the PNG relative to the SVG's size: `target` (2 = crisp on high-DPI), reduced to fit the limits. */
export function pngScale(size: SvgSize, target = 2): number {
  const bySide = MAX_SIDE / Math.max(size.width, size.height);
  const byPixels = Math.sqrt(MAX_PIXELS / (size.width * size.height));
  return Math.max(0.1, Math.min(target, bySide, byPixels));
}

/** The SVG with explicit pixel `width`/`height` and the SVG namespace (needed to load it as an image). */
export function withIntrinsicSize(svg: string, size: SvgSize): string {
  const m = rootTag(svg);
  if (!m) return svg;
  let tag = m[0]
    .replace(/\swidth\s*=\s*("[^"]*"|'[^']*')/i, '')
    .replace(/\sheight\s*=\s*("[^"]*"|'[^']*')/i, '');
  if (!/\sxmlns\s*=/i.test(tag)) tag = tag.replace(/^<svg/i, '<svg xmlns="http://www.w3.org/2000/svg"');
  tag = tag.replace(/^<svg/i, `<svg width="${size.width}" height="${size.height}"`);
  return svg.slice(0, m.index) + tag + svg.slice(m.index + m[0].length);
}

/**
 * Draws the SVG on a canvas of `scale` times its size over `background` (any CSS colour) and
 * returns the PNG bytes. Throws when the SVG has no size or the browser cannot decode it.
 */
export async function svgToPng(svg: string, background: string, target = 2): Promise<Uint8Array> {
  const size = svgSize(svg);
  if (!size) throw new Error('diagram has no size');
  const scale = pngScale(size, target);
  const width = Math.max(1, Math.round(size.width * scale));
  const height = Math.max(1, Math.round(size.height * scale));
  const source = withIntrinsicSize(svg, size);
  const img = new Image();
  img.decoding = 'sync';
  await new Promise<void>((resolve, reject) => {
    img.onload = () => resolve();
    img.onerror = () => reject(new Error('diagram image could not be decoded'));
    // A data URL (not a blob URL): the SVG has no external references, so the canvas stays clean.
    img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(source)}`;
  });
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('no canvas');
  ctx.fillStyle = background;
  ctx.fillRect(0, 0, width, height);
  ctx.drawImage(img, 0, 0, width, height);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
  if (!blob) throw new Error('PNG encoding failed');
  return new Uint8Array(await blob.arrayBuffer());
}
