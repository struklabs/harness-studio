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
const { fileName, folderLines, formatSize, inputOf, projectAllowsSkill, projectAllowsTool } = await import('../src/lib/projects.ts');

const project = (over = {}) => ({ tools: [], skills: [], mcp_servers: [], ...over });

test('a project without attachments offers every tool and skill', () => {
  for (const p of [null, undefined, project()]) {
    assert.equal(projectAllowsTool(p, { name: 'read_file' }), true);
    assert.equal(projectAllowsTool(p, { name: 'search', source: 'mcp', server: 'web' }), true);
    assert.equal(projectAllowsSkill(p, 'summarize'), true);
  }
});

test('attached tools, MCP servers and skills limit only their own kind', () => {
  const p = project({ tools: ['read_file'], mcp_servers: ['web'], skills: ['summarize'] });
  assert.equal(projectAllowsTool(p, { name: 'read_file' }), true);
  assert.equal(projectAllowsTool(p, { name: 'write_file' }), false);
  assert.equal(projectAllowsTool(p, { name: 'search', source: 'mcp', server: 'web' }), true);
  assert.equal(projectAllowsTool(p, { name: 'search', source: 'mcp', server: 'git' }), false);
  assert.equal(projectAllowsTool(p, { name: 'search', source: 'mcp', server: null }), false);
  assert.equal(projectAllowsSkill(p, 'summarize'), true);
  assert.equal(projectAllowsSkill(p, 'translate'), false);
  const toolsOnly = project({ tools: ['read_file'] });
  assert.equal(projectAllowsTool(toolsOnly, { name: 'search', source: 'mcp', server: 'git' }), true);
});

test('folder lines drop blanks and trim; file names work for both separators', () => {
  assert.deepEqual(folderLines('  /a/b \r\n\n C:\\work\\x\n   '), ['/a/b', 'C:\\work\\x']);
  assert.equal(fileName('/p/outputs/fox.png'), 'fox.png');
  assert.equal(fileName('C:\\p\\outputs\\fox.png'), 'fox.png');
  assert.equal(fileName('/p/outputs/'), 'outputs');
  assert.equal(formatSize(512), '512 B');
  assert.equal(formatSize(1536), '1.5 KB');
  assert.equal(formatSize(3 * 1024 * 1024), '3.0 MB');
});

test('editing copies the attachment lists', () => {
  const p = { name: 'n', description: '', folder: '/p', allowed_folders: ['/x'], tools: ['t'], skills: [], mcp_servers: [] };
  const input = inputOf(p);
  input.tools.push('u');
  input.allowed_folders.push('/y');
  assert.deepEqual(p.tools, ['t']);
  assert.deepEqual(p.allowed_folders, ['/x']);
});
