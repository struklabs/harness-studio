import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createInflight, inflightKey } from '../src/lib/inflight.ts';

test('concurrent callers with one key share a single run', async () => {
  const shared = createInflight();
  let runs = 0;
  const run = async () => {
    runs++;
    await new Promise((r) => setTimeout(r, 5));
    return runs;
  };
  const [a, b, c] = await Promise.all([shared('k', run), shared('k', run), shared('k', run)]);
  assert.equal(runs, 1);
  assert.deepEqual([a, b, c], [1, 1, 1]);
});

test('a settled call is not reused', async () => {
  const shared = createInflight();
  let runs = 0;
  const run = async () => ++runs;
  assert.equal(await shared('k', run), 1);
  assert.equal(await shared('k', run), 2);
});

test('different keys run separately and a failure is not cached', async () => {
  const shared = createInflight();
  let runs = 0;
  await Promise.all([shared('a', async () => ++runs), shared('b', async () => ++runs)]);
  assert.equal(runs, 2);
  await assert.rejects(shared('x', async () => { throw new Error('boom'); }), /boom/);
  assert.equal(await shared('x', async () => 'ok'), 'ok');
});

test('keys separate commands, arguments and epochs', () => {
  assert.notEqual(inflightKey('scan'), inflightKey('scan', { a: 1 }));
  assert.notEqual(inflightKey('scan', undefined, 1), inflightKey('scan', undefined, 2));
  assert.equal(inflightKey('f', { id: 'x' }), inflightKey('f', { id: 'x' }));
});
