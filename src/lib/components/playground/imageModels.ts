// Which library models the Playground Image tab offers and what each needs (pure; tested by
// scripts/test-image-models.mjs).
import type { LocalModel, ModelDefinition } from '#lib/types.js';

/** Diffusers tasks of the Python engine that make pictures (image-to-image: InstructPix2Pix,
 * Qwen-Image-Edit... start from an image). */
export const DIFFUSERS_IMAGE_TASKS = ['text-to-image', 'image-to-image'];

const norm = (v: string) => v.replace(/\\/g, '/').toLowerCase();
const definitionPath = (d: ModelDefinition) => (d.base.kind === 'local' ? d.base.path : d.base.file);

/** A definition that draws: stable-diffusion.cpp's (the file is a checkpoint, or the library says
 * image) or a diffusers text-to-image / image-to-image folder of the Python engine. */
export function isImageDefinition(d: ModelDefinition, files: LocalModel[] = []): boolean {
  const path = definitionPath(d);
  if (/\.(safetensors|ckpt)$/i.test(path)) return true;
  const draws = (f: LocalModel) => f.kind === 'image' || (f.kind === 'pipeline' && DIFFUSERS_IMAGE_TASKS.includes(f.task ?? ''));
  return files.some((f) => draws(f) && norm(f.path) === norm(path));
}

/** The Python engine image-to-image folder behind the model `alias`, if it is one. */
function editPipeline(alias: string, definitions: ModelDefinition[], files: LocalModel[]): LocalModel | undefined {
  const d = definitions.find((x) => x.alias === alias);
  if (!d) return undefined;
  const path = norm(definitionPath(d));
  return files.find((f) => f.kind === 'pipeline' && f.task === 'image-to-image' && norm(f.path) === path);
}

/** Whether the model `alias` only edits: a Python engine image-to-image pipeline needs a start image. */
export function needsStartImage(alias: string, definitions: ModelDefinition[], files: LocalModel[] = []): boolean {
  return !!editPipeline(alias, definitions, files);
}

/** Whether the model `alias` takes a strength: every image model but the instruction-following
 * editors (InstructPix2Pix, Qwen-Image-Edit), which have none; the Img2Img pipelines do. */
export function takesStrength(alias: string, definitions: ModelDefinition[], files: LocalModel[] = []): boolean {
  const f = editPipeline(alias, definitions, files);
  return !f || (f.architecture ?? '').includes('Img2Img');
}

/** mflux's names of the models it runs (`MFLUX_MODELS` in engine/mlx.rs): the library gives an
 * mflux folder its model's name as architecture. */
export const MFLUX_MODELS = [
  'flux2-klein-4b', 'flux2-klein-base-4b', 'flux2-klein-9b', 'flux2-klein-9b-kv', 'flux2-klein-base-9b',
  'z-image-turbo', 'z-image', 'schnell', 'dev', 'krea-dev', 'dev-fill', 'qwen-image',
];
/** mflux models that inpaint (an image and a mask; nothing from text alone). */
const MFLUX_INPAINT = ['dev-fill'];

function mfluxFile(alias: string, definitions: ModelDefinition[], files: LocalModel[]): LocalModel | undefined {
  const d = definitions.find((x) => x.alias === alias);
  if (!d) return undefined;
  const path = norm(definitionPath(d));
  return files.find((f) => f.kind === 'pipeline' && MFLUX_MODELS.includes(f.architecture ?? '') && norm(f.path) === path);
}

/** Whether the model `alias` is an mflux folder (Apple Silicon: FLUX.2 [klein], Z-Image, FLUX.1,
 * Qwen-Image): it takes no negative prompt and no classifier-free guidance, and without a size
 * it draws the largest that fits the Mac's memory. */
export function isMflux(alias: string, definitions: ModelDefinition[], files: LocalModel[] = []): boolean {
  return !!mfluxFile(alias, definitions, files);
}

/** Whether the model `alias` inpaints with a mask (mflux's FLUX.1 Fill): it needs a start image
 * and a mask, white where the image is redrawn. */
export function takesMask(alias: string, definitions: ModelDefinition[], files: LocalModel[] = []): boolean {
  return MFLUX_INPAINT.includes(mfluxFile(alias, definitions, files)?.architecture ?? '');
}

/** Keep the user's selection while it is still available. */
export function imageModel(current: string, definitions: ModelDefinition[], files: LocalModel[] = []): string {
  const models = definitions.filter((d) => isImageDefinition(d, files));
  return models.some((d) => d.alias === current) ? current : models[0]?.alias ?? '';
}
