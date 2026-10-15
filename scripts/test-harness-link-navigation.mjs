import test from 'node:test';
import assert from 'node:assert/strict';
import { resolveConnectionPage, connectionPageFromUrl } from '../src/lib/components/connections/navigationResolver.js';

test('remote connections page resolves from saved navigation and deep links', () => {
  assert.equal(resolveConnectionPage('harness-link'), 'harness-link');
  assert.equal(connectionPageFromUrl('https://localhost/?connection=harness-link'), 'harness-link');
  assert.equal(connectionPageFromUrl('https://localhost/#connections/harness-link'), 'harness-link');
  assert.equal(resolveConnectionPage('clients'), 'config');
  assert.equal(resolveConnectionPage('unknown'), null);
});
