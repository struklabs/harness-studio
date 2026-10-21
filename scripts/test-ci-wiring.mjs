import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';

// Every scripts/test-*.mjs runs in CI: listed by name in an npm script or in a workflow.
test('every node test script is listed where CI runs it', () => {
  const listed = readFileSync('package.json', 'utf8') + readFileSync('.github/workflows/ci.yml', 'utf8');
  const missing = readdirSync('scripts')
    .filter((f) => /^test-.*\.mjs$/.test(f))
    .filter((f) => !listed.includes(`scripts/${f}`));
  assert.deepEqual(missing, [], 'add these to "test:unit" in package.json');
});
