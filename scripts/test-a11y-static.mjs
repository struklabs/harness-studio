import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const read = (p) => readFileSync(new URL(`../${p}`, import.meta.url), 'utf8');

test('the app behind the wizard is inert exactly while the wizard is shown', () => {
  const page = read('src/routes/+page.svelte');
  const shown = page.match(/\{#if (!settings\.privacy\.onboarding_completed \|\| showWizardManually)\}\s*<LazyView\s+load=\{views\.onboarding\}/);
  assert.ok(shown, 'wizard condition not found');
  assert.ok(page.includes(`inert={${shown[1]}}`), 'the content is not inert under the same condition');
});

test('the dialog action restores focus after the update that removes it', () => {
  const action = read('src/lib/actions/dialog.ts');
  assert.match(action, /queueMicrotask\(\(\) => \{\s*if \(previouslyFocused[^\n]*\.focus\(\)/);
});

test('the command palette names its result list and the page has the app title', () => {
  assert.match(read('src/lib/components/CommandPalette.svelte'), /<Command\.List aria-label=\{t\('cmd\.results'\)\}>/);
  assert.match(read('src/app.html'), /<title>Harness Studio<\/title>/);
});
