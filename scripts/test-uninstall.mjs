import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  CATEGORY_ORDER,
  applyStepEvent,
  buildSelection,
  initialSelection,
  initialSteps,
  keptCategories,
  progressOf,
  removedCategories,
  stepIds,
  totalBytes,
} from '../src/lib/uninstall.ts';

const info = (category, bytes, present = true, default_selected = false) => ({ category, bytes, present, default_selected });
const infos = CATEGORY_ORDER.map((id) =>
  info(id, 100, id !== 'agents', ['engines', 'python_envs', 'caches', 'agents'].includes(id)),
);

test('safe defaults keep user content and only tick categories that exist', () => {
  const selected = initialSelection(infos);
  assert.deepEqual([...selected].sort(), ['caches', 'engines', 'python_envs']);
  assert.ok(!selected.has('models') && !selected.has('conversations') && !selected.has('agents'));
});

test('remove and keep lists follow the mode', () => {
  const selected = new Set(['caches', 'models']);
  assert.deepEqual(removedCategories('choose', infos, selected), ['models', 'caches']);
  assert.ok(keptCategories('choose', infos, selected).includes('conversations'));
  assert.ok(!removedCategories('everything', infos, selected).includes('agents'));
  assert.deepEqual(keptCategories('everything', infos, selected), []);
});

test('totals report pending sizes instead of guessing', () => {
  const mixed = [info('models', 50), info('caches', null)];
  assert.deepEqual(totalBytes(mixed, ['models']), { bytes: 50, pending: false });
  assert.deepEqual(totalBytes(mixed, ['models', 'caches']), { bytes: 50, pending: true });
});

test('external model folders only travel with the models category', () => {
  const folders = ['D:/models'];
  assert.deepEqual(buildSelection('choose', new Set(['caches']), folders).external_models, []);
  assert.deepEqual(buildSelection('choose', new Set(['models']), folders).external_models, folders);
  const all = buildSelection('everything', new Set(), folders);
  assert.equal(all.everything, true);
  assert.deepEqual(all.external_models, folders);
});

test('steps list credentials and leftovers only when they apply', () => {
  assert.deepEqual(stepIds('choose', ['caches']), ['stop', 'caches']);
  assert.deepEqual(stepIds('choose', ['settings']), ['stop', 'settings', 'credentials']);
  assert.deepEqual(stepIds('everything', ['models']), ['stop', 'models', 'credentials', 'leftovers']);
});

test('step events update state and progress', () => {
  let steps = initialSteps(['stop', 'caches']);
  assert.equal(progressOf(steps), 0);
  steps = applyStepEvent(steps, { state: 'started', step: 'stop' });
  assert.equal(progressOf(steps), 0.25);
  steps = applyStepEvent(steps, { state: 'done', step: 'stop' });
  steps = applyStepEvent(steps, { state: 'failed', step: 'caches', error: 'locked' });
  assert.equal(progressOf(steps), 1);
  assert.equal(steps[1].error, 'locked');
  assert.equal(applyStepEvent(steps, { state: 'done', step: 'extra' }).length, 3);
});
