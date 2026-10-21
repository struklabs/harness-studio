import assert from 'node:assert/strict';
import { test } from 'node:test';
const { formatClock, formatTimeSpan, averageMs } = await import('../src/lib/components/playground/kbFormat.ts');

test('clock and spans', () => {
  assert.equal(formatClock(5000), '0:05');
  assert.equal(formatClock(3723000), '1:02:03');
  assert.equal(formatTimeSpan(65000, 92000), '1:05-1:32');
  assert.equal(formatTimeSpan(5000, 5400), '0:05');
  assert.equal(formatTimeSpan(5000, null), '0:05');
  assert.equal(formatTimeSpan(null, null), '');
});

test('average latency', () => {
  assert.equal(averageMs(300, 3), 100);
  assert.equal(averageMs(0, 0), null);
});
