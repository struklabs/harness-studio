import { test } from 'node:test';
import assert from 'node:assert/strict';
import { officialMatches, mcpConfigTemplates } from '../src/lib/components/hub/githubDiscovery.ts';

test('official suggestions require an exact repository match', () => {
  const entries = [
    { id: 'match', source: 'https://github.com/Owner/Repo.git', homepage: '' },
    { id: 'homepage', source: '', homepage: 'https://github.com/owner/repo/' },
    { id: 'tree', source: 'https://github.com/owner/repo/tree/main/skill', homepage: '' },
    { id: 'prefix', source: 'https://github.com/owner/repo-other', homepage: '' },
    { id: 'spoof', source: 'https://github.com.evil/owner/repo', homepage: '' },
  ];
  assert.deepEqual(officialMatches('owner/repo', entries).map(e => e.id), ['match', 'homepage', 'tree']);
});

test('MCP copy includes only explicit templates and never the review metadata', () => {
  const local = { command: 'npx', args: ['-y', '@example/server@1.2.3'], env: { API_KEY: '<set API_KEY>' } };
  const remote = { url: 'https://example.com/mcp', headers: { Authorization: '<set Authorization>' } };
  const metadata = { config: [{ transport: 'unknown' }, { configTemplate: local }], registry: [{ config: [{ configTemplate: remote }] }], manifestHints: { npm: { scripts: { install: 'do not run' } } } };
  assert.deepEqual(mcpConfigTemplates(metadata), { mcpServers: { 'github-server-1': local, 'github-server-2': remote } });
  assert.deepEqual(mcpConfigTemplates(null), { mcpServers: {} });
});
