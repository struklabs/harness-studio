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
const { groupTemplates, paramsFromForm, parseComfySize, parseSourceKey, progressPercent, sourceKey } = await import('../src/lib/components/playground/comfyui.ts');

const form = (over = {}) => ({ prompt: ' a fox ', negative: ' blurry ', size: '', steps: '', cfgScale: '', seed: '', ...over });

test('workflow choices round-trip through their select keys', () => {
  for (const s of [{ kind: 'builtin' }, { kind: 'saved', path: 'a/b c.json' }, { kind: 'template', source: 'core', name: 'image_sdxl_simple' }]) {
    assert.deepEqual(parseSourceKey(sourceKey(s)), s);
  }
  assert.deepEqual(parseSourceKey('saved:'), { kind: 'builtin' });
  assert.deepEqual(parseSourceKey('template:core'), { kind: 'builtin' });
  assert.deepEqual(parseSourceKey('anything'), { kind: 'builtin' });
});

test('empty form fields keep the workflow values; seed -1 is random', () => {
  assert.deepEqual(paramsFromForm(form(), null), {
    prompt: 'a fox', negative: 'blurry', seed: null, width: null, height: null, steps: null, cfg: null, checkpoint: null,
  });
  const p = paramsFromForm(form({ size: '768x512', steps: '8', cfgScale: '2.5', seed: '42' }), 'sd.safetensors');
  assert.deepEqual([p.width, p.height, p.steps, p.cfg, p.seed, p.checkpoint], [768, 512, 8, 2.5, 42, 'sd.safetensors']);
  const odd = paramsFromForm(form({ size: '500x500', steps: 'x', cfgScale: 'abc', seed: '-1' }), '');
  assert.deepEqual([odd.width, odd.steps, odd.cfg, odd.seed, odd.checkpoint], [null, null, null, null, null]);
});

test('sizes follow the ComfyUI side limits', () => {
  assert.deepEqual(parseComfySize(' 1024 x 768 '), [1024, 768]);
  assert.deepEqual(parseComfySize('8192X64'), [8192, 64]);
  for (const bad of ['', '500x500', '56x64', '8200x64', '512', 'axb']) assert.equal(parseComfySize(bad), null, bad);
});

test('progress is a percentage only for sampler steps', () => {
  const e = (stage, value, max) => ({ prompt_id: 'p', stage, value, max, node: null });
  assert.equal(progressPercent(null), null);
  assert.equal(progressPercent(e('queued', 0, 0)), null);
  assert.equal(progressPercent(e('progress', 0, 0)), null);
  assert.equal(progressPercent(e('progress', 1, 4)), 25);
  assert.equal(progressPercent(e('progress', 9, 4)), 100);
});

test('templates are grouped by category, custom-node ones by module', () => {
  const tp = (source, name, category) => ({ source, name, title: name, description: '', category });
  const groups = groupTemplates([tp('core', 'a', 'Image'), tp('pack', 'u', ''), tp('core', 'b', 'Image')]);
  assert.deepEqual(groups.map(([g, items]) => [g, items.map((i) => i.name)]), [['Image', ['a', 'b']], ['pack', ['u']]]);
});
