// Which scanned library files still need a default definition, and where "Open in Playground" goes.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { filesNeedingDefinition, kindsForTab, playgroundTarget } = await import('../src/lib/libraryDefinitions.ts');

const file = (name, kind, extra = {}) => ({ id: name, name, path: `C:\\Models\\${name}`, kind, is_ready: true, ...extra });
const def = (path) => ({ id: 'md-1', alias: 'x', base: { kind: 'local', path } });

test('ready scanned files of the asked kinds without a definition are listed', () => {
  const files = [
    file('sd_turbo-f16-q8_0.gguf', 'image'),
    file('ggml-base.bin', 'asr'),
    file('bge-m3.gguf', 'embedding'),
    file('chat.gguf', 'llm'),
  ];
  const all = ['image', 'asr', 'embedding', 'reranker'];
  assert.deepEqual(filesNeedingDefinition(files, [], all).map((f) => f.name), ['sd_turbo-f16-q8_0.gguf', 'ggml-base.bin', 'bge-m3.gguf']);
  assert.deepEqual(filesNeedingDefinition(files, [], ['image']).map((f) => f.name), ['sd_turbo-f16-q8_0.gguf']);
});

test('a file that has a definition is not listed (paths compare like the library does)', () => {
  const files = [file('ggml-base.bin', 'asr')];
  assert.deepEqual(filesNeedingDefinition(files, [def('c:/models/GGML-BASE.bin')], ['asr']), []);
});

test('incomplete files, sherpa-onnx recognition and text-to-speech need no definition', () => {
  const files = [file('half.bin', 'asr', { is_ready: false }), file('parakeet.onnx', 'asr'), file('kokoro', 'tts')];
  assert.deepEqual(filesNeedingDefinition(files, [], ['asr', 'tts']), []);
});

test('tabs list their own kinds', () => {
  assert.deepEqual(kindsForTab('image'), ['image']);
  assert.deepEqual(kindsForTab('transcription'), ['asr']);
  assert.deepEqual(kindsForTab('rerank'), ['reranker']);
  assert.deepEqual(kindsForTab('pipeline'), ['pipeline']);
  assert.deepEqual(kindsForTab('chat'), []);
});

test('Open in Playground picks the tab and how the model is chosen', () => {
  assert.deepEqual(playgroundTarget(file('a.gguf', 'image')), { tab: 'image', pick: 'definition' });
  assert.equal(playgroundTarget(file('ggml-base.bin', 'asr')).pick, 'definition');
  assert.equal(playgroundTarget(file('parakeet.onnx', 'asr')).pick, 'name');
  assert.equal(playgroundTarget(file('kokoro', 'tts')).speech, 'speak');
  assert.equal(playgroundTarget(file('e.gguf', 'embedding')).tab, 'embedding');
  assert.equal(playgroundTarget(file('r.gguf', 'reranker')).tab, 'rerank');
  assert.equal(playgroundTarget(file('chat.gguf', 'llm')), null);
  assert.equal(playgroundTarget(file('a.gguf', 'image', { is_ready: false })), null);
});
