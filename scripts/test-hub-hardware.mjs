import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bestRepoFit, fitTier, hasCompleteSize, rankHardware, repoUnavailable, unavailable } from '../src/lib/components/hub/hardwareRanking.ts';

const variant = (name, quant = 'Q4_K_M', size = 100) => ({ name, quantization: quant, total_size_bytes: size, shards: [{ filename: `${name}.gguf` }] });
const fit = (category, host = 0, gpu = 100) => ({
  category,
  budget: { ram_bytes: 1000, vram_bytes: 500 },
  estimate: { model_host_bytes: host, model_gpu_bytes: gpu, total_layers: 32, offloaded_layers: host ? 16 : 32 },
});
const repo = (id, downloads = 0, compatibility = { status: 'runnable', runtime: 'llama.cpp' }) => ({ id, downloads, compatibility });

test('full GPU requires all weights on GPU and a non-OOM result, including context headroom', () => {
  assert.equal(fitTier(fit('full_gpu')), 'full');
  assert.equal(fitTier(fit('tight')), 'full');
  assert.equal(fitTier(fit('hybrid', 50)), 'partial');
  assert.equal(fitTier(fit('oom_risk')), 'unavailable');
  const cpu = fit('cpu_heavy', 100, 0);
  cpu.budget.vram_bytes = 0;
  assert.equal(fitTier(cpu), 'partial');
  cpu.budget.ram_bytes = 0;
  assert.equal(fitTier(cpu), 'unknown');
});

test('layerless speech and image estimates use their actual memory split', () => {
  const full = fit('full_gpu');
  full.estimate.total_layers = 0;
  full.estimate.offloaded_layers = 0;
  assert.equal(fitTier(full), 'full');
  assert.equal(fitTier({ ...full, estimate: { ...full.estimate, model_host_bytes: 50 } }), 'partial');
});

test('best quantization in the best tier wins using existing quality preferences', () => {
  const q4 = variant('q4');
  const q8 = variant('q8', 'Q8_0', 200);
  assert.deepEqual(bestRepoFit([q4, q8], new Map([['q4', fit('full_gpu')], ['q8', fit('hybrid', 50)]])), { tier: 'full', quant: 'Q4_K_M' });
  assert.deepEqual(bestRepoFit([q4, q8], new Map([['q4', fit('full_gpu')], ['q8', fit('full_gpu')]])), { tier: 'full', quant: 'Q8_0' });
  const fp16 = variant('half', 'FP16');
  assert.deepEqual(bestRepoFit([fp16], new Map([['half', fit('full_gpu')]])), { tier: 'full', quant: 'FP16' });
});

test('companions and refused variants cannot create a false fit', () => {
  const main = variant('main');
  const proj = variant('mmproj', 'mmproj');
  const refused = { ...variant('refused'), not_runnable: 'foreign_speech_gguf' };
  assert.equal(bestRepoFit([main, proj, refused], new Map([['main', fit('oom_risk')], ['mmproj', fit('full_gpu')], ['refused', fit('full_gpu')]])).tier, 'unavailable');
  assert.equal(bestRepoFit([main, variant('unknown-size', 'Q8_0', 0)], new Map([['main', fit('oom_risk')]])).tier, 'unknown');
  assert.equal(bestRepoFit([main], new Map()).tier, 'unknown');
});

test('rank tiers before download popularity and keep equal counts stable without mutation', () => {
  const repos = [repo('oom', 10000), repo('unknown', 9000), repo('partial', 1000), repo('full-low', 10), repo('full-high', 100), repo('full-tie', 100)];
  const fits = new Map([['oom', { tier: 'unavailable' }], ['partial', { tier: 'partial' }], ...['full-low', 'full-high', 'full-tie'].map(id => [id, { tier: 'full' }])]);
  assert.deepEqual(rankHardware(repos, fits).map(r => r.id), ['full-high', 'full-tie', 'full-low', 'partial', 'unknown', 'oom']);
  assert.equal(repos[0].id, 'oom');
});

test('platform and format refusals override stale fit; installable engines remain eligible', () => {
  const blocked = { status: 'unsupported_platform', runtime: 'mlx', os: 'windows', arch: 'x86_64' };
  const unsupported = { status: 'unsupported', format: 'unknown' };
  assert.equal(unavailable(blocked), true);
  assert.equal(unavailable(unsupported), true);
  assert.equal(unavailable({ status: 'needs_runtime', runtime: 'whisper.cpp' }), false);
  assert.deepEqual(rankHardware([repo('blocked', 100, blocked), repo('unknown')], new Map([['blocked', { tier: 'full' }]])).map(r => r.id), ['unknown', 'blocked']);
  assert.equal(repoUnavailable({ ...repo('bad-architecture'), not_runnable: 'unsupported_architecture' }), true);
});

test('incomplete split files never supply an artificially small weight estimate', () => {
  const split = { ...variant('split'), is_split: true, shards: [{ size_bytes: 50, shard_index: 1, total_shards: 2 }] };
  assert.equal(hasCompleteSize(split), false);
  split.shards.push({ size_bytes: 50, shard_index: 2, total_shards: 2 });
  assert.equal(hasCompleteSize(split), true);
  split.shards[1].size_bytes = 0;
  assert.equal(hasCompleteSize(split), false);
  split.shards[1] = { size_bytes: 50, shard_index: 1, total_shards: 2 };
  assert.equal(hasCompleteSize(split), false);
});
