import assert from 'node:assert/strict';
import test from 'node:test';
import { isVideoTask, videoQuestion } from '../src/lib/components/playground/video.ts';
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
