// Which definitions the Playground Image tab lists, which need a start image, which take a strength.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { isImageDefinition, isMflux, needsStartImage, takesStrength } = await import('../src/lib/components/playground/imageModels.ts');

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
