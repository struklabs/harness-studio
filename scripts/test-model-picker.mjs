import { test } from 'node:test';
import assert from 'node:assert/strict';
import { pathKey, groupModels, flattenGroups, filterRows, rowOffsets, rowWindow, applyMeasurements, scrollToRow, navigationIndex, WINDOW_THRESHOLD } from '../src/lib/components/dashboard/model/modelPicker.ts';

const model = (id, extra = {}) => ({ id: String(id), name: `Model ${id}`, filename: `file-${id}.gguf`, path: `C:/models/${id}.gguf`, shard_paths: [], quantization: 'Q4_K_M', ...extra });
const def = (id, path, alias = id) => ({ id, alias, base: { kind: 'local', path } });

test('path index preserves samePath separator, trailing slash and case semantics', () => {
  assert.equal(pathKey('C:\\Models\\FILE.gguf\\'), pathKey('c:/models/file.gguf/'));
  assert.notEqual(pathKey('a//b'), pathKey('a/b'));
  const models = [model(0, { shard_paths: ['C:/models/0.gguf', 'c:/models/shard.gguf'] }), model(1)];
  const definitions = [def('z', 'C:\\MODELS\\SHARD.gguf/', 'Zulu'), def('a', models[0].path, 'Alpha'), def('other', '/missing')];
  definitions.push({ id: 'remote', alias: 'Remote', base: { kind: 'hub', path: models[0].path } });
  const groups = groupModels(models, definitions);
  assert.deepEqual(groups.map((g) => g.defs.map((d) => d.id)), [['a', 'z'], []]);
  assert.deepEqual(definitions.slice(0, 2).map((d) => d.id), ['z', 'a']);
});

test('standalone file, single definition and multiple definitions retain selection values', () => {
  const models = [model(0), model(1), model(2), model(3, { kind: 'mmproj' }), model(4, { kind: 'adapter' })];
  const rows = flattenGroups(groupModels(models, [def('only', models[1].path), def('b', models[2].path), def('a', models[2].path)]));
  assert.deepEqual(rows.map((r) => [r.value, r.file]), [['f:0', true], ['d:only', true], ['f:2', true], ['d:a', false], ['d:b', false]]);
});

test('search covers name, filename, quantization, and single or nested definition aliases', () => {
  const models = [model(0, { name: 'Llama Special', filename: 'weights-unique.gguf' }), model(1)];
  const rows = flattenGroups(groupModels(models, [def('one', models[0].path, 'Assistant'), def('two', models[1].path, 'Coding'), def('three', models[1].path, 'Reasoning')]));
  for (const q of ['SPECIAL', 'Weights-Unique', 'q4_k_m', 'Assistant']) assert.ok(filterRows(rows, q).some((r) => r.value === 'd:one'));
  assert.deepEqual(filterRows(rows, 'model 1 coding').map((r) => r.value), ['d:two']);
  assert.deepEqual(filterRows(rows, 'does not exist'), []);
  assert.equal(filterRows(rows, '  '), rows);
  const localized = flattenGroups(groupModels([model(2, { name: 'İSTANBUL' })], []));
  assert.equal(filterRows(localized, 'İSTANBUL'.toLocaleLowerCase()).length, 1);
});

const largeRows = () => flattenGroups(groupModels(Array.from({ length: 604 }, (_, i) => model(i)), []));

test('600+ rows mount only a bounded viewport, with correct spacers at both ends', () => {
  const rows = largeRows();
  const offsets = rowOffsets(rows, new Map());
  for (const top of [0, 1000, 10000, offsets.at(-1) - 400]) {
    const w = rowWindow(offsets, top, 400, true);
    assert.ok(w.end - w.start <= 21);
    assert.equal(w.before + offsets[w.end] - offsets[w.start] + w.after, offsets.at(-1));
  }
  assert.equal(rowWindow(offsets, 0, 400, true).start, 0);
  assert.equal(rowWindow(offsets, offsets.at(-1) - 400, 400, true).end, rows.length);
  assert.deepEqual(rowWindow([0], 0, 400, true), { start: 0, end: 0, before: 0, after: 0 });
  const small = rows.slice(0, WINDOW_THRESHOLD);
  assert.equal(rowWindow(rowOffsets(small, new Map()), 500, 100, false).end, WINDOW_THRESHOLD);
});

test('measured heights override estimates and navigation mounts targets throughout the library', () => {
  const rows = largeRows();
  const offsets = rowOffsets(rows, new Map([[rows[0].value, 72], [rows[1].value, 32]]));
  assert.deepEqual(offsets.slice(0, 4), [0, 72, 104, 156]);
  let active = -1, top = 0;
  for (const key of ['ArrowDown', 'End', 'Home', ...Array(80).fill('PageDown'), ...Array(80).fill('PageUp')]) {
    active = navigationIndex(key, active, offsets, 400);
    top = scrollToRow(offsets, active, top, 400);
    const w = rowWindow(offsets, top, 400, true);
    assert.ok(active >= w.start && active < w.end);
    assert.ok(offsets[active] >= top && offsets[active + 1] <= top + 400);
  }
  assert.equal(active, 0);
  for (let i = 0; i < rows.length; i++) active = navigationIndex('ArrowDown', active, offsets, 400);
  assert.equal(active, rows.length - 1);
  for (let i = 0; i < rows.length; i++) active = navigationIndex('ArrowUp', active, offsets, 400);
  assert.equal(active, 0);
  assert.equal(navigationIndex('Enter', 0, offsets, 400), null);
  assert.equal(navigationIndex('End', -1, [0], 400), null);
});

test('mixed file/definition heights and filtered positions use the visible result set', () => {
  const models = [model(0), model(1)];
  const rows = flattenGroups(groupModels(models, [def('a', models[0].path), def('b', models[0].path)]));
  assert.deepEqual(rowOffsets(rows, new Map()), [0, 48, 72, 100, 152]);
  const result = filterRows(rows, 'model 1');
  assert.equal(result.length, 1);
  assert.deepEqual(rowWindow(rowOffsets(result, new Map()), 0, 400, true), { start: 0, end: 1, before: 0, after: 0 });
});


test('batched measurements preserve the viewport anchor and cache offscreen heights', () => {
  const rows = largeRows();
  const heights = new Map();
  const offsets = rowOffsets(rows, heights);
  const measured = applyMeasurements(rows, offsets, heights, new Map([[rows[0].value, 72], [rows[1].value, 40], [rows[20].value, 80]]), 520);
  assert.equal(measured.top, 528); // +20 and -12 above the anchor; row 20 is below it.
  assert.equal(measured.heights.get(rows[20].value), 80);
  assert.equal(heights.size, 0);
  assert.equal(rowOffsets(rows, measured.heights)[10] - offsets[10], measured.top - 520);
});
