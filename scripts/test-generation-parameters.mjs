import assert from 'node:assert/strict';
import test from 'node:test';
import { QUALITIES, generationPreset, effectiveFrames, videoDuration, videoFormats, validSize, imageParameterErrors, inRange, optionalNumber, matchingEstimate, trainingPreset, audioPresetSteps } from '../src/lib/components/playground/generationParameters.ts';

const defaults = { size: '', steps: '', cfgScale: '', seed: '', count: 1 };
test('every image and video preset stays within its engine limits', () => {
  for (const engine of ['sdcpp', 'diffusers', 'mflux', 'comfyui', 'unknown']) {
    for (const model of ['SD 1.5', 'SDXL', 'FLUX schnell', 'FLUX klein base', 'Wan2.1', 'LTX']) {
      for (const quality of QUALITIES) {
        const image = generationPreset('image', engine, quality, model);
        assert.deepEqual(imageParameterErrors({ ...defaults, size: image.size, steps: String(image.steps) }, engine), { size: false, steps: false, cfg: false, seed: false, count: false });
        const video = generationPreset('video', engine, quality, model);
        assert.equal(validSize(video.size, engine, false), true);
        assert.equal(effectiveFrames(video.frames), video.frames);
        assert.equal(effectiveFrames(video.frames, true), video.frames);
      }
    }
  }
});
test('presets increase workload and respect distilled models', () => {
  const fast = generationPreset('video', 'sdcpp', 'fast');
  const balanced = generationPreset('video', 'sdcpp', 'balanced');
  const quality = generationPreset('video', 'sdcpp', 'quality');
  assert.ok(fast.steps < balanced.steps && balanced.steps < quality.steps);
  assert.ok(fast.frames < balanced.frames && balanced.frames < quality.frames);
  assert.equal(generationPreset('image', 'mflux', 'balanced', 'schnell').steps, 6);
  assert.equal(generationPreset('image', 'mflux', 'balanced', 'klein-base').steps, 20);
  assert.equal(generationPreset('image', 'diffusers', 'quality', 'SDXL').size, '1024x1024');
});
test('duration uses encoded frame count divided by fps including the initial frame', () => {
  assert.equal(videoDuration(33, 16), 2.0625);
  assert.equal(videoDuration(1, 16), 0.0625);
  assert.equal(videoDuration(60, 60), 1);
  assert.equal(effectiveFrames(32), 29);
  assert.equal(effectiveFrames(32, true), 25);
  assert.equal(videoDuration(effectiveFrames(32, true), 16), 1.5625);
  for (const value of [0, -1, NaN, Infinity, 1.5, undefined]) {
    assert.equal(videoDuration(value, 16), null);
    assert.equal(videoDuration(9, value), null);
    assert.equal(effectiveFrames(value), 0);
  }
  assert.equal(videoDuration(9, 61), null);
  assert.equal(effectiveFrames(258), 0);
  assert.equal(effectiveFrames(257, true), 257);
});
test('formats mirror sd.cpp and diffusers rather than offering unsupported MP4', () => {
  assert.deepEqual(videoFormats('sdcpp'), ['webm', 'webp', 'avi']);
  assert.deepEqual(videoFormats('diffusers'), ['mp4', 'webm']);
  assert.deepEqual(videoFormats('unknown'), ['webm']);
});
test('image validation rejects unsafe values and applies mflux and ComfyUI limits', () => {
  assert.equal(validSize('520x512', 'sdcpp'), true);
  assert.equal(validSize('520x512', 'mflux'), false);
  assert.equal(validSize('4096x4096', 'sdcpp'), true);
  assert.equal(validSize('4096x4096', 'mflux'), false);
  assert.equal(validSize('8192x8192', 'comfyui'), true);
  assert.equal(validSize('8192x8192', 'sdcpp'), false);
  assert.equal(validSize('', 'sdcpp', false), false);
  for (const size of ['0x0', '512', '512xNaN', '513x512', 'Infinityx512']) assert.equal(validSize(size, 'sdcpp'), false);
  assert.equal(imageParameterErrors({ ...defaults, steps: '51' }, 'mflux').steps, true);
  assert.equal(imageParameterErrors({ ...defaults, steps: '201' }, 'comfyui').steps, true);
  assert.equal(imageParameterErrors({ ...defaults, steps: '500' }, 'sdcpp').steps, false);
  assert.equal(imageParameterErrors({ ...defaults, count: 5 }, 'mflux').count, true);
  assert.equal(imageParameterErrors({ ...defaults, seed: '4294967296' }, 'mflux').seed, true);
  assert.equal(imageParameterErrors({ ...defaults, cfgScale: '101' }, 'diffusers').cfg, true);
  for (const seed of ['1.1', 'NaN', '-2', '9007199254740992']) assert.equal(imageParameterErrors({ ...defaults, seed }, 'sdcpp').seed, true);
  assert.equal(optionalNumber(' ', 1, 500, true), true);
  assert.equal(optionalNumber('1.5', 1, 500, true), false);
  assert.equal(inRange(undefined, 1, 500), false);
});
test('estimates use a matching measured workload, never unrelated model speed', () => {
  const samples = [{ model: 'a', signature: 'fast', ms: 5000 }, { model: 'a', signature: 'fast', ms: 6000 }, { model: 'b', signature: 'fast', ms: 1000 }];
  assert.equal(matchingEstimate(samples, 'a', 'fast'), 6);
  assert.equal(matchingEstimate(samples, 'a', 'quality'), null);
  assert.equal(matchingEstimate([], 'a', 'fast'), null);
  assert.equal(matchingEstimate([{ model: 'a', signature: 'fast', ms: NaN }], 'a', 'fast'), null);
});
test('training and engine-described audio presets remain usable within bounds', () => {
  assert.deepEqual(trainingPreset('balanced'), { steps: 100, rank: 16, sequenceLength: 512 });
  assert.ok(trainingPreset('fast').steps < trainingPreset('quality').steps);
  assert.equal(audioPresetSteps('fast', 10, 100), 10);
  assert.equal(audioPresetSteps('quality', 1, 16), 16);
  assert.equal(audioPresetSteps('balanced', 1, 100), 20);
});
