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

/** Keep the user's selection while it is still available. */
export function imageModel(current: string, definitions: ModelDefinition[], files: LocalModel[] = []): string {
  const models = definitions.filter((d) => isImageDefinition(d, files));
  return models.some((d) => d.alias === current) ? current : models[0]?.alias ?? '';
}
