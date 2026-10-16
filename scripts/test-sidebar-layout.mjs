// Left navigation space budget (src/lib/stores/sidebarLayout.js).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { sidebarLayout } from '../src/lib/stores/sidebarLayout.js';

const NAV = { mainWidth: 216, mainRail: 48, mainCollapsed: false, sectionWidth: 224, sectionRail: 48 };

test('a wide window docks both columns', () => {
  assert.deepEqual(sidebarLayout({ viewport: 1536, ...NAV }), { navWidth: 216, forcedMainRail: false, sectionDrawer: false });
  assert.equal(sidebarLayout({ viewport: 1280, ...NAV }).sectionDrawer, false); // 150% on 1080p
});

test('200% on a 1080p screen turns the section column into a drawer', () => {
  const l = sidebarLayout({ viewport: 960, ...NAV });
  assert.equal(l.sectionDrawer, true);
  assert.equal(l.forcedMainRail, false);
  assert.equal(l.navWidth, 216);
});

test('a very narrow window also rails the main navigation', () => {
  const l = sidebarLayout({ viewport: 700, ...NAV });
  assert.deepEqual(l, { navWidth: 48, forcedMainRail: true, sectionDrawer: true });
});

test('pages without a section column never get a drawer', () => {
  assert.equal(sidebarLayout({ viewport: 700, ...NAV, sectionWidth: 0, sectionRail: 0 }).sectionDrawer, false);
});

const component = (name) => readFileSync(new URL(`../src/lib/components/${name}.svelte`, import.meta.url), 'utf8');

test('section navigation keeps the drawer open, with an explicit accessible collapse toggle', () => {
  const sidebar = component('common/SectionSidebar');
  assert.match(sidebar, /onclick=\{\(\) => onSelect\(s\.id\)\}/);
  assert.doesNotMatch(sidebar, /pane\.close\(/);
  assert.match(sidebar, /<button\s+type="button"\s+aria-current=\{active === s\.id \? 'page' : undefined\}/);
  assert.match(sidebar, /aria-controls=\{pane\.elementId\}\s+aria-expanded=\{!railed\}/);
  assert.match(sidebar, /onclick=\{\(\) => pane\.toggle\(\)\}/);
});

test('page-specific section selection does not close the shared drawer either', () => {
  for (const name of ['Settings', 'Connections', 'Create']) {
    const source = component(name);
    assert.match(source, /<SectionSidebar\b[^>]*onSelect=\{select\}/, name);
    assert.doesNotMatch(source, /(?:navPane|panes\.createNav)\.close\(/, name);
  }
});
