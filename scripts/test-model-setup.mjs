import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resourceOverrides, executionLocation } from '../src/lib/modelSetup.ts';

test('saved resources preserve every definition override', () => {
  assert.deepEqual(resourceOverrides('saved', []), {});
});

test('CPU disables GPU offload; automatic selection resets saved device restrictions', () => {
  assert.deepEqual(resourceOverrides('cpu', []), { gpu_layers: 0 });
  assert.deepEqual(resourceOverrides('auto', [3]), {
    gpu_layers: '', main_gpu: '', split_mode: '', tensor_split: '',
  });
});

test('a selected runtime device uses its actual index and disables multi-device splitting', () => {
  assert.deepEqual(resourceOverrides('3', [1, 3]), {
    gpu_layers: '', main_gpu: 3, split_mode: 'none', tensor_split: '',
  });
  assert.deepEqual(resourceOverrides('0', [0]), {
    gpu_layers: '', main_gpu: 0, split_mode: 'none', tensor_split: '',
  });
});

test('removed devices and malformed selections cannot produce load overrides', () => {
  for (const choice of ['2', '-1', '1.5', 'NaN', '', 'gpu', '1e0', 'Infinity']) {
    assert.equal(resourceOverrides(choice, [0, 1, 3]), null, choice);
  }
});

test('execution location survives an offline or unloaded remote model cache', () => {
  assert.deepEqual(executionLocation('remote:home-gpu/Qwen/Model'), { key: 'setup.remote', device: 'home-gpu' });
  assert.deepEqual(executionLocation('local-model'), { key: 'setup.this_pc' });
  assert.deepEqual(executionLocation('remote:not-a-link'), { key: 'setup.this_pc' });
});
