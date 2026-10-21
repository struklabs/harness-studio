// Model scores: hardware fit, task fit, quality and speed, each with its reasons.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { scoreModel, perfFor, modelKeyOfFile, interp, paramsFromName } = await import('../src/lib/modelScores.ts');

const base = { task: 'chat', fit: 'full_gpu', kind: 'llm', caps: { tools: true, chat: true }, paramsB: 7, quantization: 'Q4_K_M', measured: null };
const record = (extra = {}) => ({
  model_key: 'm', model_name: 'm.gguf', device: 'GPU', updated_at: 1,
  load_ms: null, bench_pp_tps: null, bench_tg_tps: null, image_secs: null, audio_secs: null, tool_probe: null, ...extra,
});

test('the four scores are separate and every one explains itself', () => {
  const s = scoreModel(base);
  for (const k of ['hardware', 'task', 'quality', 'speed']) {
    assert.equal(typeof s[k].value, 'number', k);
    assert.ok(s[k].reasons.length > 0, k);
    assert.ok(s[k].reasons.every((r) => r.key.startsWith('ms.r_')), k);
  }
  assert.equal(s.hardware.value, 100);
  assert.equal(s.speed.measured, false, 'no measurement: an estimate');
});

test('hardware fit follows the category and a slow load costs points', () => {
  assert.ok(scoreModel({ ...base, fit: 'hybrid' }).hardware.value < scoreModel(base).hardware.value);
  assert.equal(scoreModel({ ...base, fit: null }).hardware.value, null);
  const slow = scoreModel({ ...base, measured: record({ load_ms: 90_000 }) }).hardware;
  assert.equal(slow.value, 90);
  assert.equal(slow.measured, true);
  assert.deepEqual(slow.reasons.at(-1), { key: 'ms.r_load_slow', params: { seconds: 90 } });
});

test('task fit: kind, capabilities and the tool probe', () => {
  assert.equal(scoreModel({ ...base, task: 'image' }).task.value, 0);
  assert.equal(scoreModel({ ...base, task: 'image', kind: 'image', caps: null }).task.value, 100);
  assert.equal(scoreModel({ ...base, task: 'vision' }).task.value, 0);
  assert.equal(scoreModel({ ...base, task: 'tools' }).task.value, 80);
  assert.equal(scoreModel({ ...base, task: 'tools', caps: { tools: false } }).task.value, 20);
  const probed = scoreModel({ ...base, task: 'tools', measured: record({ tool_probe: { passed: 7, total: 7, failed: [], at: 1 } }) }).task;
  assert.equal(probed.value, 100);
  assert.equal(probed.measured, true);
  const none = scoreModel({ ...base, task: 'tools', measured: record({ tool_probe: { passed: 0, total: 7, failed: [], at: 1 } }) }).task;
  assert.equal(none.value, 20);
});

test('quality grows with parameters and precision', () => {
  const q = (paramsB, quantization) => scoreModel({ ...base, paramsB, quantization }).quality.value;
  assert.ok(q(30, 'Q4_K_M') > q(7, 'Q4_K_M'));
  assert.ok(q(7, 'Q8_0') > q(7, 'Q4_K_M'));
  assert.ok(q(7, 'Q4_K_M') > q(7, 'Q2_K'));
  assert.equal(q(7, 'BF16'), q(7, 'Q8_0'), 'above 8 bits adds nothing');
  assert.equal(scoreModel({ ...base, paramsB: null }).quality.value, null);
});

test('a tool probe moves quality by at most a fifth', () => {
  const without = scoreModel(base).quality.value;
  const probe = (passed) => scoreModel({ ...base, measured: record({ tool_probe: { passed, total: 7, failed: [], at: 1 } }) }).quality;
  assert.ok(probe(7).value > without);
  assert.ok(probe(0).value < without);
  assert.ok(Math.abs(probe(7).value - without) <= 20);
  assert.equal(probe(7).measured, true);
});

test('speed prefers measurements over the fit estimate', () => {
  const fast = scoreModel({ ...base, measured: record({ bench_tg_tps: 80, bench_pp_tps: 1500 }) }).speed;
  const slow = scoreModel({ ...base, measured: record({ bench_tg_tps: 2 }) }).speed;
  assert.equal(fast.value, 100);
  assert.ok(slow.value < 20);
  assert.equal(fast.measured, true);
  assert.deepEqual(fast.reasons.map((r) => r.key), ['ms.r_s_tg', 'ms.r_s_pp']);
  assert.ok(scoreModel({ ...base, fit: 'cpu_heavy' }).speed.value < scoreModel(base).speed.value);
  const img = scoreModel({ ...base, task: 'image', kind: 'image', measured: record({ image_secs: 12 }) }).speed;
  assert.ok(img.value > 75 && img.value < 100);
  assert.equal(img.reasons[0].key, 'ms.r_s_image');
});

test('records match a file by the name the gateway serves it under', () => {
  assert.equal(modelKeyOfFile('Qwen3-4B-Q4_K_M.gguf'), 'qwen3-4b-q4_k_m');
  assert.equal(modelKeyOfFile('C:\\models\\Big-00001-of-00003.gguf'), 'big');
  const a = record({ model_key: 'big', device: 'A' });
  const b = record({ model_key: 'big', device: 'B' });
  assert.equal(perfFor([a, b], 'Big-00002-of-00003.gguf'), a);
  assert.equal(perfFor([a, b], 'big.gguf', 'B'), b);
  assert.equal(perfFor([a], 'other.gguf'), null);
});

test('interp is flat outside its range', () => {
  assert.equal(interp(0, [[1, 5], [2, 10]]), 5);
  assert.equal(interp(9, [[1, 5], [2, 10]]), 10);
  assert.equal(interp(1.5, [[1, 5], [2, 10]]), 7.5);
});

test('parameters are read from a repo or file name', () => {
  assert.equal(paramsFromName('unsloth/Qwen3-4B-GGUF'), 4);
  assert.equal(paramsFromName('meta/Llama-3.1-8B-Instruct'), 8);
  assert.equal(paramsFromName('google/gemma-3-270m-it'), 0.27);
  assert.equal(paramsFromName('Qwen3.5-0.8B-Q4_K_M.gguf'), 0.8);
  assert.equal(paramsFromName('whisper-large-v3'), null);
});
