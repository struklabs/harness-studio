import { test } from 'node:test';
import assert from 'node:assert/strict';
import { canClone, cloneBlocker, cloneFields, voiceRefOf, MAX_SAMPLE_BYTES } from '../src/lib/components/playground/voiceClone.ts';

const models = [
  { name: 'pocket', voice_cloning: true },
  { name: 'kokoro', voice_cloning: false },
  { name: 'piper' },
];

test('only models with the voice_cloning capability show the clone section', () => {
  assert.equal(canClone(models, 'pocket'), true);
  assert.equal(canClone(models, 'kokoro'), false);
  assert.equal(canClone(models, 'piper'), false);
  assert.equal(canClone(models, 'missing'), false);
});

test('a clone request needs a sample, a small file and the permission box, in that order', () => {
  const file = (size) => ({ size });
  assert.equal(cloneBlocker(null, true), 'sample');
  assert.equal(cloneBlocker(file(MAX_SAMPLE_BYTES + 1), true), 'size');
  assert.equal(cloneBlocker(file(1000), false), 'consent');
  assert.equal(cloneBlocker(file(1000), true), null);
});

test('the request carries the base64 sample, the transcript and consent', async () => {
  const bytes = new Uint8Array([82, 73, 70, 70, 1, 2, 3]);
  const ref = await voiceRefOf(new Blob([bytes]), '  hello ');
  assert.equal(ref.data, Buffer.from(bytes).toString('base64'));
  assert.equal(ref.text, 'hello');
  assert.deepEqual(cloneFields(ref), {
    voice_ref: { type: 'base64', data: ref.data },
    voice_consent: true,
    voice_ref_text: 'hello',
  });
  assert.equal('voice_ref_text' in cloneFields({ data: 'AA==', text: '' }), false);
});
