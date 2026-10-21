// Which definitions the Playground Image tab lists, which need a start image, which take a strength.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { isImageDefinition, isMflux, isUpscaleDefinition, loraOk, loraScaleOk, maskMode, needsMask, needsStartImage, takesLora, takesMask, takesStrength, upscaleModel } = await import('../src/lib/components/playground/imageModels.ts');

const file = (name, task, architecture) => ({ id: name, name, path: `/m/${name}`, kind: 'pipeline', task, architecture, is_ready: true });
const def = (alias) => ({ id: `md-${alias}`, alias, base: { kind: 'local', path: `/m/${alias}` } });

const files = [
  file('tiny-sd', 'text-to-image', 'StableDiffusionPipeline'),
  file('pix2pix', 'image-to-image', 'StableDiffusionInstructPix2PixPipeline'),
  file('refiner', 'image-to-image', 'StableDiffusionXLImg2ImgPipeline'),
  file('qwen-edit', 'image-to-image', 'QwenImageEditPlusPipeline'),
  file('ner', 'token-classification', 'BertForTokenClassification'),
];
const defs = ['tiny-sd', 'pix2pix', 'refiner', 'qwen-edit', 'ner'].map(def);

test('diffusers text-to-image and image-to-image folders draw; other pipelines do not', () => {
  assert.deepEqual(defs.filter((d) => isImageDefinition(d, files)).map((d) => d.alias), ['tiny-sd', 'pix2pix', 'refiner', 'qwen-edit']);
});

test('image-to-image models need a start image; only the Img2Img ones take a strength', () => {
  assert.deepEqual(defs.map((d) => needsStartImage(d.alias, defs, files)), [false, true, true, true, false]);
  assert.deepEqual(['tiny-sd', 'pix2pix', 'refiner', 'qwen-edit'].map((a) => takesStrength(a, defs, files)), [true, false, true, false]);
  assert.equal(needsStartImage('unknown', defs, files), false);
});

test('mflux FLUX.2 [klein] folders draw and edit; they are told apart from diffusers pipelines', () => {
  const klein = file('klein', 'text-to-image', 'flux2-klein-4b');
  const all = [...files, klein];
  const defsAll = [...defs, def('klein')];
  assert.equal(isImageDefinition(def('klein'), all), true);
  assert.equal(isMflux('klein', defsAll, all), true);
  assert.equal(needsStartImage('klein', defsAll, all), false);
  assert.equal(takesStrength('klein', defsAll, all), true);
  assert.deepEqual(['tiny-sd', 'qwen-edit', 'unknown'].map((a) => isMflux(a, defsAll, all)), [false, false, false]);
});

test('mflux Z-Image, FLUX.1 and Qwen-Image folders are mflux; FLUX.1 Fill inpaints from a start image', () => {
  const z = file('zimage', 'text-to-image', 'z-image-turbo');
  const schnell = file('schnell', 'text-to-image', 'schnell');
  const fill = file('fill', 'image-to-image', 'dev-fill');
  const all = [...files, z, schnell, fill];
  const defsAll = [...defs, def('zimage'), def('schnell'), def('fill')];
  assert.deepEqual(['zimage', 'schnell', 'fill'].map((a) => isMflux(a, defsAll, all)), [true, true, true]);
  assert.deepEqual(['zimage', 'schnell', 'fill', 'tiny-sd'].map((a) => needsMask(a, defsAll, all)), [false, false, true, false]);
  assert.deepEqual(['zimage', 'schnell', 'fill'].map((a) => takesMask(a, defsAll, all)), [false, false, true]);
  assert.deepEqual(['zimage', 'fill'].map((a) => needsStartImage(a, defsAll, all)), [false, true]);
  assert.equal(takesStrength('zimage', defsAll, all), true);
});

test('Python engine pipelines: inpainting checkpoints need a mask, text-to-image ones may take one, the editors take none', () => {
  const inpaint = file('sd-inpaint', 'image-to-image', 'StableDiffusionInpaintPipeline');
  const fill = file('flux-fill', 'image-to-image', 'FluxFillPipeline');
  const all = [...files, inpaint, fill];
  const defsAll = [...defs, def('sd-inpaint'), def('flux-fill')];
  const modes = (names) => names.map((a) => maskMode(a, defsAll, all));
  assert.deepEqual(modes(['tiny-sd', 'sd-inpaint', 'flux-fill', 'pix2pix', 'qwen-edit', 'ner', 'unknown']), ['optional', 'required', 'required', 'none', 'none', 'none', 'none']);
  assert.deepEqual(['tiny-sd', 'sd-inpaint', 'pix2pix'].map((a) => takesMask(a, defsAll, all)), [true, true, false]);
  assert.deepEqual(['tiny-sd', 'sd-inpaint', 'pix2pix'].map((a) => needsMask(a, defsAll, all)), [false, true, false]);
  // An inpainting checkpoint is an editor that needs a start image, and takes a strength (Fill has none).
  assert.deepEqual(['sd-inpaint', 'flux-fill'].map((a) => needsStartImage(a, defsAll, all)), [true, true]);
  assert.deepEqual(['sd-inpaint', 'flux-fill'].map((a) => takesStrength(a, defsAll, all)), [true, false]);
});

test('a LoRA per request is for Python engine diffusers pipelines only', () => {
  const klein = file('klein', 'text-to-image', 'flux2-klein-4b');
  const all = [...files, klein];
  const defsAll = [...defs, def('klein')];
  assert.deepEqual(['tiny-sd', 'pix2pix', 'qwen-edit', 'klein', 'ner', 'unknown'].map((a) => takesLora(a, defsAll, all)), [true, true, true, false, false, false]);
});

test('LoRA fields: an absolute .safetensors path or a repo id; the scale from -4 to 4', () => {
  for (const ok of ['', '  ', 'C:\\loras\\style.safetensors', 'C:/loras/style.safetensors', '/home/me/style.safetensors', 'owner/name', 'owner/name:dir/style.safetensors', 'a.b/c_d-e']) {
    assert.equal(loraOk(ok), true, ok);
  }
  for (const bad of ['style.safetensors', 'C:\\loras\\style.bin', '/home/me/style', 'owner', 'a/b/c', '../x/y', 'owner/name:x.bin', 'owner/name:../x.safetensors', '-owner/name']) {
    assert.equal(loraOk(bad), false, bad);
  }
  assert.deepEqual(['', '1', '-4', '4', '0.5'].map(loraScaleOk), [true, true, true, true, true]);
  assert.deepEqual(['4.5', '-5', 'x'].map(loraScaleOk), [false, false, false]);
});

test('super-resolution folders are the upscale models, and the selection survives while it exists', () => {
  const swin = file('swin2sr', 'image-super-resolution', 'Swin2SRForImageSuperResolution');
  const other = file('swin2sr-x4', 'image-super-resolution', 'Swin2SRForImageSuperResolution');
  const all = [...files, swin, other];
  const defsAll = [...defs, def('swin2sr'), def('swin2sr-x4')];
  assert.deepEqual(defsAll.filter((d) => isUpscaleDefinition(d, all)).map((d) => d.alias), ['swin2sr', 'swin2sr-x4']);
  // They do not draw, so the Image tab does not list them as image models.
  assert.equal(isImageDefinition(def('swin2sr'), all), false);
  assert.equal(upscaleModel('swin2sr-x4', defsAll, all), 'swin2sr-x4');
  assert.equal(upscaleModel('gone', defsAll, all), 'swin2sr');
  assert.equal(upscaleModel('x', defs, files), '');
});
