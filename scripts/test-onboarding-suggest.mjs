import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fitReasonKey, needsSmallModelWave, pickBest, smallQueries } from '../src/lib/components/onboarding/suggestPlan.ts';
import { purposeFooterAction } from '../src/lib/components/onboarding/purposeFooter.ts';

test('purpose footer promotes selected installs and restores navigation during and after installation', () => {
  assert.equal(purposeFooterAction(0, false, false, 0), 'skip');
  assert.equal(purposeFooterAction(1, false, false, 0), 'install');
  assert.equal(purposeFooterAction(3, false, false, 0), 'install');
  assert.equal(purposeFooterAction(3, true, false, 0), 'continue');
  assert.equal(purposeFooterAction(0, false, true, 2), 'continue');
  assert.equal(purposeFooterAction(0, false, false, 2), 'continue');
  assert.equal(purposeFooterAction(0, false, true, 0), 'continue');
  assert.equal(purposeFooterAction(1, false, true, 0), 'continue');
  // A failed attempt with no successful installs can be retried.
  assert.equal(purposeFooterAction(1, false, false, 0), 'install');
});

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
  assert.equal(pickChat(found).repoId, 'unsloth/Qwen3-4B-Instruct-2507-GGUF');
});

test('chat on a small card takes the best fitting known model, smallest file when nothing is comfortable', () => {
  const tight = [
    { repoId: 'unsloth/Qwen3-8B-GGUF', category: 'hybrid', sizeBytes: 5 * GIB },
    { repoId: 'unsloth/Qwen3-4B-Instruct-2507-GGUF', category: 'hybrid', sizeBytes: 2.5 * GIB },
  ];
  assert.equal(pickChat(tight).repoId, 'unsloth/Qwen3-4B-Instruct-2507-GGUF');
  assert.equal(pickChat([{ repoId: 'a/Qwen3-Coder-GGUF', category: 'full_gpu', sizeBytes: GIB }]), null);
});

test('chat on a 4 GB card takes the tight 4B instruct over a roomy 1B', () => {
  const found = [
    { repoId: 'lmstudio-community/gemma-3-1b-it-GGUF', category: 'full_gpu', sizeBytes: 0.75 * GIB },
    { repoId: 'unsloth/Qwen3-4B-Instruct-2507-GGUF', category: 'tight', sizeBytes: 2.3 * GIB },
    { repoId: 'unsloth/gemma-3-4b-it-GGUF', category: 'tight', sizeBytes: 2.3 * GIB },
    { repoId: 'unsloth/Qwen3-8B-GGUF', category: 'hybrid', sizeBytes: 5 * GIB },
  ];
  assert.equal(pickChat(found).repoId, 'unsloth/Qwen3-4B-Instruct-2507-GGUF');
  // In one family a full fit still beats a tight one.
  const same = [
    { repoId: 'unsloth/gemma-3-4b-it-GGUF', category: 'tight', sizeBytes: 2.3 * GIB },
    { repoId: 'lmstudio-community/gemma-3-1b-it-GGUF', category: 'full_gpu', sizeBytes: 0.75 * GIB },
  ];
  assert.equal(pickChat(same).repoId, 'lmstudio-community/gemma-3-1b-it-GGUF');
});

import { familyRank } from '../src/lib/components/onboarding/suggestPlan.ts';

test('chat skips reasoning distills, fine-tunes and merges', () => {
  for (const id of ['unsloth/DeepSeek-R1-0528-Qwen3-8B-GGUF', 'bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF', 'unsloth/Qwen3-4B-Thinking-2507-GGUF', 'x/Qwen3-8B-abliterated-GGUF', 'x/Qwen3-8B-uncensored-GGUF', 'x/Llama-3.1-8B-roleplay-GGUF', 'x/Llama-3-8B-RP-GGUF', 'x/Qwen3-merge-GGUF', 'x/QwQ-32B-reasoning-GGUF']) {
    assert.equal(isGeneralChatRepo(id), false, id);
  }
  // A foreign name in front of the family is no official release.
  assert.equal(isKnownChatFamily('someone/Hermes-Qwen3-8B-GGUF'), false);
});

test('official instruct releases rank Qwen3 Instruct-2507, Gemma, hybrid Qwen3, Llama, Mistral, Phi', () => {
  const ids = ['unsloth/Qwen3-4B-Instruct-2507-GGUF', 'bartowski/google_gemma-3-4b-it-GGUF', 'unsloth/Qwen3-8B-GGUF', 'bartowski/Llama-3.2-3B-Instruct-GGUF', 'bartowski/Ministral-8B-Instruct-2410-GGUF', 'unsloth/Phi-4-mini-instruct-GGUF'];
  assert.deepEqual(ids.map(familyRank), [0, 1, 2, 3, 4, 5]);
  assert.equal(familyRank('unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF'), 0);
});

test('chat picks the non-thinking Qwen3 Instruct-2507, then Gemma, before hybrid Qwen3', () => {
  const found = [
    { repoId: 'unsloth/DeepSeek-R1-0528-Qwen3-8B-GGUF', category: 'full_gpu', sizeBytes: 4.7 * GIB },
    { repoId: 'unsloth/gemma-3-12b-it-GGUF', category: 'full_gpu', sizeBytes: 5.9 * GIB },
    { repoId: 'unsloth/Qwen3-4B-Instruct-2507-GGUF', category: 'full_gpu', sizeBytes: 2.5 * GIB },
    { repoId: 'unsloth/Qwen3-8B-GGUF', category: 'full_gpu', sizeBytes: 5 * GIB },
  ];
  assert.equal(pickChat(found).repoId, 'unsloth/Qwen3-4B-Instruct-2507-GGUF');
  // Without the 2507 release Gemma beats the plain hybrid Qwen3.
  assert.equal(pickChat(found.filter((c) => !c.repoId.includes('2507'))).repoId, 'unsloth/gemma-3-12b-it-GGUF');
  // The 30B-A3B is far above the comfort size: the 4B stays the pick.
  const big = [...found, { repoId: 'unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF', category: 'full_gpu', sizeBytes: 18 * GIB }];
  assert.equal(pickChat(big).repoId, 'unsloth/Qwen3-4B-Instruct-2507-GGUF');
  // 4 GB card: only the 4B instruct fits whole.
  const small = [
    { repoId: 'unsloth/Qwen3-8B-GGUF', category: 'hybrid', sizeBytes: 5 * GIB },
    { repoId: 'unsloth/Qwen3-4B-Instruct-2507-GGUF', category: 'full_gpu', sizeBytes: 2.5 * GIB },
    { repoId: 'unsloth/gemma-3-1b-it-GGUF', category: 'full_gpu', sizeBytes: 0.8 * GIB },
  ];
  assert.equal(pickChat(small).repoId, 'unsloth/Qwen3-4B-Instruct-2507-GGUF');
  // CPU only: nothing fits whole, the smallest of the best family.
  const cpu = [
    { repoId: 'unsloth/Qwen3-8B-GGUF', category: 'cpu_heavy', sizeBytes: 5 * GIB },
    { repoId: 'unsloth/Qwen3-4B-Instruct-2507-GGUF', category: 'cpu_heavy', sizeBytes: 2.5 * GIB },
  ];
  assert.equal(pickChat(cpu).repoId, 'unsloth/Qwen3-4B-Instruct-2507-GGUF');
});

test('speech to text runs on MLX on Apple Silicon when its engine can be installed', async () => {
  const { asrEngine, mlxWhisperRepo, MLX_WHISPER_REPOS } = await import('../src/lib/components/onboarding/suggestPlan.ts');
  const mac = { os: 'macos', arch: 'aarch64' };
  assert.equal(asrEngine(mac, true), 'mlx');
  // macOS too old for the MLX wheels, an Intel Mac, Windows: whisper.cpp.
  assert.equal(asrEngine(mac, false), 'whisper.cpp');
  assert.equal(asrEngine({ os: 'macos', arch: 'x86_64' }, true), 'whisper.cpp');
  assert.equal(asrEngine({ os: 'windows', arch: 'x86_64' }, true), 'whisper.cpp');
  assert.equal(asrEngine(null, true), 'whisper.cpp');
  assert.equal(mlxWhisperRepo(16 * 1024 ** 3), MLX_WHISPER_REPOS[0]);
  assert.equal(mlxWhisperRepo(null), MLX_WHISPER_REPOS[0]);
  assert.equal(mlxWhisperRepo(2 * 1024 ** 3), MLX_WHISPER_REPOS[1]);
});

test('whisper on the CPU (Linux) starts from small; elsewhere from large-v3-turbo', async () => {
  const { recommendWhisperVariant, whisperOnCpu } = await import('../src/lib/components/hub/recommend.ts');
  const v = (name, mb) => ({ name, quantization: 'Q8_0', total_size_bytes: mb * 1e6, is_split: false, shards: [] });
  const variants = [v('ggml-large-v3-turbo-q8_0', 874), v('ggml-small-q8_0', 264), v('ggml-base-q8_0', 82)];
  const ram = 16e9;
  assert.equal(recommendWhisperVariant(variants, ram).name, 'ggml-large-v3-turbo-q8_0');
  assert.equal(recommendWhisperVariant(variants, ram, true).name, 'ggml-small-q8_0');
  assert.equal(recommendWhisperVariant(variants, 300e6, true).name, 'ggml-base-q8_0');
  assert.ok(whisperOnCpu('linux') && !whisperOnCpu('windows') && !whisperOnCpu('macos') && !whisperOnCpu(undefined));
});

test('a fit reason calls shared memory a budget, not VRAM', () => {
  assert.equal(fitReasonKey('full_gpu', false), 'onboarding.sg_reason_vram');
  assert.equal(fitReasonKey('full_gpu', true), 'onboarding.sg_reason_shared');
  assert.equal(fitReasonKey('tight', false), 'onboarding.sg_reason_tight');
  assert.equal(fitReasonKey('tight', true), 'onboarding.sg_reason_tight_shared');
  assert.equal(fitReasonKey('hybrid', true), 'onboarding.sg_reason_hybrid');
  assert.equal(fitReasonKey('cpu_heavy', true), 'onboarding.sg_reason_cpu');
});
