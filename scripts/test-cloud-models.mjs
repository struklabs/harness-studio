import assert from 'node:assert/strict';
import test from 'node:test';
import { cloudLabel, filterCloudModels, formatPrice, groupCloudModels, isCloudModel, isExternalModel, priceValue } from '../src/lib/components/playground/cloudModels.ts';

const row = (provider, model, label = 'OpenAI') => ({
  id: `cloud:${provider}/${model}`, model, provider, kind: 'openai', label, state: 'online',
  display_name: null, context_length: null, input_price: null, output_price: null, tools: null, vision: null,
});

test('cloud ids are recognized next to remote ids', () => {
  assert.equal(isCloudModel('cloud:openai/gpt-4o'), true);
  assert.equal(isCloudModel('cloud:router/meta/llama'), true);
  assert.equal(isCloudModel('cloud:Bad/x'), false);
  assert.equal(isCloudModel('remote:office/x'), false);
  assert.equal(isExternalModel('remote:office/x'), true);
  assert.equal(isExternalModel('cloud:openai/gpt-4o'), true);
  assert.equal(isExternalModel('qwen3-8b'), false);
});

test('models are grouped per provider and filtered by any field', () => {
  const rows = [row('openai', 'gpt-4o'), row('router', 'meta/llama', 'OpenRouter'), row('openai', 'o3')];
  const groups = groupCloudModels(rows);
  assert.deepEqual(groups.map((g) => [g.provider, g.label, g.models.length]), [['openai', 'OpenAI', 2], ['router', 'OpenRouter', 1]]);
  assert.deepEqual(filterCloudModels(rows, 'openrouter').map((m) => m.model), ['meta/llama']);
  assert.deepEqual(filterCloudModels(rows, 'openai o3').map((m) => m.model), ['o3']);
});

test('custom endpoints are labeled by their name', () => {
  assert.equal(cloudLabel({ kind: 'openai', label: 'OpenAI', provider: 'work' }), 'OpenAI');
  assert.equal(cloudLabel({ kind: 'custom', label: 'Custom', provider: 'local-vllm' }), 'local-vllm');
});

test('prices stay short', () => {
  assert.equal(formatPrice(0), '0');
  assert.equal(formatPrice(0.15), '0.15');
  assert.equal(formatPrice(2.5), '2.5');
  assert.equal(formatPrice(0.0012), '0.0012');
});

test('price fields read typed numbers, untouched strings and cleared inputs', () => {
  assert.equal(priceValue(1000), 1000);
  assert.equal(priceValue('2.5'), 2.5);
  assert.equal(priceValue(''), null);
  assert.equal(priceValue('  '), null);
  assert.equal(priceValue(null), null);
  assert.equal(priceValue(undefined), null);
  assert.equal(priceValue(0), 0);
});
