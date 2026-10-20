import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resourceOverrides, executionLocation } from '../src/lib/modelSetup.ts';

test('saved resources preserve every definition override', () => {
  assert.deepEqual(resourceOverrides('saved', [], []), {});
});

test('CPU disables GPU offload; automatic selection resets saved device restrictions', () => {
  assert.deepEqual(resourceOverrides('cpu', [], []), { gpu_layers: 0 });
  assert.deepEqual(resourceOverrides('auto', [], []), {
    gpu_layers: '', main_gpu: '', split_mode: '', tensor_split: '',
  });
});

const gpu = (index, name = `GPU ${index}`) => ({ index, name });

test('a selected device uses its OS adapter index and disables multi-device splitting', () => {
  assert.deepEqual(resourceOverrides('3', [gpu(1), gpu(3)], [gpu(1), gpu(3)]), {
    gpu_layers: '', main_gpu: 3, split_mode: 'none', tensor_split: '',
  });
  assert.deepEqual(resourceOverrides('0', [gpu(0)], [gpu(0)]), {
    gpu_layers: '', main_gpu: 0, split_mode: 'none', tensor_split: '',
  });
});

test('removed devices and malformed selections cannot produce load overrides', () => {
  for (const choice of ['2', '-1', '1.5', 'NaN', '', 'gpu', '1e0', 'Infinity']) {
    assert.equal(resourceOverrides(choice, [gpu(0), gpu(1), gpu(3)], [gpu(0), gpu(1), gpu(3)]), null, choice);
  }
});

test('execution location survives an offline or unloaded remote model cache', () => {
  assert.deepEqual(executionLocation('remote:home-gpu/Qwen/Model'), { key: 'setup.remote', device: 'home-gpu' });
  assert.deepEqual(executionLocation('local-model'), { key: 'setup.this_pc' });
  assert.deepEqual(executionLocation('remote:not-a-link'), { key: 'setup.this_pc' });
});

test('hybrid Intel adapter 0 and NVIDIA adapter 1 maps CUDA0 to adapter 1', () => {
  const adapters = [gpu(0, 'Intel(R) Iris Xe Graphics'), gpu(1, 'NVIDIA GeForce RTX 3050 Ti')];
  const devices = [gpu(0, 'NVIDIA GeForce RTX 3050 Ti')];
  assert.deepEqual(resourceOverrides('0', devices, adapters), {
    gpu_layers: '', main_gpu: 1, split_mode: 'none', tensor_split: '',
  });
});

test('adapter matching normalizes names and pairs identical GPUs in adapter order', () => {
  const adapters = [gpu(4, 'AMD Radeon (TM) RX 7900'), gpu(2, 'AMD Radeon (TM) RX 7900')];
  const devices = [gpu(0, 'AMD Radeon RX 7900'), gpu(1, 'AMD Radeon RX 7900')];
  assert.equal(resourceOverrides('0', devices, adapters).main_gpu, 2);
  assert.equal(resourceOverrides('1', devices, adapters).main_gpu, 4);
});

test('unmatched or unavailable adapters cannot leak runtime indices into launch settings', () => {
  assert.equal(resourceOverrides('0', [gpu(0, 'NVIDIA RTX')], [gpu(0, 'Intel Iris')]), null);
  assert.equal(resourceOverrides('0', [gpu(0)], []), null);
});
