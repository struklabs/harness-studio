import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ensureEngine, missingEngineFromError, runWithEngine, autoInstallFamily, EngineInstallFailed } from '../src/lib/engineAuto.ts';

function deps(over = {}) {
  const installedSet = new Set(over.installed ?? []);
  const calls = [];
  return {
    calls,
    installedSet,
    installed: (f) => installedSet.has(f),
    canInstall: (f) => f !== 'mistral.rs',
    offered: () => true,
    install: async (f) => {
      calls.push(['install', f]);
      if (over.failInstall) return false;
      installedSet.add(f);
      return true;
    },
    installManaged: async () => {
      calls.push(['managed']);
      return !over.failManaged;
    },
    raw: (e) => String(e?.message ?? e),
    ...over.extra,
  };
}

test('ensureEngine installs a missing family and skips an installed one', async () => {
  const d = deps({ installed: ['python-hf'] });
  assert.equal(await ensureEngine('python-hf', d), 'ready');
  assert.deepEqual(d.calls, []);
  assert.equal(await ensureEngine('whisper.cpp', d), 'ready');
  assert.deepEqual(d.calls, [['install', 'whisper.cpp']]);
});

test('ensureEngine reports manual families and failed installs without falling back', async () => {
  assert.equal(await ensureEngine('mistral.rs', deps()), 'manual');
  const off = deps({ extra: { offered: () => false } });
  assert.equal(await ensureEngine('onnxruntime', off), 'manual');
  assert.equal(await ensureEngine('whisper.cpp', deps({ failInstall: true })), 'failed');
});

test('missingEngineFromError names the family or the managed llama.cpp build', () => {
  assert.equal(missingEngineFromError('boom: runtime.family_not_installed: whisper.cpp'), 'whisper.cpp');
  assert.equal(missingEngineFromError('runtime.not_installed'), 'llama.cpp');
  assert.equal(missingEngineFromError('ocr.rasterizer_unavailable'), 'python-hf');
  assert.equal(missingEngineFromError('network.lan_without_key'), null);
});

test('runWithEngine installs the engine a failed action needs, then runs the action again', async () => {
  const d = deps();
  const seen = [];
  d.onInstalling = (f) => seen.push(`start:${f}`);
  d.onDone = (f) => seen.push(`done:${f}`);
  let runs = 0;
  const result = await runWithEngine(async () => {
    runs++;
    if (!d.installedSet.has('python-hf')) throw new Error('x: runtime.family_not_installed: python-hf');
    return 'loaded';
  }, d);
  assert.equal(result, 'loaded');
  assert.equal(runs, 2);
  assert.deepEqual(d.calls, [['install', 'python-hf']]);
  assert.deepEqual(seen, ['start:python-hf', 'done:python-hf']);
});

test('runWithEngine chains two different engines and the managed llama.cpp build', async () => {
  const d = deps();
  let managed = false;
  d.installManaged = async () => ((managed = true), true);
  const result = await runWithEngine(async () => {
    if (!managed) throw new Error('runtime.not_installed');
    if (!d.installedSet.has('mlx')) throw new Error('runtime.family_not_installed: mlx');
    return 'ok';
  }, d);
  assert.equal(result, 'ok');
  assert.deepEqual(d.calls, [['install', 'mlx']]);
});

test('scanned PDF retries after installing Python, then installs its VLM engine', async () => {
  const d = deps();
  let managed = false;
  d.installManaged = async () => ((managed = true), d.calls.push(['managed']), true);
  const result = await runWithEngine(async () => {
    if (!d.installedSet.has('python-hf')) throw new Error('ocr.rasterizer_unavailable');
    if (!managed) throw new Error('runtime.not_installed');
    return 'page text';
  }, d);
  assert.equal(result, 'page text');
  assert.deepEqual(d.calls, [['install', 'python-hf'], ['managed']]);
});

test('runWithEngine surfaces a failed install and never retries the same engine twice', async () => {
  await assert.rejects(
    runWithEngine(async () => { throw new Error('runtime.family_not_installed: whisper.cpp'); }, deps({ failInstall: true })),
    (e) => e instanceof EngineInstallFailed && e.family === 'whisper.cpp' && e.outcome === 'failed',
  );
  const d = deps();
  let runs = 0;
  await assert.rejects(
    runWithEngine(async () => { runs++; throw new Error('runtime.family_not_installed: whisper.cpp'); }, d),
    /family_not_installed/,
  );
  assert.equal(runs, 2);
  assert.equal(d.calls.length, 1);
  await assert.rejects(runWithEngine(async () => { throw new Error('runtime.not_installed'); }, deps({ failManaged: true })), EngineInstallFailed);
});

test('runWithEngine passes unrelated failures through untouched', async () => {
  const d = deps();
  await assert.rejects(runWithEngine(async () => { throw new Error('network.lan_without_key'); }, d), /lan_without_key/);
  assert.deepEqual(d.calls, []);
});

test('autoInstallFamily picks only installable, missing families of a needs_runtime model', () => {
  const d = deps({ installed: ['sherpa-onnx'] });
  assert.equal(autoInstallFamily({ status: 'needs_runtime', runtime: 'whisper.cpp' }, d), 'whisper.cpp');
  assert.equal(autoInstallFamily({ status: 'needs_runtime', runtime: 'sherpa-onnx' }, d), null);
  assert.equal(autoInstallFamily({ status: 'needs_runtime', runtime: 'mistral.rs' }, d), null);
  assert.equal(autoInstallFamily({ status: 'runnable', runtime: 'whisper.cpp' }, d), null);
});

test('Turkish UI strings never say "çalışma zamanı" for the model engine', async () => {
  const { readdirSync } = await import('node:fs');
  const { pathToFileURL } = await import('node:url');
  const dir = new URL('../src/lib/locales/', import.meta.url);
  const bad = [];
  for (const f of readdirSync(dir).filter((n) => n.endsWith('.ts') && n !== 'index.ts')) {
    const tr = (await import(new URL(f, dir).href)).default?.tr ?? {};
    for (const [k, v] of Object.entries(tr)) if (/çalışma\s+zaman|ÇALIŞMA\s+ZAMAN|çalışma motoru/i.test(v)) bad.push(`${f}:${k}`);
  }
  assert.deepEqual(bad, []);
});
