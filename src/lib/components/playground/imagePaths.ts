import { invoke } from '#lib/ipc.js';
import { isImageFile, loadImage, type PgImage } from './images';

/** Dropped paths that are pictures (the rest are documents for the RAG side). */
export function splitImagePaths(paths: string[]): { images: string[]; others: string[] } {
  const images: string[] = [];
  const others: string[] = [];
  for (const p of paths) {
    const name = p.split(/[/\\]/).pop() ?? p;
    (isImageFile({ type: '', name }) ? images : others).push(p);
  }
  return { images, others };
}

const MIME: Record<string, string> = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', bmp: 'image/bmp', webp: 'image/webp', avif: 'image/avif',
};

/** A dropped picture file: bytes through the backend (the window gives paths), then the usual scaling. */
export async function loadImagePath(path: string): Promise<PgImage> {
  const name = path.split(/[/\\]/).pop() ?? path;
  const b64 = await invoke<string>('read_image_file', { path });
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  const ext = name.split('.').pop()?.toLowerCase() ?? '';
  return loadImage(new File([bytes], name, { type: MIME[ext] ?? '' }));
}
