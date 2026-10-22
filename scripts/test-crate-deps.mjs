// The crate dependency rules: what they allow and what they refuse.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { check, RULES } = await import('./check-crate-deps.mjs');

const pkg = (name, deps = []) => ({
  name,
  dependencies: deps.map((d) => (typeof d === 'string' ? { name: d, kind: null } : d)),
});
/** A workspace that follows RULES exactly. */
const clean = () => ({ packages: Object.entries(RULES).map(([name, deps]) => pkg(name, deps)) });
const withDep = (from, dep) => {
  const m = clean();
  m.packages.find((p) => p.name === from).dependencies.push(typeof dep === 'string' ? { name: dep, kind: null } : dep);
  return m;
};

test('a workspace that follows the layers passes', () => {
  assert.deepEqual(check(clean()), []);
});

test('the gateway must not depend on the UI crate, the CLI or the Control API', () => {
  for (const up of ['harness-studio', 'harness-cli', 'harness-api', 'harness-mcp']) {
    const problems = check(withDep('harness-gateway', up));
    assert.equal(problems.length, 1, up);
    assert.match(problems[0], new RegExp(`harness-gateway -> ${up}`));
  }
});

test('the core stays at the bottom: only the ONNX sidecar below it', () => {
  assert.equal(check(withDep('harness-core', 'harness-gateway')).length, 1);
  assert.equal(check(withDep('harness-onnx', 'harness-core')).length, 1);
});

test('dev dependencies count too', () => {
  const problems = check(withDep('harness-gateway', { name: 'harness-api', kind: 'dev' }));
  assert.match(problems[0], /harness-api \(dev\)/);
});

test('only the shell may use the desktop UI toolkit', () => {
  assert.equal(check(withDep('harness-studio', 'tauri')).length, 0);
  assert.equal(check(withDep('harness-studio', 'tauri-plugin-opener')).length, 0);
  const problems = check(withDep('harness-core', 'tauri'));
  assert.match(problems[0], /only harness-studio may use the desktop UI toolkit/);
  assert.equal(check(withDep('harness-gateway', 'wry')).length, 1);
  // A crate that merely starts with the letters is not the toolkit.
  assert.equal(check(withDep('harness-core', 'taoism')).length, 0);
});

test('a new crate has to take a place in the layers', () => {
  const m = clean();
  m.packages.push(pkg('harness-new', ['harness-core']));
  assert.match(check(m)[0], /harness-new: workspace crate has no layer/);
  const gone = clean();
  gone.packages = gone.packages.filter((p) => p.name !== 'harness-mcp');
  assert.ok(check(gone).some((p) => /harness-mcp: listed in RULES but not a workspace member/.test(p)));
});
