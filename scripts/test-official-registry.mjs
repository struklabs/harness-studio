import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { filterCatalog, mergeRegistryServers } from '../src/lib/components/tools/registryCatalog.ts';

const { entries } = JSON.parse(readFileSync(new URL('../crates/harness-core/src/tools/fixtures/registry-index.json', import.meta.url), 'utf8'));

test('catalog searches both languages and tags while respecting the type filter', () => {
  assert.equal(filterCatalog(entries, ' JSON ', 'all', 'name', 'en')[0].type, 'tool');
  assert.equal(filterCatalog(entries, 'JSON', 'skill', 'name', 'en').length, 0);
  assert.equal(filterCatalog(entries, 'writing', 'skill', 'name', 'tr')[0].name, 'Release Notes');
  assert.equal(filterCatalog(entries, 'SUNUCUSU', 'mcp', 'name', 'tr')[0].name, 'Microsoft Learn');
  assert.equal(filterCatalog(entries, 'not present', 'all', 'name', 'en').length, 0);
});

test('catalog sorts deterministically without mutating the source snapshot', () => {
  const original = entries.map(e => e.id);
  assert.deepEqual(filterCatalog(entries, '', 'all', 'name', 'en').map(e => e.name), ['JSON Formatter', 'Microsoft Learn', 'Release Notes']);
  assert.deepEqual(filterCatalog(entries, '', 'all', 'type', 'en').map(e => e.type), ['mcp', 'skill', 'tool']);
  assert.deepEqual(entries.map(e => e.id), original);
});

test('MCP merge preserves other configs and credentials and refuses name collisions', () => {
  const existing = { private: { command: 'local', env: { TOKEN: 'masked' }, disabled: true } };
  const incoming = { learn: { url: 'https://learn.microsoft.com/api/mcp', transport: 'http' } };
  const merged = mergeRegistryServers(existing, incoming);
  assert.deepEqual(merged, { ...existing, ...incoming });
  assert.equal(mergeRegistryServers(existing, { private: incoming.learn }), null);
  assert.equal(Object.keys(existing).length, 1);
});
