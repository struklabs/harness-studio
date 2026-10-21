import assert from 'node:assert/strict';
import test from 'node:test';
import { definitionVideoTask, fileVideoTask, isVideoTask, videoFormatsFor, videoQuestion } from '../src/lib/components/playground/video.ts';

test('GGUF vision definitions also offer video questions, while audio projectors and text models do not', () => {
  const def = { base: { kind: 'local', path: '/m/qwen.gguf' }, mmproj: '/m/mmproj.gguf' };
  const file = { kind: 'llm', path: '/m/qwen.gguf' };
  assert.equal(definitionVideoTask(def, file), 'video-text-to-text');
  assert.equal(definitionVideoTask({ ...def, mmproj: null }, file), undefined);
  assert.equal(definitionVideoTask(def, file, [{ path: '/m/mmproj.gguf', gguf: { has_audio_encoder: true, has_vision_encoder: false } }]), undefined);
  assert.equal(definitionVideoTask({ ...def, base: { kind: 'local', path: '/m/text-folder' } }, file), undefined);
  assert.equal(definitionVideoTask(def, { kind: 'pipeline', task: 'text-to-video' }), 'text-to-video');
});
test('video tasks use the video surface; image and other pipelines do not', () => {
  for (const task of ['text-to-video', 'image-to-video', 'video-text-to-text']) assert.equal(isVideoTask(task), true);
  for (const task of [undefined, 'text-to-image', 'image-to-text']) assert.equal(isVideoTask(task), false);
});
test('video questions use non-streaming chat and inline video content', () => {
  const body = videoQuestion('smol', 'What moves?', 'data:video/mp4;base64,AAAA');
  assert.equal(body.stream, false);
  assert.equal(body.model, 'smol');
  assert.deepEqual(body.messages[0].content, [
    { type: 'text', text: 'What moves?' },
    { type: 'video_url', video_url: { url: 'data:video/mp4;base64,AAAA' } },
  ]);
});

test('sd.cpp video GGUFs get a video task; image GGUFs and LLMs do not', () => {
  const gguf = (name, arch, kind = 'image') => ({ name, path: `/m/${name}.gguf`, kind, gguf: { architecture: arch } });
  assert.equal(fileVideoTask(gguf('Wan2.1-T2V-1.3B-Q4_K_M', 'wan')), 'text-to-video');
  assert.equal(fileVideoTask(gguf('Wan2.1-I2V-14B-480P-Q4_K_M', 'wan')), 'image-to-video');
  assert.equal(fileVideoTask(gguf('Wan2.2-TI2V-5B-Q4_K_M', 'wan')), 'text-to-video');
  assert.equal(fileVideoTask(gguf('ltx-video-2b', 'ltxv')), 'text-to-video');
  assert.equal(fileVideoTask(gguf('flux1-schnell', 'flux')), undefined);
  assert.equal(fileVideoTask(gguf('umt5-xxl-encoder', 't5encoder', 'llm')), undefined);
  assert.equal(fileVideoTask({ kind: 'pipeline', task: 'image-to-video' }), 'image-to-video');
  assert.equal(fileVideoTask({ kind: 'pipeline', task: 'text-to-image' }), undefined);
  assert.equal(fileVideoTask(undefined), undefined);
});

test('the format list holds only what the model engine writes', () => {
  const wan = { kind: 'image', name: 'Wan2.1-T2V-1.3B-Q8_0', gguf: { architecture: 'wan' } };
  assert.deepEqual(videoFormatsFor(wan), ['webm', 'webp', 'avi']);
  assert.deepEqual(videoFormatsFor({ kind: 'pipeline', task: 'text-to-video' }), ['mp4', 'webm']);
  assert.deepEqual(videoFormatsFor(undefined), ['mp4', 'webm']);
});
