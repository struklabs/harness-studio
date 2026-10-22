import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/lib/components/training/wizardConfig.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } });
const w = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

const preset = (over = {}) => ({
  tier: 'balanced', method: 'lora', device: 'cuda', batch_size: 2, grad_accum: 8, sequence_length: 1024, rank: 32,
  lora_alpha: 64, learning_rate: 0.0002, steps: 120, warmup_ratio: 0.05, lr_scheduler: 'cosine', lora_dropout: 0.05,
  weight_decay: 0.01, fit: 'fits', recommended: true, notes: [], estimate: {}, ...over,
});
const report = (histogram) => ({ histogram, rows: histogram.reduce((a, b) => a + b, 0) });

test('a preset fills the tuned fields and keeps name, model, dataset and split', () => {
  const s = { ...w.defaultState(), name: 'Mine', model: '/m', dataset: '/d.csv', validationFraction: 0.2 };
  const next = w.applyPreset(s, preset());
  assert.equal(next.name, 'Mine');
  assert.equal(next.dataset, '/d.csv');
  assert.equal(next.validationFraction, 0.2);
  assert.equal(next.rank, 32);
  assert.equal(next.loraAlpha, 64);
  assert.equal(next.sequenceLength, 1024);
  assert.equal(next.device, 'cuda');
  assert.equal(w.effectiveBatch(next), 16);
  assert.deepEqual(w.changedFields(next, preset()), []);
  assert.deepEqual(w.changedFields({ ...next, rank: 8, learningRate: 0.0001 }, preset()), ['learningRate', 'rank']);
});

test('the request carries every setting, trims text and drops blank optionals', () => {
  const s = w.applyPreset({ ...w.defaultState(), name: '  Support Bot ', model: ' /m ', dataset: ' /d ' }, preset());
  const req = w.buildRequest(s);
  assert.equal(req.name, 'Support Bot');
  assert.equal(req.model, '/m');
  assert.equal(req.dataset, '/d');
  assert.equal(req.batch_size, 2);
  assert.equal(req.grad_accum, 8);
  assert.equal(req.lora_alpha, 64);
  assert.equal(req.lr_scheduler, 'cosine');
  assert.equal(req.checkpoint_steps, undefined);
  assert.equal(w.buildRequest({ ...s, name: '   ' }).name, undefined);
  assert.equal(w.buildRequest({ ...s, evalSteps: 5 }).eval_steps, 5);
});

test('settings are checked against the backend limits', () => {
  const ok = w.defaultState();
  assert.equal(w.settingsError(ok), null);
  assert.equal(w.settingsError({ ...ok, steps: 0 }), 'steps');
  assert.equal(w.settingsError({ ...ok, learningRate: 1 }), 'learning_rate');
  assert.equal(w.settingsError({ ...ok, batchSize: 65 }), 'batch_size');
  assert.equal(w.settingsError({ ...ok, loraDropout: 0.6 }), 'lora_dropout');
  assert.equal(w.settingsError({ ...ok, validationFraction: 0 }), 'validation_fraction');
  assert.equal(w.settingsError({ ...ok, sequenceLength: Number.NaN }), 'sequence_length');
});

test('the length histogram marks buckets that a sequence length would cut', () => {
  const r = report([10, 20, 30, 20, 10, 5, 3, 2, 0, 0]);
  const bars = w.histogramBars(r, 256);
  assert.equal(bars.length, 10);
  assert.equal(bars[0].label, '≤32');
  assert.equal(bars[9].label, '>8192');
  assert.deepEqual(bars.map((b) => b.truncated), [false, false, false, false, true, true, true, true, true, true]);
  assert.equal(bars[2].height, 1);
  assert.equal(w.truncatedShare(r, 256), 20 / 100);
  assert.equal(w.truncatedShare(r, 8192), 0);
  assert.equal(w.truncatedShare({ rows: 0 }, 256), 0);
});

test('a recipe round-trips and rejects broken input by naming the field', () => {
  const config = {
    name: 'Run', model: '/m', dataset: '/d', method: 'qlora', device: 'cuda', steps: 50, learning_rate: 0.0001, rank: 8,
    lora_alpha: 0, lora_dropout: 0.1, batch_size: 4, grad_accum: 4, sequence_length: 256, warmup_ratio: 0.1,
    lr_scheduler: 'linear', weight_decay: 0.02, validation_fraction: 0.15, checkpoint_steps: 10, eval_steps: 5,
  };
  const parsed = w.parseRecipe(w.recipeFromConfig(config));
  assert.equal(parsed.ok, true);
  assert.equal(parsed.state.method, 'qlora');
  assert.equal(parsed.state.loraAlpha, 16);
  assert.equal(parsed.state.batchSize, 4);
  assert.equal(parsed.state.checkpointSteps, 10);
  const withPrompts = { ...config, sample_prompts: ['First question', 'Second question'] };
  const restored = w.parseRecipe(w.recipeFromConfig(withPrompts));
  assert.equal(restored.ok, true);
  assert.deepEqual(w.buildRequest(restored.state).sample_prompts, withPrompts.sample_prompts);
  assert.equal(w.parseRecipe('{"recipe":1,"sample_prompts":[4]}').error, 'field');
  assert.equal(w.settingsError({ ...w.defaultState(), samplePrompts: 'p\n'.repeat(9) }), 'sample_prompts');
  assert.equal(w.parseRecipe('nope').error, 'json');
  assert.equal(w.parseRecipe('{"recipe": 2}').error, 'version');
  assert.deepEqual(w.parseRecipe('{"recipe":1,"rank":"x"}'), { ok: false, error: 'field', field: 'rank' });
  assert.deepEqual(w.parseRecipe('{"recipe":1,"method":"full"}'), { ok: false, error: 'field', field: 'method' });
  assert.deepEqual(w.parseRecipe('{"recipe":1,"steps":0}'), { ok: false, error: 'field', field: 'steps' });
  assert.equal(w.parseRecipe('{"recipe":1,"unknown":true}').ok, true);
  // MLX on Apple Silicon is a device of its own; an unknown one is still refused.
  assert.equal(w.parseRecipe(w.recipeFromConfig({ ...config, device: 'mlx' })).state.device, 'mlx');
  assert.deepEqual(w.parseRecipe('{"recipe":1,"device":"tpu"}'), { ok: false, error: 'field', field: 'device' });
});

const fmtSource = readFileSync(new URL('../src/lib/components/training/trainingFormat.ts', import.meta.url), 'utf8');
const fmtOut = ts.transpileModule(fmtSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
const { liveStats } = await import(`data:text/javascript;base64,${Buffer.from(fmtOut).toString('base64')}`);

test('live stats appear after a few steps and give speed and time left', () => {
  const job = { status: 'running', training: true, step: 20, total_steps: 100, started_at: 1000, finished_at: null, estimate: { tokens_per_step: 2048 } };
  const s = liveStats(job, 1040);
  assert.equal(s.stepsPerSecond, 0.5);
  assert.equal(s.tokensPerSecond, 1024);
  assert.equal(s.etaSeconds, 160);
  assert.equal(liveStats({ ...job, step: 2 }, 1040), null);
  assert.equal(liveStats({ ...job, training: false }, 1040), null);
  assert.equal(liveStats({ ...job, status: 'done' }, 1040), null);
});

test('the PyTorch environment card stays away where MLX trains by default', () => {
  const mac = ['cpu', 'mps', 'mlx'];
  assert.equal(w.showsTorchEnv(mac, { installed: false }), false);
  assert.equal(w.showsTorchEnv(mac, { installed: true }), true);
  assert.equal(w.showsTorchEnv(mac, { installed: false }, true), true);
  assert.equal(w.showsTorchEnv(['cpu', 'cuda'], { installed: false }), true);
  assert.equal(w.showsTorchEnv(['cpu'], null), true);
  assert.equal(w.showsTorchEnv(undefined, null), true);
});

test('live stats leave out model loading and steps of an earlier run', () => {
  // Loaded at 1030 after a resume from step 8; 12 steps in 20 s of training.
  const job = { status: 'running', training: true, step: 20, total_steps: 32, started_at: 1000, finished_at: null, training_since: 1030, training_from_step: 8, estimate: { tokens_per_step: 100 } };
  const s = liveStats(job, 1050);
  assert.equal(s.stepsPerSecond, 0.6);
  assert.equal(s.etaSeconds, 20);
  assert.equal(liveStats({ ...job, step: 8 }, 1050), null);
});

test('hardware defaults prefer CUDA, then MLX over MPS and CPU', () => {
  assert.equal(w.preferredDevice(['cpu', 'mps', 'mlx']), 'mlx');
  assert.equal(w.preferredDevice(['cpu', 'cuda', 'mlx']), 'cuda');
  assert.equal(w.preferredDevice(['cpu', 'mps']), 'mps');
  assert.equal(w.preferredDevice(['cpu']), 'cpu');
  const wizard = readFileSync(new URL('../src/lib/components/training/Wizard.svelte', import.meta.url), 'utf8');
  assert.match(wizard, /if \(compat && !deviceInitialized\)/);
  assert.match(wizard, /s.device = preferredDevice\(devices\)/);
});
