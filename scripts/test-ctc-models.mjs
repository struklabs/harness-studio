import { speechNeedsConversion } from '../src/lib/components/hub/convertible.ts';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isSpeechDefinition, isOnnxCtcDefinition, dictationFamily, dictationRequirement } from '../src/lib/components/playground/speechModels.ts';
import { isSherpaAsrFile } from '../src/lib/components/playground/asr.ts';
import { filesNeedingDefinition, playgroundTarget, speechKindKey } from '../src/lib/libraryDefinitions.ts';

const definition = (path, load = {}) => ({ base: { kind: 'local', path }, load });
test('speech labels distinguish Hub CTC, Sherpa ONNX and Whisper files', () => {
  const ctc = { kind: 'asr', path: 'Xenova/wav2vec2-base-960h/onnx/model.onnx', architecture: 'onnx' };
  assert.equal(speechKindKey(ctc), 'lib.kind_asr_ctc');
  assert.equal(speechKindKey({ ...ctc, architecture: 'nemo_ctc' }), 'sasr.kind');
  assert.equal(speechKindKey({ ...ctc, path: 'ggml-base.bin', architecture: 'whisper' }), 'lib.kind_asr_whisper');
  assert.equal(speechKindKey({ ...ctc, path: 'unknown.model' }), 'lib.kind_asr');
});
test('CTC definitions join Whisper speech choices, other ONNX tasks stay out', () => {
  const ctc = definition('speech/onnx/model.onnx', { task: 'asr_ctc' });
  assert.equal(isSpeechDefinition(ctc), true);
  assert.equal(isOnnxCtcDefinition(ctc), true);
  assert.equal(isSpeechDefinition(definition('ggml-base.bin')), true);
  for (const task of ['embedding', 'rerank', undefined]) {
    assert.equal(isSpeechDefinition(definition('onnx/model.onnx', { task })), false);
  }
});
test('CTC library files get definitions and open by alias; Sherpa opens by name', () => {
  const ctc = { kind: 'asr', path: 'speech/onnx/model.onnx', architecture: 'onnx', is_ready: true };
  const sherpa = { ...ctc, architecture: 'nemo_ctc' };
  assert.deepEqual(filesNeedingDefinition([ctc, sherpa], [], ['asr']), [ctc]);
  assert.equal(isSherpaAsrFile(ctc), false);
  assert.equal(isSherpaAsrFile(sherpa), true);
  assert.equal(playgroundTarget(ctc).pick, 'definition');
  assert.equal(playgroundTarget(sherpa).pick, 'name');
});

test('CTC dictation uses the managed ONNX engine without requiring Whisper or Sherpa', () => {
  const ctc = { ...definition('speech/onnx/model.onnx', { task: 'asr_ctc' }), alias: 'speech' };
  assert.equal(dictationFamily('speech', [], [ctc]), 'onnxruntime');
  assert.equal(dictationRequirement('speech', [], true, [], [ctc]), null);
  assert.equal(dictationRequirement('speech', [], false, [], [ctc]), 'gateway');
  assert.equal(dictationRequirement('whisper', [], true), 'runtime');
  assert.equal(dictationFamily('sherpa', ['sherpa']), 'sherpa-onnx');
});

test('source speech repos say conversion is needed; ONNX exports keep their runnable verdict', () => {
  const repo = { format: 'safetensors', inferred_task: 'automatic-speech-recognition', compatibility: { status: 'unsupported' } };
  assert.equal(speechNeedsConversion(repo), true);
  assert.equal(speechNeedsConversion({ ...repo, format: 'pytorch' }), true);
  assert.equal(speechNeedsConversion({ ...repo, format: 'onnx', compatibility: { status: 'runnable', runtime: 'onnxruntime' } }), false);
  assert.equal(speechNeedsConversion({ ...repo, inferred_task: 'text-generation' }), false);
});
