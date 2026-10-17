import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/lib/components/hub/hubData.ts', import.meta.url), 'utf8');
const calls = [];
const stored = new Map();
globalThis.__hubSortInvoke = async (command, args) => {
  calls.push({ command, options: structuredClone(args.options) });
  return { items: [{ id: `${args.options.sort}/${args.options.cursor ?? 'first'}` }], next_cursor: 'next' };
};
globalThis.localStorage = { getItem: (key) => stored.get(key) ?? null, setItem: (key, value) => { stored.set(key, value); } };
const js = ts.transpileModule(source.replace("import { invoke } from '#lib/ipc.js';", 'const invoke = globalThis.__hubSortInvoke;'), {
  compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
}).outputText;
const { DEFAULT_PREFS, loadPrefs, savePrefs, fetchPage, clearPageCache } = await import(`data:text/javascript;base64,${Buffer.from(js).toString('base64')}`);

test('Hub defaults to all HF formats and preserves an explicitly saved GGUF filter', () => {
  stored.clear();
  assert.equal(DEFAULT_PREFS.includeNonGguf, true);
  assert.equal(loadPrefs().includeNonGguf, true);
  savePrefs({ ...DEFAULT_PREFS, includeNonGguf: false });
  assert.equal(loadPrefs().includeNonGguf, false);
  savePrefs(DEFAULT_PREFS);
  assert.equal(loadPrefs().includeNonGguf, true);
});

test('v1 curated format preference migrates while query, task and sort are retained', () => {
  stored.clear();
  stored.set('harness_hub_prefs_v1', JSON.stringify({ ...DEFAULT_PREFS, query: 'sd-turbo', task: 'text-to-image', sort: 'likes', includeNonGguf: false }));
  assert.deepEqual(loadPrefs(), { ...DEFAULT_PREFS, query: 'sd-turbo', task: 'text-to-image', sort: 'likes', includeNonGguf: true });
  savePrefs({ ...loadPrefs(), includeNonGguf: false });
  assert.equal(loadPrefs().includeNonGguf, false);
});

test('sorts and pagination forward identical filters and cache separate HF pages in order', async () => {
  clearPageCache();
  calls.length = 0;
  const filters = { query: 'sd-turbo', pipeline_tag: 'text-to-image', license: 'mit', author: 'stabilityai', include_non_gguf: true, limit: 30 };
  for (const sort of ['downloads', 'likes', 'trendingScore', 'lastModified', 'createdAt']) {
    const options = { ...filters, sort };
    const first = await fetchPage(options);
    const second = await fetchPage({ ...options, cursor: first.next_cursor });
    assert.deepEqual(first.items, [{ id: `${sort}/first` }]);
    assert.deepEqual(second.items, [{ id: `${sort}/next` }]);
    assert.equal(await fetchPage(options), first);
    assert.deepEqual(calls.at(-2), { command: 'search_hf_models_page', options });
    assert.deepEqual(calls.at(-1), { command: 'search_hf_models_page', options: { ...options, cursor: 'next' } });
  }
  assert.equal(calls.length, 10);
  await fetchPage({ ...filters, sort: 'downloads', include_non_gguf: false });
  assert.equal(calls.length, 11);
});
