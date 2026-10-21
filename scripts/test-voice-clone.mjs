import { test } from 'node:test';
import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { canClone, cloneBlocker, cloneFields, cloneNeedsText, voiceRefOf, MAX_SAMPLE_BYTES } from '../src/lib/components/playground/voiceClone.ts';

// tts.ts imports its neighbours without the `.ts` extension, as Vite allows.
registerHooks({ resolve(specifier, context, next) {
  if (specifier.startsWith('./') && context.parentURL && !specifier.endsWith('.ts')) {
    const url = new URL(`${specifier}.ts`, context.parentURL);
    if (existsSync(fileURLToPath(url))) return next(url.href, context);
  }
  return next(specifier, context);
} });
const { allTtsModels } = await import('../src/lib/components/playground/tts.ts');

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

test('engine models keep their voice_cloning capability (mlx-audio Qwen3-TTS Base)', () => {
  const engine = (name, extra) => ({ id: `/m/${name}`, name, family: 'mlx-audio', size_bytes: 1, voices: [], ...extra });
  const models = allTtsModels({
    models: [],
    catalog: [],
    runtime_installed: false,
    engine_models: [
      engine('qwen3-tts-base', { voice_cloning: true, voice_clone_needs_text: true }),
      engine('chatterbox', { voice_cloning: true, voice_clone_needs_text: false }),
      engine('soprano', {}),
      engine('custom', { voice_cloning: false }),
    ],
  });
  assert.equal(canClone(models, 'qwen3-tts-base'), true);
  assert.equal(cloneNeedsText(models, 'qwen3-tts-base'), true);
  assert.equal(canClone(models, 'chatterbox'), true);
  assert.equal(cloneNeedsText(models, 'chatterbox'), false);
  assert.equal(canClone(models, 'soprano'), false);
  assert.equal(canClone(models, 'custom'), false);
});

test('a clone request needs a sample, a small file and the permission box, in that order', () => {
  const file = (size) => ({ size });
  assert.equal(cloneBlocker(null, true), 'sample');
  assert.equal(cloneBlocker(file(MAX_SAMPLE_BYTES + 1), true), 'size');
  assert.equal(cloneBlocker(file(1000), false), 'consent');
  assert.equal(cloneBlocker(file(1000), true), null);
  // A model that needs the transcript waits for it before the permission box.
  assert.equal(cloneBlocker(file(1000), false, true, '  '), 'text');
  assert.equal(cloneBlocker(file(1000), false, true, 'hi'), 'consent');
  assert.equal(cloneBlocker(file(1000), true, true, 'hi'), null);
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
