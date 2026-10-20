import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  agentsFit,
  cancelPending,
  defaultSelection,
  initialRun,
  isFinished,
  orderedSelection,
  patchRun,
  profileOf,
  recommendedModelTier,
  retryIds,
  runProgress,
  totalDownload,
} from '../src/lib/setup.ts';

const GIB = 1024 ** 3;
const gpu = (vendor, vram, discrete = true) => ({ vendor, vram_bytes: vram, is_discrete: discrete });
const hw = (ram, gpus, backend, extra = {}) => ({ total_ram_bytes: ram * GIB, gpus, recommended_backend: backend, ...extra });
const none = { installed: {}, unavailable: {}, freeDiskBytes: 200 * GIB };

const machines = {
  tinyLaptop: hw(4, [gpu('Intel', null, false)], 'Cpu'),
  cpuOffice: hw(16, [gpu('Intel', null, false)], 'Cpu'),
  cpuBig: hw(32, [], 'Cpu'),
  amd8: hw(32, [gpu('Amd', 8 * GIB)], 'Vulkan', { primary_gpu_index: 0 }),
  nvidia4: hw(16, [gpu('Nvidia', 4 * GIB)], 'Cuda12'),
  nvidia24: hw(64, [gpu('Nvidia', 24 * GIB)], 'Cuda12'),
  m1: hw(16, [gpu('Apple', null)], 'Metal', { unified_memory_budget_bytes: 10 * GIB }),
};

test('unreadable hardware is a small CPU machine', () => {
  const p = profileOf(null);
  assert.equal(p.accelerator, 'cpu');
  assert.equal(p.tier, 'low');
  assert.equal(recommendedModelTier(p), 'tiny');
});

test('the accelerator and the engine build follow the detected hardware', () => {
  assert.deepEqual(
    Object.entries(machines).map(([k, m]) => [k, profileOf(m).accelerator, profileOf(m).backend]),
    [
      ['tinyLaptop', 'cpu', 'cpu'],
      ['cpuOffice', 'cpu', 'cpu'],
      ['cpuBig', 'cpu', 'cpu'],
      ['amd8', 'amd', 'vulkan'],
      ['nvidia4', 'nvidia', 'cuda'],
      ['nvidia24', 'nvidia', 'cuda'],
      ['m1', 'apple', 'metal'],
    ],
  );
});

test('an integrated GPU does not count as its own memory', () => {
  const p = profileOf(hw(16, [gpu('Amd', 2 * GIB, false)], 'Vulkan'));
  assert.equal(p.acceleratorBytes, 0);
  assert.equal(p.accelerator, 'amd');
});

test('the first model grows with what the machine can hold', () => {
  const tier = (k) => recommendedModelTier(profileOf(machines[k]));
  assert.equal(tier('tinyLaptop'), 'tiny');
  assert.equal(tier('cpuOffice'), 'small');
  assert.equal(tier('cpuBig'), 'standard');
  assert.equal(tier('nvidia4'), 'small');
  assert.equal(tier('amd8'), 'standard');
  assert.equal(tier('nvidia24'), 'standard');
  assert.equal(tier('m1'), 'standard');
});

test('defaults tick only the engine and the first model', () => {
  for (const [name, m] of Object.entries(machines)) {
    const selected = defaultSelection(profileOf(m), none);
    assert.ok(selected.has('engine'), name);
    assert.ok(![...selected].some((id) => ['agents', 'cli', 'opencode', 'zed', 'vscode'].includes(id)), name);
  }
});

test('a machine under 4 GB gets no model ticked, the engine still is', () => {
  const selected = defaultSelection(profileOf(hw(2, [], 'Cpu')), none);
  assert.deepEqual([...selected], ['engine']);
});

test('little free disk unticks the model; unknown disk keeps it', () => {
  const p = profileOf(machines.cpuOffice);
  const low = defaultSelection(p, { ...none, freeDiskBytes: 1 * GIB });
  assert.ok(!low.has('model') && low.has('engine'));
  assert.ok(defaultSelection(p, { ...none, freeDiskBytes: null }).has('model'));
});

test('almost no free disk ticks nothing', () => {
  const p = profileOf(machines.cpuOffice);
  assert.equal(defaultSelection(p, { ...none, freeDiskBytes: 0.5 * GIB }).size, 0);
});

test('finished items are not ticked again; unavailable ones are not ticked', () => {
  const p = profileOf(machines.nvidia24);
  const done = defaultSelection(p, { ...none, installed: { engine: true } });
  assert.ok(!done.has('engine') && done.has('model'));
  const blocked = defaultSelection(p, { ...none, unavailable: { model: true } });
  assert.ok(!blocked.has('model'));
});

test('a heavier tier needs more free disk', () => {
  const p = profileOf(machines.nvidia24);
  const facts = { ...none, freeDiskBytes: 3 * GIB };
  assert.ok(!defaultSelection(p, facts, 'standard').has('model'));
  assert.ok(defaultSelection(p, facts, 'tiny').has('model'));
});

test('agents are offered from 8 GB RAM', () => {
  assert.ok(!agentsFit(profileOf(machines.tinyLaptop)));
  assert.ok(agentsFit(profileOf(machines.cpuOffice)));
});

test('items run in a fixed order and sizes add up', () => {
  const selected = new Set(['zed', 'model', 'cli', 'engine']);
  assert.deepEqual(orderedSelection(selected), ['engine', 'model', 'cli', 'zed']);
  assert.equal(totalDownload(selected, { engine: 10, model: 100, cli: 5 }), 115);
});

test('run progress, retry and cancel', () => {
  let items = initialRun(['engine', 'model', 'cli']);
  assert.equal(runProgress(items), 0);
  items = patchRun(items, 'engine', { state: 'done' });
  items = patchRun(items, 'model', { state: 'running', fraction: 0.5 });
  assert.equal(runProgress(items), (1 + 0.5) / 3);
  assert.ok(!isFinished(items));
  items = cancelPending(patchRun(items, 'model', { state: 'failed', detail: 'x' }));
  assert.deepEqual(items.map((i) => i.state), ['done', 'failed', 'cancelled']);
  assert.ok(isFinished(items));
  assert.deepEqual(retryIds(items), ['model', 'cli']);
  assert.equal(runProgress(items), 1);
});
