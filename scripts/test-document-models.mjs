import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readsDocuments } from '../src/lib/components/playground/documentModels.ts';

const def = (alias, path, mmproj) => ({ id: alias, alias, base: { kind: 'local', path }, load: {}, ...(mmproj ? { mmproj } : {}) });

test('Read Document offers GGUF vision models, MLX vision folders and OCR pipelines only', () => {
  const files = [
    { path: '/m/mlx-community/SmolVLM-256M-Instruct-8bit', kind: 'llm', vision: true },
    { path: '/m/mlx-community/Qwen3-0.6B-4bit', kind: 'llm' },
    { path: '/m/smolvlm.gguf', kind: 'llm' },
    { path: '/m/mmproj-smolvlm.gguf', kind: 'mmproj', gguf: { has_vision_encoder: true } },
    { path: '/m/trocr', kind: 'pipeline', task: 'image-to-text' },
    { path: '/m/layoutlm', kind: 'pipeline', task: 'document-question-answering' },
  ];
  const definitions = [
    def('mlx-vlm', '/m/mlx-community/SmolVLM-256M-Instruct-8bit'),
    def('mlx-text', '/m/mlx-community/Qwen3-0.6B-4bit'),
    def('gguf-vlm', '/m/smolvlm.gguf', '/m/mmproj-smolvlm.gguf'),
    def('trocr', '/m/trocr'),
    def('layoutlm', '/m/layoutlm'),
  ];
  const fileOf = (d) => files.find((f) => f.path === d.base.path);
  assert.deepEqual(definitions.filter((d) => readsDocuments(d, fileOf(d), files)).map((d) => d.alias), ['mlx-vlm', 'gguf-vlm', 'trocr', 'layoutlm']);
  // A projector that only hears (an audio encoder) does not read pages.
  const audio = [...files, { path: '/m/mmproj-audio.gguf', kind: 'mmproj', gguf: { has_audio_encoder: true, has_vision_encoder: false } }];
  assert.equal(readsDocuments(def('a', '/m/smolvlm.gguf', '/m/mmproj-audio.gguf'), audio[2], audio), false);
});
