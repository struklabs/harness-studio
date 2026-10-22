import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = readFileSync(new URL('../src/lib/components/training/runActions.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } });
const { exportPlan, comparePrompts, compareRun, canCompare } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

test('export plan selects several supported quants in a stable order from one merge', () => {
  assert.deepEqual(exportPlan(' Support ', ['F16', 'Q5_K_M', 'Q5_K_M', 'Q4_K_M', 'Q8_0']), {
    name: 'Support', gguf: true, adapter_only: false, quant: 'Q4_K_M', quants: ['Q4_K_M', 'Q5_K_M', 'Q8_0', 'F16'],
  });
  assert.equal(exportPlan('Run', []), null);
  assert.equal(exportPlan(' ', ['F16']).name, null);
});

test('compare validates prompts and preserves duplicates and order for fair pairs', () => {
  assert.deepEqual(comparePrompts(' First \n\nSecond\nFirst '), ['First', 'Second', 'First']);
  for (const text of ['', ' \n ', 'p\n'.repeat(9), 'a'.repeat(2001)]) assert.equal(comparePrompts(text), null);
  assert.ok(comparePrompts('a'.repeat(2000)));
});

test('compare freezes the request while generation is in flight and propagates failures', async () => {
  const prompts = ['One', 'Two'];
  const result = await compareRun(prompts, async request => {
    prompts[0] = 'Edited';
    assert.deepEqual(request.prompts, ['One', 'Two']);
    return [{ prompt: 'One', base: 'Before', trained: 'After' }];
  });
  assert.equal(result[0].trained, 'After');
  await assert.rejects(compareRun(['One'], async () => { throw new Error('failed'); }), /failed/);
});

test('compare is available only for a finished adapter or a complete checkpoint', () => {
  for (const status of ['draft', 'queued', 'running']) assert.equal(canCompare({ status, checkpoint_step: 5 }), false);
  assert.equal(canCompare({ status: 'done', checkpoint_step: null }), true);
  for (const status of ['cancelled', 'failed']) {
    assert.equal(canCompare({ status, checkpoint_step: null }), false);
    assert.equal(canCompare({ status, checkpoint_step: 5 }), true);
  }
});

test('a training card shows when the engine is being installed', () => {
  const card = readFileSync(new URL('../src/lib/components/training/JobList.svelte', import.meta.url), 'utf8');
  assert.match(card, /job.installing_engine \? t\('training.installing_engine'\)/);
});
