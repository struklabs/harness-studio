import { test } from 'node:test';
import assert from 'node:assert/strict';
import { suggestOfficial, suggestConnectors, tokens } from '../src/lib/components/tools/officialSuggest.ts';

const entry = (id, type, name, tags, en, tr = '') => ({ id, type, name, tags, description: { en, tr } });
const entries = [
  entry('io.example.pdf', 'tool', 'PDF Text Extractor', ['pdf', 'text'], 'Extract text from PDF files.'),
  entry('io.example.pdf-skill', 'skill', 'PDF Skill', ['pdf'], 'Guides a model through PDF work.'),
  entry('io.example.notion', 'mcp', 'Notion', ['notion', 'notes'], 'Search and edit Notion pages.'),
  entry('io.example.clock', 'tool', 'Clock', ['time'], 'Tells the time.'),
];

test('suggestions match the requested kind by name and tags first', () => {
  assert.deepEqual(suggestOfficial(entries, 'tool', 'I need a tool that extracts text from a PDF').map(e => e.id), ['io.example.pdf']);
  assert.deepEqual(suggestOfficial(entries, 'skill', 'pdf').map(e => e.id), ['io.example.pdf-skill']);
  assert.deepEqual(suggestOfficial(entries, 'mcp', 'connect to notion').map(e => e.id), ['io.example.notion']);
});

test('nothing is suggested for empty, filler-only or unrelated requests', () => {
  assert.deepEqual(suggestOfficial(entries, 'tool', ''), []);
  assert.deepEqual(suggestOfficial(entries, 'tool', 'create a tool'), []);
  assert.deepEqual(suggestOfficial(entries, 'tool', 'weather in Paris'), []);
  assert.deepEqual(tokens('The PDF, the PDF!'), ['pdf']);
});

test('connector requests point at the built-in connectors in English and Turkish', () => {
  assert.deepEqual(suggestConnectors('read my email inbox'), ['gmail']);
  assert.deepEqual(suggestConnectors('Excel tablosunu oku ve takvime ekle'), ['calendar', 'excel']);
  assert.deepEqual(suggestConnectors('summarize a web page'), []);
});
