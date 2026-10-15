import assert from 'node:assert/strict';
import { test } from 'node:test';
import { anchoredOffset, invalidateHeights, meanHeight, needsAnchorCorrection, columnCount, groupRows, renderedRows, rowAt, totalHeight, visibleRange } from '../src/lib/components/library/windowing.ts';

import { beginDefinitionAction, createCardState, FitCache } from '../src/lib/components/library/cardState.ts';

const ids = Array.from({ length: 604 }, (_, i) => `model-${i}`);

test('columns match auto-fill with 340px minimum and the computed gap', () => {
  for (const [width, columns] of [[0, 1], [200, 1], [340, 1], [687.99, 1], [688, 2], [1035.99, 2], [1036, 3]]) {
    assert.equal(columnCount(width, 'grid'), columns);
    assert.equal(columnCount(width, 'list'), 1);
  }
  assert.equal(columnCount(692, 'grid', 12), 2);
  assert.equal(columnCount(691, 'grid', 12), 1);
});

test('rows use the tallest member, cached by id across reorder and refresh', () => {
  const heights = new Map([['a', 120], ['b', 260], ['c', 80]]);
  const rows = groupRows(['a', 'b', 'c', 'd', 'e'], 2, heights, 200);
  assert.deepEqual(rows, [
    { start: 0, end: 2, height: 260, offset: 0 },
    { start: 2, end: 4, height: 200, offset: 268 },
    { start: 4, end: 5, height: 200, offset: 476 },
  ]);
  assert.equal(totalHeight(rows), 676);
  assert.equal(groupRows(['b', 'c', 'a'], 1, heights)[2].height, 120);
  assert.equal(totalHeight([]), 0);
  assert.deepEqual(visibleRange([], 500, 600), [0, 0]);
});

test('window covers a full screen on either side, including partial and very tall rows', () => {
  const heights = new Map(ids.map((id, i) => [id, i === 40 ? 2400 : 90 + i % 5 * 31]));
  const rows = groupRows(ids, 3, heights);
  for (let top = 0; top < totalHeight(rows); top += 137) {
    const [start, end] = visibleRange(rows, top, 700);
    assert.ok(rows[start].offset <= Math.max(0, top - 700));
    assert.ok(end === rows.length || rows[end].offset >= top + 1400);
    const current = rowAt(rows, top);
    assert.ok(start <= current && current < end);
    if (current > 0) assert.ok(start < current);
    if (current + 1 < rows.length) assert.ok(end > current + 1);
  }
});

test('a distant focus keeps its row and keyboard neighbours without mounting the intervening gap', () => {
  const rows = groupRows(ids, 3, new Map());
  const mounted = renderedRows(rows, 25000, 700, 4);
  assert.ok(mounted.has(0) && mounted.has(1) && mounted.has(2));
  assert.ok(mounted.has(rowAt(rows, 25000)));
  assert.ok(!mounted.has(60));
  assert.ok(mounted.size < 30);
  // Repeated Tab/Shift+Tab can traverse all 604 cards, even with a distant wheel position.
  for (const index of [...ids.keys(), ...ids.keys()].map((i, n) => n < ids.length ? i : ids.length - 1 - i)) {
    const window = renderedRows(rows, 25000, 700, index);
    assert.ok(window.has(Math.floor(index / 3)));
    if (index > 0) assert.ok(window.has(Math.floor((index - 1) / 3)));
    if (index + 1 < ids.length) assert.ok(window.has(Math.floor((index + 1) / 3)));
  }
});

test('measurement, regrouping, and refreshed records keep the first visible id at the same offset', () => {
  const before = groupRows(ids, 3, new Map());
  const top = before[30].offset + 47;
  const heights = new Map(ids.map((id, i) => [id, 110 + i % 4 * 40]));
  const measured = groupRows(ids, 3, heights);
  assert.equal(anchoredOffset(before, ids, measured, [...ids], top), measured[30].offset + 47);
  const list = groupRows(ids, 1, heights);
  assert.equal(anchoredOffset(before, ids, list, ids, top), list[90].offset + 47);
  const resized = groupRows(ids, 2, heights);
  assert.equal(anchoredOffset(before, ids, resized, ids, top), resized[45].offset + 47);
  const refreshed = ['new-model', ...ids];
  const next = groupRows(refreshed, 1, heights);
  assert.equal(anchoredOffset(before, ids, next, refreshed, top), next[91].offset + 47);
  assert.equal(anchoredOffset(before, ids, [], [], top), top);
});

test('604 models mount a bounded number of cards at typical viewport sizes', () => {
  for (const columns of [1, 2, 3, 4]) {
    const rows = groupRows(ids, columns, new Map());
    for (const top of [0, 2000, 15000]) {
      const mounted = renderedRows(rows, top, 800);
      const count = [...mounted].reduce((sum, i) => sum + rows[i].end - rows[i].start, 0);
      assert.ok(count <= 14 * columns, `${columns} columns at ${top}: ${count} cards`);
    }
  }
});


test('unmeasured rows use the mean measured card height, with a fallback for a new layout', () => {
  const heights = new Map([['a', 100], ['b', 300]]);
  assert.equal(meanHeight(heights), 200);
  assert.equal(groupRows(['a', 'b', 'c'], 1, heights, meanHeight(heights))[2].height, 200);
  assert.equal(meanHeight(new Map()), 220);
  assert.equal(meanHeight(new Map(), 180), 180);
  assert.equal(meanHeight(new Map([['hidden', 0], ['bad', NaN], ['a', 150]])), 150);
});

test('height invalidation follows columns and layout, retaining measurements within a column band', () => {
  assert.equal(invalidateHeights(columnCount(700, 'grid'), columnCount(1000, 'grid'), 'grid', 'grid'), false);
  assert.equal(invalidateHeights(columnCount(1000, 'grid'), columnCount(1040, 'grid'), 'grid', 'grid'), true);
  assert.equal(invalidateHeights(1, 1, 'grid', 'list'), true);
  assert.equal(invalidateHeights(1, 1, 'list', 'list'), false);
  assert.equal(invalidateHeights(0, 1, undefined, 'grid'), true);
});

test('anchor writes require more than half a pixel in either direction', () => {
  for (const delta of [0, 0.1, 0.5, -0.1, -0.5]) assert.equal(needsAnchorCorrection(100, 100 + delta), false);
  for (const delta of [0.501, -0.501, 200, -100]) assert.equal(needsAnchorCorrection(100, 100 + delta), true);
});

test('Library-owned state blocks duplicate definition actions across card remounts', () => {
  const entries = new Map([['a', createCardState()], ['b', createCardState()]]);
  const firstMount = entries.get('a');
  assert.equal(beginDefinitionAction(firstMount, 'def'), true);
  const remount = entries.get('a');
  assert.equal(beginDefinitionAction(remount, 'def'), false);
  assert.equal(beginDefinitionAction(remount, 'other-def'), true);
  assert.equal(beginDefinitionAction(entries.get('b'), 'def'), true);
  firstMount.busy.def = false;
  assert.equal(beginDefinitionAction(remount, 'def'), true);
});

test('fit cache shares in-flight and completed results by path and invalidates dependencies', async () => {
  const cache = new FitCache();
  let resolve;
  let calls = 0;
  const load = () => { calls++; return new Promise(done => { resolve = done; }); };
  const pending = cache.get('/model', 'hardware/settings/file-v1', load);
  assert.equal(cache.get('/model', 'hardware/settings/file-v1', load), pending);
  resolve({ category: 'full_gpu' });
  const result = await pending;
  assert.equal(await cache.get('/model', 'hardware/settings/file-v1', load), result);
  assert.equal(calls, 1);
  for (const key of ['new-hardware', 'new-settings', 'new-file']) {
    const next = cache.get('/model', key, async () => ({ category: key }));
    assert.notEqual(next, pending);
    assert.equal((await next).category, key);
  }
  assert.equal((await cache.get('/other-model', 'new-file', async () => ({ category: 'cpu_heavy' }))).category, 'cpu_heavy');
});

test('an obsolete failed fit cannot evict its replacement, and failures can retry', async () => {
  const cache = new FitCache();
  let reject;
  const old = cache.get('/model', 'old', () => new Promise((_, fail) => { reject = fail; }));
  const fresh = cache.get('/model', 'new', async () => ({ category: 'hybrid' }));
  reject(new Error('obsolete'));
  await assert.rejects(old);
  assert.equal(cache.get('/model', 'new', () => { throw new Error('must stay cached'); }), fresh);
  const failed = cache.get('/model', 'failed', async () => { throw new Error('retry'); });
  await assert.rejects(failed);
  assert.equal((await cache.get('/model', 'failed', async () => ({ category: 'full_gpu' }))).category, 'full_gpu');
});
