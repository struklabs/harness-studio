// One image session for the Playground Image tab: the form and the gallery survive a tab switch
// (the pictures live in memory only; "Save" writes one to a file).
import type { ImageResult } from './image';

export interface GalleryItem {
  id: number;
  image: ImageResult;
  prompt: string;
  model: string;
  size: string;
  ms: number;
}

/** Pictures kept in the gallery (older ones drop out). */
export const GALLERY_LIMIT = 24;

export const imageSession = $state({
  model: '',
  prompt: '',
  negative: '',
  size: '',
  steps: '',
  cfgScale: '',
  seed: '',
  count: 1,
  init: null as ImageResult | null,
  strength: '',
  gallery: [] as GalleryItem[],
});

let nextId = 1;

/** Adds pictures (newest first) and keeps the gallery within its limit. */
export function addToGallery(items: Omit<GalleryItem, 'id'>[]): void {
  const added = items.map((i) => ({ ...i, id: nextId++ }));
  imageSession.gallery = [...added.reverse(), ...imageSession.gallery].slice(0, GALLERY_LIMIT);
}

export function removeFromGallery(id: number): void {
  imageSession.gallery = imageSession.gallery.filter((g) => g.id !== id);
}
