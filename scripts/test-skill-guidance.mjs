import assert from 'node:assert/strict';
import { test } from 'node:test';
import { withSkillGuidance } from '../src/lib/components/playground/skillGuidance.ts';

test('skill catalog includes only enabled metadata and leaves history unchanged', () => {
  const messages = [{role: 'system', content: 'User instructions'}, {role: 'user', content: 'Draft release notes'}];
  const skills = [
    { name: 'release-notes', description: 'Draft notes', enabled: true, error: null, body: 'Secret full instructions' },
    { name: 'disabled', description: 'Disabled skill', enabled: false, error: null },
    { name: 'broken', description: 'Invalid skill', enabled: true, error: 'Broken metadata' },
  ];
  const result = withSkillGuidance(messages, skills);
  assert.match(result[0].content, /release-notes/);
  assert.match(result[0].content, /load_skill/);
  assert.match(result[0].content, /resources only as needed/);
  assert.doesNotMatch(result[0].content, /Secret full instructions|Disabled skill|Invalid skill/);
  assert.equal(messages[0].content, 'User instructions');
  assert.equal(result[1], messages[1]);
});

test('empty catalogs add nothing; a catalog creates a system message when needed', () => {
  const messages = [{ role: 'user', content: 'Hello' }];
  assert.equal(withSkillGuidance(messages, []), messages);
  const result = withSkillGuidance(messages, [{ name: 'example', description: 'Examples', enabled: true, error: null }]);
  assert.equal(result[0].role, 'system');
  assert.equal(result[1], messages[0]);
});
