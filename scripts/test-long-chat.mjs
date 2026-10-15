import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  TAIL_MESSAGES, HISTORY_BATCH, tailStart, previousBatch, pathStart, anchoredScroll, needsOlder,
  estimateMessageHeight, settledWhile, renderCacheKey, RenderCache, whenIdle,
} from '../src/lib/components/playground/longChat.ts';

test('the tail window shows the last messages and walks back in batches', () => {
  assert.equal(tailStart(10), 0);
  assert.equal(tailStart(320), 320 - TAIL_MESSAGES);
  let start = tailStart(320), steps = 0;
  while (start > 0) { const next = previousBatch(start); assert.ok(next < start && start - next <= HISTORY_BATCH); start = next; steps++; }
  assert.equal(start, 0);
  assert.equal(previousBatch(5), 0);
  assert.equal(previousBatch(40, 30), 30);
  assert.ok(steps > 1);
});

test('a branch change keeps the first mounted message, or falls back to the tail', () => {
  const ids = Array.from({ length: 100 }, (_, i) => `m${i}`);
  assert.equal(pathStart(ids, 'm40'), 40);
  assert.equal(pathStart(ids, 'gone'), tailStart(100));
  assert.equal(pathStart(ids, undefined), tailStart(100));
});

test('scroll anchoring keeps the same content under the viewport', () => {
  assert.equal(anchoredScroll(100, 2000, 3500), 1600);
  assert.ok(needsOlder(5, 100) && !needsOlder(0, 100) && !needsOlder(5, 5000));
});

test('placeholder height grows with content and stays bounded', () => {
  assert.ok(estimateMessageHeight(0) >= 72);
  assert.ok(estimateMessageHeight(4000) > estimateMessageHeight(400));
  assert.equal(estimateMessageHeight(10_000_000), 4000);
});

test('a held derived does not recompute (or read its inputs) while streaming', () => {
  let streaming = false, reads = 0, token = 0;
  const get = settledWhile(() => streaming, () => { reads++; return token; });
  assert.equal(get(), 0);
  streaming = true; token = 1;
  assert.equal(get(), 0);
  assert.equal(reads, 1);
  streaming = false;
  assert.equal(get(), 1);
  const first = settledWhile(() => true, () => 'x');
  assert.equal(first(), 'x');
});

test('render cache keys never collide and the cache stays bounded', () => {
  assert.notEqual(renderCacheKey('a', 'bc'), renderCacheKey('ab', 'c'));
  assert.notEqual(renderCacheKey('a', 'b|c'), renderCacheKey('a|b', 'c'));
  const cache = new RenderCache(3, 1_000_000);
  for (const k of ['1', '2', '3']) cache.set(k, k, 10);
  cache.get('1');
  cache.set('4', '4', 10);
  assert.equal(cache.get('2'), undefined);
  assert.equal(cache.get('1'), '1');
  const tiny = new RenderCache(10, 100);
  tiny.set('big', 'v', 1000);
  assert.equal(tiny.get('big'), undefined);
  tiny.set('a', 'v', 20); tiny.set('b', 'v', 20); tiny.set('c', 'v', 20);
  assert.equal(tiny.get('a'), undefined);
  assert.equal(tiny.get('c'), 'v');
});

test('a held computation still recomputes when its key changes', () => {
  let streaming = true, length = 1, reads = 0;
  const get = settledWhile(() => streaming, () => ++reads, () => length);
  assert.equal(get(), 1);
  assert.equal(get(), 1);
  length = 2;
  assert.equal(get(), 2);
  assert.equal(get(), 2);
});

test('whenIdle falls back to a timer without requestIdleCallback and can be cancelled', async () => {
  assert.equal(typeof globalThis.requestIdleCallback, 'undefined');
  let ran = 0;
  whenIdle(() => ran++);
  const cancel = whenIdle(() => ran++);
  cancel();
  await new Promise((r) => setTimeout(r, 260));
  assert.equal(ran, 1);
});
