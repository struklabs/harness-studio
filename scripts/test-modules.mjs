import { test } from 'node:test';
import assert from 'node:assert/strict';
import { moduleActions, progressPercent } from '../src/lib/modules.ts';
const view = (status, options = {}) => ({ manifest: { planned: false }, state: { status }, incompatibility: null, progress: null, ...options });
test('module actions permit recovery and protect planned, incompatible, and active modules', () => {
  assert.equal(moduleActions(view('not_installed')).install, true);
  assert.equal(moduleActions(view('failed')).install, true);
  assert.equal(moduleActions(view('failed')).remove, true);
  assert.equal(moduleActions(view('installed')).open, true);
  assert.equal(moduleActions(view('installed')).install, false);
  assert.equal(moduleActions(view('not_installed'), true).install, false);
  assert.equal(moduleActions(view('not_installed', { incompatibility: 'platform' })).install, false);
  assert.deepEqual(moduleActions(view('not_installed', { manifest: { planned: true } })), { install: false, open: false, enable: false, disable: false, repair: false, remove: false, cancel: false });
  assert.deepEqual(moduleActions(view('installing')), { install: false, open: false, enable: false, disable: false, repair: false, remove: false, cancel: true });
  assert.equal(moduleActions(view('installed', { progress: {} })).remove, false);
});
test('unknown progress stays indeterminate and byte progress is bounded', () => {
  assert.equal(progressPercent(0, 0), undefined);
  assert.equal(progressPercent(50, 100), 50);
  assert.equal(progressPercent(200, 100), 100);
  assert.equal(progressPercent(-2, 100), 0);
  assert.equal(progressPercent(NaN, 100), undefined);
});
import { readFileSync } from 'node:fs';
import moduleLocales from '../src/lib/locales/modules.ts';
import agentLocales from '../src/lib/locales/agents.ts';
const locales = { en: { ...moduleLocales.en, ...agentLocales.en }, tr: { ...moduleLocales.tr, ...agentLocales.tr } };
test('every bundled module has translated metadata and future areas stay planned', () => {
  const modules = JSON.parse(readFileSync(new URL('../crates/harness-core/src/modules/bundled.json', import.meta.url), 'utf8'));
  assert.deepEqual(modules.filter(m => !m.planned).map(m => m.id), ['speech', 'agents']);
  for (const manifest of modules) {
    for (const field of [manifest.name, manifest.description, manifest.hardware, ...manifest.functions, ...manifest.permissions]) {
      assert.equal(typeof locales.en[field.en], 'string');
      assert.equal(typeof locales.tr[field.tr], 'string');
    }
  }
  assert.equal(locales.tr['modules.title'], 'Modüller');
});

test('disabled modules retain recovery actions and cannot be opened or reinstalled', () => {
  const actions = moduleActions(view('disabled'));
  assert.equal(actions.install, false);
  assert.equal(actions.open, false);
  assert.equal(actions.enable, true);
  assert.equal(actions.disable, false);
  assert.equal(actions.repair, true);
  assert.equal(actions.remove, true);
  assert.equal(moduleActions(view('disabled'), true).enable, false);
  assert.equal(moduleActions(view('installed', { incompatibility: 'contract' })).remove, false);
});

test('uninstall hooks preserve data and module removal choices default to keeping it', () => {
  const hooks = readFileSync(new URL('../src-tauri/windows/hooks.nsh', import.meta.url), 'utf8');
  assert.doesNotMatch(hooks, /^\s*(?:RMDir|Delete)\s/im);
  const ui = readFileSync(new URL('../src/lib/components/Modules.svelte', import.meta.url), 'utf8');
  assert.match(ui, /deleteHistory = \$state\(false\)/);
  assert.match(ui, /deleteLogs = \$state\(false\)/);
  assert.match(ui, /choice\.disk_bytes/);
  for (const key of ['modules.disabled', 'modules.enable', 'modules.delete_history', 'modules.delete_logs']) {
    assert.ok(locales.en[key]); assert.ok(locales.tr[key]);
    assert.ok(!locales.tr[key].includes('?'), key);
  }
});

test('Hermes manifest pin matches the staged backend checkout', () => {
  const source = readFileSync(new URL('../crates/harness-core/src/modules/agents.rs', import.meta.url), 'utf8');
  const modules = JSON.parse(readFileSync(new URL('../crates/harness-core/src/modules/bundled.json', import.meta.url), 'utf8'));
  const pin = modules.find(m => m.id === 'agents').components[0].version;
  assert.match(pin, /^[a-f0-9]{40}$/);
  assert.ok(source.includes(`HERMES_REVISION: &str = "${pin}"`));
  assert.match(source, /\.args\(\["-Commit", HERMES_REVISION\]\)/);
});

import { moduleStatusKey } from '../src/lib/modules.ts';
test('a module with deleted files reads Needs Repair and offers only repair and remove', () => {
  const broken = view('installed', { needs_repair: ['transcribe-cpp'] });
  assert.equal(moduleStatusKey(broken), 'needs_repair');
  const actions = moduleActions(broken);
  assert.equal(actions.repair, true);
  assert.equal(actions.remove, true);
  assert.equal(actions.open, false);
  assert.equal(actions.disable, false);
  assert.equal(moduleActions(view('disabled', { needs_repair: ['hermes'] })).enable, false);
  assert.equal(moduleStatusKey(view('installed', { needs_repair: [] })), 'installed');
  assert.equal(moduleStatusKey(view('failed', { needs_repair: ['x'] })), 'failed');
  assert.equal(locales.en['modules.needs_repair'], 'Needs Repair');
  assert.equal(typeof locales.tr['modules.needs_repair'], 'string');
});
