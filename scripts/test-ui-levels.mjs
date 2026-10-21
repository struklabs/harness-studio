import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

// Evaluate the production policy and declarative menus without a Svelte/browser runtime.
async function sourceModule(path) {
  const source = readFileSync(new URL(`../src/lib/${path}`, import.meta.url), 'utf8')
    .replace(/import \{([^}]+)\} from '@lucide\/svelte';/g, (_, names) =>
      `const ${names.split(',').map(name => `${name.trim()} = null`).join(', ')};`)
    .replace(/^import type .*;$/gm, '');
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 },
  });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
}

const { visibleGroups, initialPage, pageLevel, tabLevel } = await sourceModule('uiLevel.ts');
const menus = await sourceModule('components/common/screenSections.ts');
const { CONNECTION_GROUPS } = await sourceModule('components/connections/navigation.ts');
const { SETTINGS_GROUPS } = await sourceModule('components/settings/sections.ts');
const { PLAYGROUND_MODES } = await sourceModule('components/playground/navigation.ts');
const ids = groups => groups.flatMap(group => group.pages.map(page => page.id));

test('Advanced retains every declared page; filtering never mutates the menu', () => {
  for (const groups of [...Object.values(menus), CONNECTION_GROUPS, SETTINGS_GROUPS]) {
    const before = JSON.stringify(groups);
    const advanced = ids(groups);
    visibleGroups(groups, 'simple');
    assert.deepEqual(ids(visibleGroups(groups, 'advanced')), advanced);
    assert.equal(JSON.stringify(groups), before);
    assert.equal(new Set(advanced).size, advanced.length);
  }
});

test('Simple keeps core journeys and removes technical library categories', () => {
  assert.deepEqual(ids(visibleGroups(menus.DASHBOARD_GROUPS, 'simple')), ['overview', 'model', 'context']);
  assert.deepEqual(ids(visibleGroups(menus.LIBRARY_GROUPS, 'simple')), ['all', 'llm', 'vlm', 'asr', 'image', 'tts', 'import']);
  assert.deepEqual(ids(visibleGroups(CONNECTION_GROUPS, 'simple')), ['config', 'guide', 'harness-link', 'cloud-providers', 'endpoints']);
  assert.deepEqual(ids(visibleGroups(SETTINGS_GROUPS, 'simple')), ['runtime-installed', 'storage-directories', 'storage-hf', 'tools-installed', 'automations', 'appearance', 'updates', 'privacy']);
  assert.deepEqual(PLAYGROUND_MODES.filter(mode => !mode.level).map(mode => mode.id), ['chat', 'studio', 'transcription', 'image', 'ocr']);
  for (const tab of ['dashboard', 'hub', 'library', 'playground', 'connections', 'settings']) assert.equal(tabLevel(tab), undefined);
  for (const tab of ['servers', 'create', 'performance', 'analytics']) assert.equal(tabLevel(tab), 'advanced');
});

test('restoration falls back to a visible page without removing deep-link targets', () => {
  for (const groups of [menus.DASHBOARD_GROUPS, menus.LIBRARY_GROUPS, CONNECTION_GROUPS, SETTINGS_GROUPS]) {
    const fallback = ids(visibleGroups(groups, 'simple'))[0];
    for (const id of ids(groups)) {
      assert.equal(initialPage(groups, id, 'advanced'), id);
      assert.equal(initialPage(groups, id, 'simple'), pageLevel(groups, id) === 'advanced' ? fallback : id);
    }
  }
  assert.equal(pageLevel(SETTINGS_GROUPS, 'network-listen'), 'advanced');
  assert.equal(pageLevel(menus.LIBRARY_GROUPS, 'embedding'), 'advanced');
});

test('group visibility is inherited and invalid saved pages recover safely', () => {
  const groups = [
    { pages: [{ id: 'home' }, { id: 'expert', level: 'advanced' }] },
    { level: 'advanced', pages: [{ id: 'inherited' }] },
  ];
  assert.deepEqual(ids(visibleGroups(groups, 'simple')), ['home']);
  assert.equal(pageLevel(groups, 'inherited'), 'advanced');
  assert.equal(initialPage(groups, 'missing', 'simple'), 'home');
  assert.equal(initialPage(groups, 'missing', 'advanced'), 'home');
  assert.equal(initialPage(groups, 'inherited', 'advanced'), 'inherited');
});
