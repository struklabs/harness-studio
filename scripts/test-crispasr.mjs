import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isSpeechDefinition, dictationFamily, dictationRequirement, crispasrCanTranslate } from '../src/lib/components/playground/speechModels.ts';
import { speechKindKey, playgroundTarget } from '../src/lib/libraryDefinitions.ts';
import { recommendCrispasrVariant } from '../src/lib/components/hub/recommend.ts';
import { engineSpec, engineLabel } from '../src/lib/engines.ts';

test('CrispASR joins speech choices through validated definition metadata', () => {
  const def = { alias: 'data2vec', base: { kind: 'local', path: 'data2vec.gguf' }, load: { task: 'crispasr' } };
  assert.equal(isSpeechDefinition(def), true);
  assert.equal(isSpeechDefinition({ ...def, load: {} }), false);
  assert.equal(dictationFamily(def.alias, [], [def]), 'crispasr');
  assert.equal(dictationFamily('whisper'), 'whisper.cpp');
  assert.equal(dictationFamily('parakeet', ['parakeet']), 'sherpa-onnx');
  assert.equal(dictationFamily('ctc', [], [{ ...def, alias: 'ctc', load: { task: 'asr_ctc' } }]), 'onnxruntime');
  assert.equal(dictationRequirement('data2vec', [], true, [], [def]), 'runtime');
  assert.equal(dictationRequirement('data2vec', [{ available: true, entry: { family: 'crispasr' } }], true, [], [def]), null);
  assert.equal(engineSpec('crispasr').repo, 'CrispStrobe/CrispASR');
});

test('library labels and navigation use CrispASR speech models', () => {
  const file = { kind: 'asr', path: 'data2vec.gguf', architecture: 'wav2vec2', is_ready: true };
  assert.equal(speechKindKey(file), 'lib.kind_asr_crispasr');
  assert.equal(playgroundTarget(file).pick, 'definition');
});

test('quant recommendation respects memory and prefers Q5/Q4', () => {
  const variants = ['F16', 'Q8_0', 'Q5_K_M', 'Q4_K'].map((quantization, i) => ({ name: quantization, quantization, total_size_bytes: 800 - 150 * i }));
  assert.equal(recommendCrispasrVariant(variants, 1000).quantization, 'Q5_K_M');
  assert.equal(recommendCrispasrVariant(variants, 600).quantization, 'Q4_K');
  assert.equal(recommendCrispasrVariant(variants, 100), null);
  assert.equal(recommendCrispasrVariant(variants, null, 1000).quantization, 'Q5_K_M');
});

test('translation is offered only for translating architectures', () => {
  const def = { alias: 'm', base: { kind: 'local', path: 'm.gguf' }, load: { task: 'crispasr' } };
  assert.equal(crispasrCanTranslate(def, [{ path: 'm.gguf', architecture: 'canary' }]), true);
  assert.equal(crispasrCanTranslate(def, [{ path: 'm.gguf', architecture: 'wav2vec2' }]), false);
  assert.equal(crispasrCanTranslate(def, []), false);
  assert.equal(crispasrCanTranslate({ ...def, load: {} }, [{ path: 'm.gguf', architecture: 'canary' }]), false);
  assert.equal(engineLabel('crispasr'), 'CrispASR');
  assert.equal(engineLabel('whisper.cpp'), 'whisper.cpp');
});
