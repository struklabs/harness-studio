import assert from 'node:assert/strict';
import { test } from 'node:test';
import { registerHooks } from 'node:module';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

registerHooks({ resolve(specifier, context, next) {
  if (specifier.startsWith('./') && context.parentURL && !specifier.endsWith('.ts')) {
    const url = new URL(`${specifier}.ts`, context.parentURL);
    if (existsSync(fileURLToPath(url))) return next(url.href, context);
  }
  return next(specifier, context);
} });

const { ttsRequirement, ttsRunBlocker, pickTtsModel, pickTtsVoice, voicesOf, ttsSupportsSpeed, fetchSpeech } = await import('../src/lib/components/playground/tts.ts');
const models = [
  { name: 'pocket-en', family: 'pocket_tts', engine: 'audio.cpp', supports_speed: false, runtime_installed: true, voices: [{ id: 'alba' }, { id: 'anna' }] },
  { name: 'piper', family: 'vits', engine: 'sherpa-onnx', runtime_installed: false, voices: [{ id: '0' }] },
];

test('TTS uses the selected model engine, including audio.cpp without sherpa', () => {
  assert.equal(ttsRequirement(models, 'pocket-en', false), null);
  assert.equal(ttsRequirement(models, 'piper', true), 'sherpa-onnx');
  assert.equal(ttsRequirement([{ ...models[0], runtime_installed: false }], 'pocket-en', true), 'audio.cpp');
});

test('Speech run blocker explains prerequisites and clears after they are resolved', () => {
  assert.equal(ttsRunBlocker(false, !!ttsRequirement(models, 'piper', false), true, models.length, 'Hello'), 'runtime');
  assert.equal(ttsRunBlocker(false, true, false, 0, ''), 'runtime');
  assert.equal(ttsRunBlocker(false, false, false, 0, ''), 'gateway');
  assert.equal(ttsRunBlocker(false, false, true, 0, ''), 'model');
  assert.equal(ttsRunBlocker(false, false, true, 1, ' \n\t'), 'text');
  assert.equal(ttsRunBlocker(false, false, true, 1, ' Hello '), null);
  assert.equal(ttsRunBlocker(false, !!ttsRequirement(models, 'pocket-en', false), true, models.length, 'Hello'), null);
  assert.equal(ttsRunBlocker(true, true, false, 0, ''), 'busy');
  assert.equal(ttsRunBlocker(true, false, true, 1, 'Hello'), 'busy');
});

test('audio.cpp aliases and embedding voices survive picker refreshes', () => {
  assert.equal(pickTtsModel('pocket-en', models), 'pocket-en');
  assert.equal(pickTtsModel('missing', models), 'pocket-en');
  assert.deepEqual(voicesOf(models, 'pocket-en').map(v => v.id), ['alba', 'anna']);
  assert.equal(pickTtsVoice('anna', models, 'pocket-en'), 'anna');
  assert.equal(pickTtsVoice('missing', models, 'pocket-en'), '');
});


test('picker speed support follows the selected model', () => {
  assert.equal(ttsSupportsSpeed(models, 'pocket-en'), false);
  assert.equal(ttsSupportsSpeed(models, 'piper'), true);
  assert.equal(ttsSupportsSpeed([{ ...models[0], supports_speed: true }], 'pocket-en'), true);
});

test('speech omits default speed and preserves non-default speed', async () => {
  const original = globalThis.fetch;
  const bodies = [];
  globalThis.fetch = async (_url, options) => {
    bodies.push(JSON.parse(options.body));
    return new Response(new Blob(['wav']), { status: 200 });
  };
  try {
    for (const speed of [1, 1.25]) {
      await fetchSpeech('http://example.invalid/v1', { model: 'pocket-en', input: 'Hi', voice: '', speed }, {});
    }
    assert.equal(Object.hasOwn(bodies[0], 'speed'), false);
    assert.equal(bodies[1].speed, 1.25);
  } finally { globalThis.fetch = original; }
});
