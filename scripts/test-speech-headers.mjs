import { test } from 'node:test';
import assert from 'node:assert/strict';
import { truncationOf } from '../src/lib/components/playground/speechHeaders.ts';

test('x-harness-truncated reads as a reason; none is a whole answer', () => {
  assert.equal(truncationOf(null), null);
  assert.equal(truncationOf('time'), 'time');
  assert.equal(truncationOf('frames'), 'frames');
  // A reason this build does not know (a newer worker) still says the audio is partial.
  assert.equal(truncationOf('something-new'), 'time');
  assert.equal(truncationOf(''), 'time');
});
