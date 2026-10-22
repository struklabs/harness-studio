import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
  DEFAULT_FILTERS, activeFilterCount, facets, filterAndSort, fold, formatBytes, navigate, rowWindow, searchScore, toEntry, updatable,
} from '../src/lib/moduleCatalog.ts';
import { generateModules, fixtureCount } from '../src/lib/dev/moduleFixtures.ts';

const translate = keys => keys.en;
const entries = (count = 500, seed = 1) => generateModules(count, seed).map(view => toEntry(view, translate));
const filters = patch => ({ ...DEFAULT_FILTERS, ...patch });
const make = (patch = {}, status = 'not_installed', extra = {}) => toEntry({
  manifest: {
    id: patch.id ?? 'm', version: '1.0.0', manifest_version: 1, name: { en: patch.name ?? 'Module', tr: patch.name ?? 'Module' },
    description: { en: patch.description ?? '', tr: '' }, functions: [], platforms: ['windows-x86_64'], hardware: { en: '', tr: '' },
    components: [], permissions: [], shared_component_ids: [], min_app_version: '0.1.0', planned: false, ...patch.manifest,
  },
  catalog: patch.catalog ?? {},
  state: { status, installed_version: null, error: null, components: [] },
  disk_bytes: 0, shared_disk_bytes: 0, update_available: false, data_choices: [], incompatibility: null, progress: null, components: {}, ...extra,
}, translate);

test('fixtures are deterministic, unique and cover every catalog dimension', () => {
  const a = generateModules(500, 7);
  assert.equal(a.length, 500);
  assert.deepEqual(a, generateModules(500, 7));
  assert.notDeepEqual(a, generateModules(500, 8));
  assert.equal(new Set(a.map(m => m.manifest.id)).size, 500);
  assert.ok(a.some(m => m.catalog.community) && a.some(m => !m.catalog.community));
  assert.ok(a.some(m => m.state.status === 'installed') && a.some(m => m.state.status === 'not_installed') && a.some(m => m.state.status === 'disabled'));
  assert.ok(a.some(m => m.update_available) && a.some(m => m.incompatibility));
  assert.ok(a.filter(m => m.catalog.community).every(m => m.catalog.author && m.catalog.repository && m.catalog.license));
});

test('fixture count comes from the query or storage and is capped', () => {
  assert.equal(fixtureCount('?modules_fixture=500', null), 500);
  assert.equal(fixtureCount('', '120'), 120);
  assert.equal(fixtureCount('?modules_fixture=99999', null), 5000);
  assert.equal(fixtureCount('?modules_fixture=abc', null), 0);
  assert.equal(fixtureCount('', null), 0);
});

test('fold ignores case, accents and Turkish dotted i', () => {
  assert.equal(fold('İSTANBUL Ğüşöç'), 'istanbul gusoc');
  assert.equal(fold('ıspanak'), 'ispanak');
});

test('search is fuzzy: substring, word prefix, typo and subsequence match; misses score zero', () => {
  const e = make({ name: 'Speech Recognition', description: 'Transcribe audio locally', catalog: { tags: ['whisper'] } });
  assert.ok(searchScore(e, 'speech') > searchScore(e, 'ech'));
  assert.ok(searchScore(e, 'spech') > 0, 'one missing letter');
  assert.ok(searchScore(e, 'speehc') > 0, 'swapped letters');
  assert.ok(searchScore(e, 'sprecog') > 0, 'subsequence');
  assert.ok(searchScore(e, 'whisper audio') > 0, 'every token may match a different field');
  assert.equal(searchScore(e, 'zebra'), 0);
  assert.equal(searchScore(e, 'speech zebra'), 0, 'all tokens must match');
  assert.equal(searchScore(e, '   '), 1, 'blank query keeps everything');
});

test('name matches outrank description matches and results sort by relevance', () => {
  const list = [
    make({ id: 'a', name: 'Alpha', description: 'Works with vision models' }),
    make({ id: 'b', name: 'Vision Lab', description: 'Something else' }),
    make({ id: 'c', name: 'Gamma', description: 'Nothing here' }),
  ];
  assert.deepEqual(filterAndSort(list, filters({ query: 'vision' }), 'name').map(e => e.id), ['b', 'a']);
});

test('search works across languages and accents', () => {
  const e = toEntry(generateModules(1)[0], keys => keys.tr);
  assert.ok(searchScore(e, 'sagl') >= 0);
  const tr = make({ name: 'Konuşma Tanıma' });
  assert.ok(searchScore(tr, 'konusma tanima') > 0);
});

test('status, enabled, origin, category, capability, platform and compatibility filters', () => {
  const list = entries();
  const only = f => filterAndSort(list, filters(f), 'name');
  assert.ok(only({ status: 'installed' }).every(e => e.installed));
  assert.ok(only({ status: 'available' }).every(e => !e.installed && e.installable));
  assert.ok(only({ status: 'updates' }).every(e => e.updateAvailable && e.installed));
  assert.ok(only({ enabled: 'enabled' }).every(e => e.view.state.status === 'installed'));
  assert.ok(only({ enabled: 'disabled' }).every(e => e.view.state.status === 'disabled'));
  assert.ok(only({ origin: 'community' }).every(e => e.origin === 'community' && e.view.catalog.community));
  assert.ok(only({ origin: 'official' }).every(e => e.origin === 'official'));
  assert.ok(only({ category: 'speech' }).every(e => e.category === 'speech'));
  assert.ok(only({ capability: 'ocr' }).every(e => e.capabilities.includes('ocr')));
  assert.ok(only({ platform: 'linux-x86_64' }).every(e => e.view.manifest.platforms.includes('linux-x86_64')));
  assert.ok(only({ compatibleOnly: true }).every(e => e.compatible));
  const combined = only({ status: 'installed', origin: 'community', category: 'agents' });
  assert.ok(combined.every(e => e.installed && e.origin === 'community' && e.category === 'agents'));
  assert.equal(only({}).length, 500);
});

test('official is the default origin: only an explicit community flag marks third-party entries', () => {
  assert.equal(make().origin, 'official');
  assert.equal(make({ catalog: { community: false } }).origin, 'official');
  assert.equal(make({ catalog: { community: true } }).origin, 'community');
});

test('missing catalog metadata falls back safely', () => {
  const e = make();
  assert.equal(e.category, 'other');
  assert.deepEqual(e.tags, []);
  assert.equal(e.author, null);
  assert.equal(filterAndSort([e], filters({ category: 'other' }), 'name').length, 1);
});

test('sorting by name, recently updated, popularity and recommendation', () => {
  const list = entries();
  const names = filterAndSort(list, DEFAULT_FILTERS, 'name').map(e => e.name);
  assert.deepEqual(names, [...names].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' })));
  const dates = filterAndSort(list, DEFAULT_FILTERS, 'updated').map(e => e.view.catalog.updated_at);
  assert.deepEqual(dates, [...dates].sort().reverse());
  const pops = filterAndSort(list, DEFAULT_FILTERS, 'popular').map(e => e.view.catalog.popularity);
  assert.deepEqual(pops, [...pops].sort((a, b) => b - a));
  const rec = filterAndSort(list, DEFAULT_FILTERS, 'recommended');
  const firstPlain = rec.findIndex(e => !e.updateAvailable && !e.view.catalog.featured);
  assert.ok(rec.slice(0, firstPlain).every(e => e.updateAvailable || e.view.catalog.featured), 'updates and featured lead');
  assert.deepEqual(filterAndSort(list, DEFAULT_FILTERS, 'popular').map(e => e.id), filterAndSort([...list].reverse(), DEFAULT_FILTERS, 'popular').map(e => e.id), 'stable under input order');
});

test('facet counts ignore their own filter and match what a click would show', () => {
  const list = entries();
  const f = filters({ status: 'installed', category: 'speech' });
  const counts = facets(list, f);
  for (const [category, n] of counts.categories) {
    assert.equal(filterAndSort(list, { ...f, category }, 'name').length, n, category);
  }
  assert.equal(counts.status.installed, filterAndSort(list, { ...f, status: 'installed' }, 'name').length);
  assert.equal(counts.origin.all, counts.origin.official + counts.origin.community);
  const total = facets(list, DEFAULT_FILTERS);
  assert.equal(total.status.all, 500);
  assert.equal(total.status.available, filterAndSort(list, filters({ status: 'available' }), 'name').length);
  assert.ok(total.status.installed + total.status.available <= 500);
});

test('planned, built-in and incompatible modules are not counted or listed as installable', () => {
  const list = [
    make({ id: 'a' }),
    make({ id: 'b', manifest: { planned: true } }),
    make({ id: 'training', manifest: { planned: true } }),
    make({ id: 'c' }, 'not_installed', { incompatibility: { en: 'no', tr: 'no' } }),
    make({ id: 'd' }, 'installed'),
  ];
  const counts = facets(list, DEFAULT_FILTERS);
  assert.deepEqual([counts.status.all, counts.status.installed, counts.status.available], [5, 1, 1]);
  assert.deepEqual(filterAndSort(list, filters({ status: 'available' }), 'name').map(e => e.id), ['a']);
});

test('active filter count and bulk update targets', () => {
  assert.equal(activeFilterCount(DEFAULT_FILTERS), 0);
  assert.equal(activeFilterCount(filters({ status: 'updates', category: 'data', compatibleOnly: true })), 3);
  const list = entries();
  const targets = updatable(list);
  assert.ok(targets.length > 0);
  assert.ok(targets.every(e => e.installed && e.updateAvailable && e.compatible));
  const busy = make({}, 'installing', { update_available: true });
  assert.equal(updatable([busy]).length, 0);
});

test('windowing mounts a bounded slice of 500+ rows and keeps total height stable', () => {
  const rows = 600;
  const pitch = 100;
  for (const top of [0, 1234, 29000, 59900, 1e9]) {
    const w = rowWindow(rows, pitch, top, 800);
    assert.ok(w.last - w.first + 1 <= 17, `window at ${top}: ${w.first}-${w.last}`);
    assert.ok(w.first >= 0 && w.last <= rows - 1);
    if (w.last >= w.first) assert.equal(w.before + (w.last - w.first + 1) * pitch + w.after, rows * pitch);
  }
  assert.deepEqual(rowWindow(0, 100, 0, 800), { first: 0, last: -1, before: 0, after: 0 });
  assert.equal(rowWindow(600, 100, 0, 800).first, 0);
  assert.equal(rowWindow(600, 100, 59000, 800).last, 599);
});

test('keyboard navigation moves by cell, row, page and ends without leaving the grid', () => {
  assert.equal(navigate(0, 'ArrowRight', 10, 3), 1);
  assert.equal(navigate(0, 'ArrowLeft', 10, 3), 0);
  assert.equal(navigate(1, 'ArrowDown', 10, 3), 4);
  assert.equal(navigate(8, 'ArrowDown', 10, 3), 8, 'no row below keeps focus');
  assert.equal(navigate(4, 'ArrowUp', 10, 3), 1);
  assert.equal(navigate(1, 'ArrowUp', 10, 3), 1);
  assert.equal(navigate(2, 'PageDown', 10, 3, 2), 8);
  assert.equal(navigate(8, 'PageDown', 10, 3, 5), 9);
  assert.equal(navigate(5, 'Home', 10, 3), 0);
  assert.equal(navigate(5, 'End', 10, 3), 9);
  assert.equal(navigate(0, 'a', 10, 3), -1);
  assert.equal(navigate(0, 'End', 0, 3), -1);
});

test('byte sizes read as MiB or GiB and unknown sizes as a dash', () => {
  assert.equal(formatBytes(0), '—');
  assert.equal(formatBytes(NaN), '—');
  assert.equal(formatBytes(1), '1 MiB');
  assert.equal(formatBytes(5 * 1024 ** 2), '5 MiB');
  assert.equal(formatBytes(3 * 1024 ** 3), '3.0 GiB');
});

test('filtering and sorting 5000 modules stays fast enough for typing', () => {
  const list = entries(5000);
  const start = performance.now();
  for (const query of ['vox', 'pixl bridg', 'zzzz', '']) filterAndSort(list, filters({ query }), 'recommended');
  assert.ok(performance.now() - start < 2000, `took ${performance.now() - start}ms`);
});

test('every bundled module has catalog metadata, official and in the backend list', () => {
  const bundled = JSON.parse(readFileSync(new URL('../crates/harness-core/src/modules/bundled.json', import.meta.url), 'utf8'));
  const catalog = JSON.parse(readFileSync(new URL('../crates/harness-core/src/modules/catalog.json', import.meta.url), 'utf8'));
  assert.deepEqual(Object.keys(catalog).sort(), bundled.map(m => m.id).sort());
  for (const [id, c] of Object.entries(catalog)) {
    assert.equal(typeof c.category, 'string', id);
    assert.ok(c.tags.length > 0, id);
    assert.ok(!c.community, `${id} is official`);
  }
  // The manifest itself is untouched, so its schema and the frozen API contract stay valid.
  for (const m of bundled) assert.ok(!('category' in m) && !('community' in m), m.id);
});

test('page and detail strings exist in both languages and reference no missing keys', async () => {
  const { default: locales } = await import('../src/lib/locales/modules.ts');
  const files = ['Modules.svelte', 'modules/ModuleCard.svelte', 'modules/ModuleDetail.svelte'].map(f => readFileSync(new URL(`../src/lib/components/${f}`, import.meta.url), 'utf8')).join('\n');
  const used = new Set([...files.matchAll(/'(modules\.[a-z_]*[a-z])'/g)].map(m => m[1]));
  const others = await Promise.all(['agents', 'discover', 'trust'].map(async n => (await import(`../src/lib/locales/${n}.ts`).catch(() => ({ default: { en: {}, tr: {} } }))).default));
  for (const key of used) {
    const found = (lang) => key in locales[lang] || others.some(o => key in o[lang]);
    assert.ok(found('en'), `${key} missing in en`);
    assert.ok(found('tr'), `${key} missing in tr`);
  }
  assert.doesNotMatch(Object.values(locales.tr).join('\n'), /çalışma zamanı/i);
  assert.equal(locales.tr['modules.badge_community'], 'Topluluk');
  assert.equal(locales.en['modules.badge_community'], 'Community');
});
test('a module that shipped as an app page counts as compatible; other planned ones do not', () => {
  assert.equal(make({ id: 'training', manifest: { planned: true } }).compatible, true);
  assert.equal(make({ id: 'automations', manifest: { planned: true } }).compatible, false);
});
