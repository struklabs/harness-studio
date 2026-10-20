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
  assert.deepEqual(moduleActions(view('not_installed', { manifest: { planned: true } })), { install: false, open: false, remove: false, cancel: false });
  assert.deepEqual(moduleActions(view('installing')), { install: false, open: false, remove: false, cancel: true });
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
