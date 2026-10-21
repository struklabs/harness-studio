// Playground studios: the list, resource facts, memory estimate, deep links and video job helpers.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { STUDIOS, STUDIO_STARTERS, engineOfFile, studioById, studioFromHash, studioMemoryEstimate, studioResources } = await import('../src/lib/components/playground/studios.ts');
const { VIDEO_FORMATS, jobPercent, videoMime, videoPreview, waitForVideo } = await import('../src/lib/components/playground/video.ts');

const MB = 1024 * 1024;
const file = (kind, path, size = 1000 * MB, task) => ({ id: path, name: path, path, kind, task, size_bytes: size });

test('the eight studios are listed once each and reuse an existing Playground mode', () => {
  assert.deepEqual(STUDIOS.map(s => s.id), ['image', 'audio', 'video', 'voice', 'document', 'coding', 'analysis', 'agent']);
  assert.equal(new Set(STUDIOS.map(s => s.id)).size, STUDIOS.length);
  for (const s of STUDIOS) assert.ok(['chat', 'image', 'transcription', 'pipeline', 'ocr'].includes(s.mode), s.id);
  assert.equal(studioById('video').mode, 'pipeline');
  assert.equal(studioById('video').api, '/v1/videos, /v1/chat/completions');
  assert.equal(studioById('nope'), undefined);
  assert.ok(STUDIO_STARTERS.coding && STUDIO_STARTERS.analysis);
});

test('memory estimate scales the file and grows with video frames and size', () => {
  const image = studioById('image');
  assert.equal(studioMemoryEstimate(image, 1000), 1300);
  assert.equal(studioMemoryEstimate(image, 0), null);
  assert.equal(studioMemoryEstimate(image, undefined), null);
  const video = studioById('video');
  const small = studioMemoryEstimate(video, 1000 * MB, { frames: 9, size: '256x256' });
  const large = studioMemoryEstimate(video, 1000 * MB, { frames: 81, size: '832x480' });
  assert.ok(large > small);
  assert.ok(large <= 1000 * MB * (1.6 + 2));
  assert.equal(studioMemoryEstimate(video, 1000 * MB, { frames: 9, size: 'bad' }), studioMemoryEstimate(video, 1000 * MB, {}));
});

test('resources name the engine, where it runs and the estimate; remote and cloud models have none', () => {
  const image = studioById('image');
  const local = studioResources(image, 'sdxl', file('image', '/m/sdxl.safetensors', 2000 * MB));
  assert.deepEqual([local.engine, local.location, local.sizeBytes], ['stable-diffusion.cpp', 'local', 2000 * MB]);
  assert.equal(local.memoryBytes, Math.round(2000 * MB * 1.3));
  // A definition's companions (Wan's umt5 and VAE) load with it and count toward the estimate.
  const wan = studioResources(studioById('video'), 'wan', file('image', '/m/wan.gguf', 1000 * MB), { companionBytes: 3000 * MB });
  assert.equal(wan.sizeBytes, 4000 * MB);
  assert.ok(wan.memoryBytes > 4000 * MB);
  const remote = studioResources(image, 'remote:box/sdxl', undefined);
  assert.deepEqual([remote.location, remote.engine, remote.memoryBytes], ['remote', null, null]);
  assert.equal(studioResources(image, 'cloud:x/y', undefined).location, 'cloud');
  assert.equal(studioResources(image, '', undefined).memoryBytes, null);
  assert.equal(engineOfFile(file('asr', '/m/ggml-base.bin')), 'whisper.cpp');
  assert.equal(engineOfFile(file('pipeline', '/m/wan', 1, 'text-to-video')), 'Python (diffusers)');
  assert.equal(engineOfFile(file('pipeline', '/m/bert', 1, 'fill-mask')), 'Python (transformers)');
  assert.equal(engineOfFile(file('llm', '/m/q.gguf')), 'llama.cpp');
  // The registry's runtime wins: an MLX folder's kind and task look like llama.cpp's or diffusers'.
  assert.equal(studioResources(image, 'klein', file('pipeline', '/m/klein', MB, 'text-to-image'), { runtime: 'mflux' }).engine, 'mflux');
  assert.equal(studioResources(image, 'vlm', file('llm', '/m/smolvlm', MB), { runtime: 'mlx-vlm' }).engine, 'MLX-VLM');
  assert.equal(studioResources(image, 'q', file('llm', '/m/q.gguf', MB), { runtime: null }).engine, 'llama.cpp');
  assert.equal(engineOfFile(undefined), null);
});

test('deep links open the studio, defaulting to image', () => {
  assert.equal(studioFromHash('#playground/studio/video'), 'video');
  assert.equal(studioFromHash('#playground/studio'), 'image');
  assert.equal(studioFromHash('#playground/studio/unknown'), 'image');
  assert.equal(studioFromHash('#playground/chat'), undefined);
});

test('video formats match the gateway and each has a preview and media type', () => {
  assert.deepEqual([...VIDEO_FORMATS], ['mp4', 'webm', 'webp', 'avi']);
  assert.deepEqual(VIDEO_FORMATS.map(videoPreview), ['video', 'video', 'image', 'file']);
  assert.deepEqual(VIDEO_FORMATS.map(videoMime), ['video/mp4', 'video/webm', 'image/webp', 'video/x-msvideo']);
  assert.equal(jobPercent({ progress: 250 }), 100);
  assert.equal(jobPercent({ progress: -3 }), 0);
  assert.equal(jobPercent({}), 0);
  assert.equal(jobPercent({ progress: 41.6 }), 42);
});

test('waitForVideo reports progress until completed, throws on failure and on abort', async () => {
  const sleep = async () => {};
  const steps = [{ status: 'queued', progress: 0 }, { status: 'in_progress', progress: 40 }, { status: 'completed', progress: 100 }];
  const seen = [];
  let i = 0;
  await waitForVideo(async () => steps[i++], { signal: new AbortController().signal, sleep, onProgress: (p, s) => seen.push([p, s]) });
  assert.deepEqual(seen, [[0, 'queued'], [40, 'in_progress'], [100, 'completed']]);
  await assert.rejects(waitForVideo(async () => ({ status: 'failed', error: { message: 'out of memory' } }), { signal: new AbortController().signal, sleep }), /out of memory/);
  const stop = new AbortController();
  let polls = 0;
  await assert.rejects(waitForVideo(async () => { polls++; stop.abort(); return { status: 'in_progress', progress: 1 }; }, { signal: stop.signal, sleep }));
  assert.equal(polls, 1);
});
