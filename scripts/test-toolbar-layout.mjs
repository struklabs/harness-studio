import test from 'node:test';
import assert from 'node:assert/strict';
import { shownItems, splitItems, inlineCount, parseHidden } from '../src/lib/toolbarLayout.js';

test('simple level shows only the basic buttons', () => {
  assert.deepEqual(shownItems([], false), ['sidebar', 'downloads', 'server']);
  assert.equal(shownItems([], true).length, 5);
});

test('hidden buttons are dropped', () => {
  assert.deepEqual(shownItems(['downloads', 'logs'], true), ['sidebar', 'server', 'folder']);
});

test('narrow windows move buttons into the overflow', () => {
  const all = shownItems([], true);
  assert.equal(inlineCount(1200), 5);
  assert.deepEqual(splitItems(all, 600), { inline: ['sidebar', 'downloads'], overflow: ['server', 'logs', 'folder'] });
  assert.equal(splitItems(all, 400).inline.length, 1);
});

test('parseHidden ignores garbage', () => {
  assert.deepEqual(parseHidden('not json'), []);
  assert.deepEqual(parseHidden('["logs","bogus"]'), ['logs']);
  assert.deepEqual(parseHidden(null), []);
});
