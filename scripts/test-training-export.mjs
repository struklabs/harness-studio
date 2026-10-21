import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = readFileSync(new URL('../src/lib/components/training/trainingFormat.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } });
const { estimateGgufBytes, canExport } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

test('GGUF size choices are ordered and distinguish quantization from full precision', () => {
  const params = 135_000_000;
  const sizes = ['Q4_K_M', 'Q5_K_M', 'Q8_0', 'F16'].map(q => estimateGgufBytes(params, q));
  for (let i = 1; i < sizes.length; i++) assert.ok(sizes[i] > sizes[i - 1]);
  assert.ok(sizes[0] > 80_000_000 && sizes[0] < 100_000_000);
  assert.ok(sizes[3] > 270_000_000);
  const selected = ['Q4_K_M', 'Q8_0', 'F16'];
  const batchBytes = selected.reduce((sum, q) => sum + estimateGgufBytes(params, q), 0);
  assert.ok(batchBytes > sizes[3] + sizes[2]);
});
test('failed GGUF export can be retried, but running export and training cannot overlap', () => {
  const job = { status: 'done', checkpoint_step: 5, export: { status: 'running' } };
  assert.equal(canExport(job), false);
  job.export.status = 'failed';
  assert.equal(canExport(job), true);
  job.status = 'running';
  assert.equal(canExport(job), false);
  job.status = 'cancelled';
  assert.equal(canExport(job), true);
  job.checkpoint_step = null;
  assert.equal(canExport(job), false);
});
