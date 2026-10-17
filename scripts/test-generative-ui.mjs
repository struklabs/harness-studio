import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseChart, parseUi, parseJson, structuredResult, toCsv, sortRows } from '../src/lib/components/playground/generativeUi.ts';

const chart = { type: 'bar', labels: ['A', 'B'], series: [{ name: 'Count', values: [2, -3] }] };
test('chart types, finite aligned data, bounds, and pie constraints', () => {
  for (const type of ['bar', 'line']) assert.equal(parseChart(JSON.stringify({ ...chart, type })).type, type);
  for (const invalid of [{ ...chart, type: 'html' }, { ...chart, labels: [] }, { ...chart, onclick: 'run()' }, { ...chart, labels: ['A'] }, { ...chart, type: 'pie' }]) assert.equal(parseChart(JSON.stringify(invalid)), null);
  assert.equal(parseChart('{"type":"line","labels":["A"],"series":[{"name":"X","values":[1e999]}]}'), null);
  assert.ok(parseChart(JSON.stringify({ ...chart, type: 'pie', series: [{ name: 'Count', values: [0, 2] }] })));
  assert.equal(parseChart(JSON.stringify({ ...chart, labels: Array(41).fill('A') })), null);
  assert.equal(parseChart(JSON.stringify({ ...chart, type: { toString: null } })), null);
});
test('whitelist all UI blocks and keep model markup inert', () => {
  const title = '<img src=x onerror=alert(1)>';
  for (const block of [
    { type: 'card', title, text: '<script>alert(1)</script>' },
    { type: 'callout', title, text: 'Notice' },
    { type: 'list', title, items: ['One'] }, { type: 'steps', title, items: [] },
    { type: 'progress', title, value: 0 }, { type: 'progress', title, value: 100 },
    { type: 'key-value', title, entries: { Name: 'Text' } },
  ]) assert.deepEqual(parseUi(JSON.stringify(block)), block);
  for (const block of [
    { type: 'html', title }, { type: 'progress', title, value: 101 },
    { type: 'progress', title, value: -1 }, { type: 'list', title, items: [3] },
    { type: 'card', title, text: 'OK', style: 'color:red' },
    { type: 'card', title, text: 'OK', onclick: 'alert(1)' },
    { type: 'key-value', title, entries: { Nested: {} } },
  ]) assert.equal(parseUi(JSON.stringify(block)), null);
});
test('reject malformed, oversized, prototype keys and incomplete output', () => {
  for (const source of ['{', 'undefined', 'x'.repeat(200001), '{"__proto__":{}}', '{"nested":{"constructor":"bad"}}']) assert.equal(parseJson(source), undefined);
  assert.equal(parseUi('{"type":"card"}'), null);
  assert.equal(parseUi(JSON.stringify({ type: 'list', title: 'X', items: Array(101).fill('X') })), null);
});
test('structured objects and heterogeneous object arrays preserve original values', () => {
  assert.deepEqual(structuredResult('[{"a":1},{"b":{"x":2}}]'), [{ a: 1 }, { b: { x: 2 } }]);
  for (const source of ['null', '42', '[1,2]', '[{"a":1},null]', JSON.stringify(Array(501).fill({ a: 1 }))]) assert.equal(structuredResult(source), null);
  assert.deepEqual(structuredResult('{}'), {});
  assert.deepEqual(structuredResult('[]'), []);
});
test('CSV quotes, line breaks and spreadsheet formula protection; stable numeric sorting', () => {
  assert.equal(toCsv([['a,b', 'a"b', 'line\nbreak', '=1+1', '\t@SUM(1)']]), '"a,b","a""b","line\nbreak","\'=1+1","\'\t@SUM(1)"');
  const rows = [['10'], ['2'], ['2']];
  assert.deepEqual(sortRows(rows, 0, 1), [['2'], ['2'], ['10']]);
  assert.deepEqual(rows, [['10'], ['2'], ['2']]);
});
