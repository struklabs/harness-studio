import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseSearchQuery } from '../src/lib/components/hub/searchParser.ts';
import { isCompanionVariant, pickProjectorVariant, recommendVariant } from '../src/lib/components/hub/recommend.ts';

test('exact repository IDs retain GGUF and quantization names', () => {
  for (const repo of ['unsloth/Qwen3-4B-GGUF', 'owner/model-Q4_K_M', 'OWNER/Model.gguf']) {
    assert.deepEqual(parseSearchQuery(` ${repo} `), { cleanedQuery: repo, detectedQuant: null });
  }
  assert.deepEqual(parseSearchQuery('Qwen3 GGUF (Q4_K_M)'), { cleanedQuery: 'Qwen3', detectedQuant: 'Q4_K_M' });
});

const variant = (name, size) => ({ name, total_size_bytes: size, quantization: 'Q8_0', shards: [{ filename: `${name}.gguf` }] });

test('current diffusion weights precede larger archived weights', () => {
  const current = variant('sd_turbo-f16-q8_0', 1_880_000_000);
  for (const folder of ['old', 'Legacy', 'deprecated', 'nested/OLD']) {
    const archived = variant(`${folder}/sd_turbo-q8_0`, 3_140_000_000);
    const fits = new Map([[current.name, 'full_gpu'], [archived.name, 'full_gpu']]);
    assert.equal(recommendVariant([archived, current], fits), current);
    assert.equal(recommendVariant([archived], fits), archived);
    assert.equal(recommendVariant([archived, { ...current, total_size_bytes: 0 }], fits), archived);
    assert.equal(recommendVariant([{ ...archived, name: 'display-name' }, current], new Map([['display-name', 'full_gpu'], [current.name, 'full_gpu']])), current);
  }
});

test('ordinary variant quality and fit ranking is preserved', () => {
  const smaller = variant('current-small', 1_000);
  const larger = variant('current-large', 2_000);
  assert.equal(recommendVariant([smaller, larger], new Map([[smaller.name, 'full_gpu'], [larger.name, 'full_gpu']])), larger);
  assert.equal(recommendVariant([smaller, larger], new Map([[smaller.name, 'hybrid'], [larger.name, 'hybrid']])), smaller);
});

const gguf = (name, quantization, size) => ({ name, total_size_bytes: size, quantization, shards: [{ filename: `${name}.gguf` }] });

test('projectors, drafts, and image components are never the recommended main model', () => {
  const main = gguf('Model-Q8_0', 'Q8_0', 8_000_000_000);
  for (const companion of [
    gguf('mmproj-BF16', 'mmproj', 900_000_000),
    gguf('Model-mmproj-F16', 'F16', 900_000_000),
    gguf('MTP/mtp-Qwen3.8-27B-Q4_0', 'Q4_0', 1_370_000_000),
    gguf('draft-Model-Q4_0', 'Q4_0', 1_000_000_000),
    gguf('Model-assistant-Q4_0', 'Q4_0', 1_000_000_000),
    gguf('eagle3/model', 'Q4_0', 1_000_000_000),
    { name: 'ae', quantization: 'component', total_size_bytes: 300_000_000, shards: [{ filename: 'ae.safetensors' }] },
    { name: 'sd_xl_offset_example-lora_1.0', quantization: 'lora', total_size_bytes: 49_000_000, shards: [{ filename: 'sd_xl_offset_example-lora_1.0.safetensors' }] },
  ]) {
    for (const tier of ['full_gpu', 'hybrid', 'cpu_heavy']) {
      const fits = new Map([[companion.name, tier], [main.name, tier]]);
      assert.equal(recommendVariant([companion, main], fits), main, `${companion.name} ${tier}`);
    }
    // Main files out of memory: the companion is still not recommended.
    assert.equal(recommendVariant([companion, main], new Map([[companion.name, 'full_gpu'], [main.name, 'oom_risk']])), null, companion.name);
  }
  assert.equal(isCompanionVariant(main), false);
  assert.equal(isCompanionVariant(gguf('Model-Q4_K_M', 'Q4_K_M', 1)), false);
});

test('a main GGUF variant brings the best matching projector', () => {
  const v = (file, quantization, size = 1) => ({ name: file.replace('.gguf', ''), quantization, total_size_bytes: size, shards: [{ filename: file }] });
  const main = v('m-Q4_K_M.gguf', 'Q4_K_M', 100);
  const f16 = v('mmproj-m-F16.gguf', 'mmproj', 900);
  const bf16 = v('mmproj-m-BF16.gguf', 'mmproj', 900);
  const f32 = v('mmproj-m-F32.gguf', 'mmproj', 1800);
  assert.equal(pickProjectorVariant([main, f32, bf16, f16], main), f16);
  assert.equal(pickProjectorVariant([main, f32, bf16], main), bf16);
  assert.equal(pickProjectorVariant([main, f32], main), f32);
  const odd = v('mmproj-m-Q8_0.gguf', 'mmproj', 500);
  assert.equal(pickProjectorVariant([main, odd], main), odd);
  assert.equal(pickProjectorVariant([main], main), null);
  assert.equal(pickProjectorVariant([f16, main], f16), null);
  const sub = v('sub/m-Q4_K_M.gguf', 'Q4_K_M');
  const subProj = v('sub/mmproj-F32.gguf', 'mmproj');
  assert.equal(pickProjectorVariant([sub, f16, subProj], sub), subProj);
});
