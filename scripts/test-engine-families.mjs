// Engine families and the helpers behind the one-click engine installer.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { ENGINES, engineSpec, familyFromError, familyInstalled, progressPercent } = await import('../src/lib/engines.ts');

test('every github family has an official repository', () => {
  for (const e of ENGINES.filter((x) => x.method === 'github')) assert.match(e.repo ?? '', /^[\w.-]+\/[\w.-]+$/, e.family);
  assert.equal(new Set(ENGINES.map((e) => e.family)).size, ENGINES.length);
});

test('families a model can need are all known', () => {
  for (const f of ['whisper.cpp', 'stable-diffusion.cpp', 'sherpa-onnx', 'onnxruntime', 'lemonade', 'mistral.rs']) {
    assert.ok(engineSpec(f), f);
  }
  assert.equal(engineSpec('mistral.rs').method, 'manual');
  assert.equal(engineSpec('onnxruntime').method, 'onnx');
  assert.equal(engineSpec('lemonade').method, 'lemonade');
  assert.equal(engineSpec('nope'), undefined);
});

test('family comes out of the raw backend failure', () => {
  assert.equal(familyFromError('load failed: runtime.family_not_installed: whisper.cpp'), 'whisper.cpp');
  assert.equal(familyFromError('runtime.family_not_installed:sherpa-onnx\n'), 'sherpa-onnx');
  assert.equal(familyFromError('something else'), null);
});

test('installed state needs an available runtime of the family', () => {
  const rt = (family, available = true) => ({ available, entry: { family } });
  assert.equal(familyInstalled('whisper.cpp', [rt('whisper.cpp')]), true);
  assert.equal(familyInstalled('whisper.cpp', [rt('whisper.cpp', false)]), false);
  assert.equal(familyInstalled('whisper.cpp', [rt('llama.cpp'), { available: true, entry: null }]), false);
});

test('progress percent', () => {
  assert.equal(progressPercent(5, 0), null);
  assert.equal(progressPercent(50, 200), 25);
  assert.equal(progressPercent(300, 200), 100);
});
