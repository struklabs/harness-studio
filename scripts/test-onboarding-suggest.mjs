import assert from 'node:assert/strict';
import { test } from 'node:test';
import { needsSmallModelWave, pickBest, smallQueries } from '../src/lib/components/onboarding/suggestPlan.ts';

test('small-model wave runs when nothing fits or only weak tiers do', () => {
  assert.equal(needsSmallModelWave([]), true);
  assert.equal(needsSmallModelWave(['hybrid', 'cpu_heavy']), true);
  assert.equal(needsSmallModelWave(['hybrid', 'tight']), false);
  assert.equal(needsSmallModelWave(['full_gpu']), false);
});

test('small queries are role specific', () => {
  assert.ok(smallQueries('chat').includes('3B'));
  assert.ok(smallQueries('code').every((q) => q.startsWith('coder-')));
});

test('pickBest prefers the better fit tier, then the bigger file', () => {
  const a = { id: 'a', category: 'hybrid', sizeBytes: 9 };
  const b = { id: 'b', category: 'full_gpu', sizeBytes: 2 };
  const c = { id: 'c', category: 'full_gpu', sizeBytes: 3 };
  assert.equal(pickBest([a, b, c]).id, 'c');
  assert.equal(pickBest([a]).id, 'a');
  assert.equal(pickBest([]), null);
});

test('pickBest ranks tool evidence first for code', () => {
  const a = { id: 'a', category: 'full_gpu', sizeBytes: 5, rank: 2 };
  const b = { id: 'b', category: 'hybrid', sizeBytes: 1, rank: 0 };
  assert.equal(pickBest([a, b], (x) => x.rank).id, 'b');
});
