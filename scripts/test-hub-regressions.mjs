import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseSearchQuery } from '../src/lib/components/hub/searchParser.ts';
import { recommendVariant } from '../src/lib/components/hub/recommend.ts';

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
