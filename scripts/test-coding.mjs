import { test } from 'node:test';
import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';

// The helpers are pure; only the `invoke` import needs a stand-in.
registerHooks({
  resolve(specifier, context, next) {
    if (specifier === '#lib/ipc.js') return { url: 'data:text/javascript,export const invoke = async () => {};', shortCircuit: true };
    return next(specifier, context);
  },
});
const { applyEvent, diffLines, starters, totals, warningKey } = await import('../src/lib/coding.ts');

test('a diff is split into header, hunk, added, removed and context lines', () => {
  const lines = diffLines('--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n\\ No newline at end of file\n');
  assert.deepEqual(lines.map((l) => l.kind), ['meta', 'meta', 'hunk', 'ctx', 'del', 'add', 'meta']);
  // A removed line that starts with dashes is still a removal inside a hunk.
  assert.equal(diffLines('@@ -1 +1 @@\n--- x\n').at(-1).kind, 'del');
  assert.deepEqual(diffLines(''), [{ kind: 'meta', text: '' }]);
});

test('totals add the changed files and lines', () => {
  const change = (additions, deletions) => ({ path: 'x', status: 'modified', additions, deletions, binary: false, diff: '' });
  assert.deepEqual(totals([change(3, 1), change(0, 2)]), { files: 2, additions: 3, deletions: 3 });
  assert.deepEqual(totals([]), { files: 0, additions: 0, deletions: 0 });
});

test('a call event adds a step once and later events update it in place', () => {
  const call = (status, id = 'c1') => ({ type: 'call', run_id: 'r', call_id: id, tool: 'edit_file', summary: 'a.rs', status });
  let steps = applyEvent([], call('running'));
  steps = applyEvent(steps, call('done'));
  steps = applyEvent(steps, call('running', 'c2'));
  assert.deepEqual(steps.map((s) => [s.key, s.status]), [['c1', 'done'], ['c2', 'running']]);
  // Calls the workspace rules refused before they started have no id: each one is its own row.
  steps = applyEvent(steps, call('denied', ''));
  steps = applyEvent(steps, call('denied', ''));
  assert.equal(steps.length, 4);
  assert.equal(applyEvent(steps, { type: 'round', run_id: 'r', round: 2 }), steps);
});

test('command warnings and starters map to locale keys', () => {
  assert.equal(warningKey('discard'), 'coding.warn_discard');
  assert.equal(warningKey('something-new'), 'coding.warn_unknown');
  assert.deepEqual(starters.map((s) => s.id), ['write', 'fix', 'refactor', 'test']);
});
