import assert from 'node:assert/strict';
import { test } from 'node:test';
import { needsSmallModelWave, pickBest, smallQueries } from '../src/lib/components/onboarding/suggestPlan.ts';

test('small-model wave runs when nothing fits or only weak tiers do', () => {
  assert.equal(needsSmallModelWave([]), true);
  assert.equal(needsSmallModelWave(['hybrid', 'cpu_heavy']), true);
  assert.equal(needsSmallModelWave(['hybrid', 'tight']), false);
  assert.equal(needsSmallModelWave(['full_gpu']), false);
});

test('small queries are role specific', () => {
  assert.ok(smallQueries('chat').includes('3B'));
  assert.ok(smallQueries('code').every((q) => q.startsWith('coder-')));
});

test('pickBest prefers the better fit tier, then the bigger file', () => {
  const a = { id: 'a', category: 'hybrid', sizeBytes: 9 };
  const b = { id: 'b', category: 'full_gpu', sizeBytes: 2 };
  const c = { id: 'c', category: 'full_gpu', sizeBytes: 3 };
  assert.equal(pickBest([a, b, c]).id, 'c');
  assert.equal(pickBest([a]).id, 'a');
  assert.equal(pickBest([]), null);
});

test('pickBest ranks tool evidence first for code', () => {
  const a = { id: 'a', category: 'full_gpu', sizeBytes: 5, rank: 2 };
  const b = { id: 'b', category: 'hybrid', sizeBytes: 1, rank: 0 };
  assert.equal(pickBest([a, b], (x) => x.rank).id, 'b');
});

import { isGeneralChatRepo, isKnownChatFamily, pickChat } from '../src/lib/components/onboarding/suggestPlan.ts';

const GIB = 1024 ** 3;

test('chat skips coder, math, embedding, reranker and vision repos', () => {
  for (const id of ['bartowski/DeepSeek-Coder-V2-Lite-Instruct-GGUF', 'unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF', 'bartowski/Qwen2.5-Math-7B-GGUF', 'x/bge-embedding-GGUF', 'x/bge-reranker-v2-GGUF', 'unsloth/Qwen2.5-VL-7B-Instruct-GGUF', 'x/Llama-3.2-11B-Vision-GGUF', 'bartowski/codegemma-7b-it-GGUF']) {
    assert.equal(isGeneralChatRepo(id), false, id);
  }
  for (const id of ['unsloth/Qwen3-4B-Instruct-2507-GGUF', 'unsloth/gemma-3-4b-it-GGUF', 'bartowski/Llama-3.2-3B-Instruct-GGUF', 'bartowski/Ministral-8B-Instruct-2410-GGUF', 'unsloth/Phi-4-mini-instruct-GGUF']) {
    assert.equal(isKnownChatFamily(id), true, id);
  }
  assert.equal(isKnownChatFamily('unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF'), false);
});

test('chat picks a modest known instruct model, not the 9 GB coder', () => {
  const found = [
    { repoId: 'bartowski/DeepSeek-Coder-V2-Lite-Instruct-GGUF', category: 'full_gpu', sizeBytes: 8.9 * GIB },
    { repoId: 'unsloth/Qwen3-4B-Instruct-2507-GGUF', category: 'full_gpu', sizeBytes: 2.5 * GIB },
    { repoId: 'unsloth/Qwen3-8B-GGUF', category: 'full_gpu', sizeBytes: 5 * GIB },
    { repoId: 'unsloth/gemma-3-27b-it-GGUF', category: 'full_gpu', sizeBytes: 16 * GIB },
    { repoId: 'someone/random-finetune-GGUF', category: 'full_gpu', sizeBytes: 5.5 * GIB },
  ];
  assert.equal(pickChat(found).repoId, 'unsloth/Qwen3-8B-GGUF');
});

test('chat on a small card takes the best fitting known model, smallest file when nothing is comfortable', () => {
  const tight = [
    { repoId: 'unsloth/Qwen3-8B-GGUF', category: 'hybrid', sizeBytes: 5 * GIB },
    { repoId: 'unsloth/Qwen3-4B-Instruct-2507-GGUF', category: 'hybrid', sizeBytes: 2.5 * GIB },
  ];
  assert.equal(pickChat(tight).repoId, 'unsloth/Qwen3-4B-Instruct-2507-GGUF');
  assert.equal(pickChat([{ repoId: 'a/Qwen3-Coder-GGUF', category: 'full_gpu', sizeBytes: GIB }]), null);
});
