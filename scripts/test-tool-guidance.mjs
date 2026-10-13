import assert from 'node:assert/strict';
import { test } from 'node:test';
import { compactToolDefinitions, firstSentence, withToolGuidance } from '../src/lib/components/playground/toolGuidance.ts';

const search = [{ function: { name: 'web_search' } }, { function: { name: 'weather' } }];

test('no offered tools leaves messages unchanged', () => {
  const messages = [{ role: 'system', content: 'User instructions' }, { role: 'user', content: 'Hi' }];
  assert.equal(withToolGuidance(messages, []), messages);
});

test('merges guidance with user system prompt without mutating history', () => {
  const messages = [{ role: 'system', content: 'Answer in Turkish', extra: true }, { role: 'user', content: 'mert güzeldağ kim?' }];
  const result = withToolGuidance(messages, search);
  assert.ok(result[0].content.startsWith('Answer in Turkish\n\n'));
  assert.match(result[0].content, /people, organizations, events/);
  assert.match(result[0].content, /only through the tool_calls format/);
  assert.match(result[0].content, /always call web_search first before answering/);
  assert.match(result[0].content, /exactly, letter by letter; never drop or change letters/);
  assert.match(result[0].content, /“kim\?” veya “kimdir\?”/);
  assert.match(result[0].content, /yazıldığı gibi web_search/);
  assert.match(result[0].content, /do not combine profiles into one biography/);
  assert.match(result[0].content, /list the separate possibilities with source URLs/);
  assert.match(result[0].content, /do not guess why a page was marked for deletion/);
  assert.equal(result[0].extra, true);
  assert.equal(result[1], messages[1]);
  assert.equal(messages[0].content, 'Answer in Turkish');
});

test('adds system guidance with empty or absent system prompt and preserves tool rounds', () => {
  const round = [{ role: 'user', content: 'Who?' }, { role: 'assistant', tool_calls: [] }, { role: 'tool', content: 'Result', tool_call_id: '1' }];
  const result = withToolGuidance(round, search);
  assert.equal(result[0].role, 'system');
  assert.deepEqual(result.slice(1), round);
  assert.match(result[0].content, /Never invent facts/);
  assert.equal(withToolGuidance([{ role: 'system', content: '' }, ...round], search)[0].content, result[0].content);
});

test('does not request web_search when it was deselected', () => {
  const result = withToolGuidance([], [{ function: { name: 'weather' } }]);
  assert.match(result[0].content, /Use the available tools/);
  assert.doesNotMatch(result[0].content, /web_search/);
  assert.match(result[0].content, /weather kullan/);
  assert.match(result[0].content, /olmayan tahmin günleri ekleme/);
  assert.match(result[0].content, /apparent_temperature_c hissedilen/);
});

test('does not request weather when it was deselected', () => {
  const result = withToolGuidance([], [{ function: { name: 'web_search' } }]);
  assert.doesNotMatch(result[0].content, /weather/);
});

test('first sentence keeps abbreviations and text without a full stop', () => {
  assert.equal(firstSentence('Recursively search for files. Skips .gitignore entries.'), 'Recursively search for files.');
  assert.equal(firstSentence('Glob pattern (e.g. "*.cpp"). Default: **'), 'Glob pattern (e.g. "*.cpp").');
  assert.equal(firstSentence('Returns runtime info: the OS name/version'), 'Returns runtime info: the OS name/version');
});

test('compact tool definitions shorten descriptions only', () => {
  const tool = {
    type: 'function',
    function: {
      name: 'grep_search',
      description: 'Search for a pattern in files under a path. Returns matching lines with file paths.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'File or directory to search in' },
          edits: { type: 'array', items: { type: 'object', properties: { old_text: { type: 'string', description: 'Exact text. Must be unique.' } } } },
        },
        required: ['path'],
      },
    },
  };
  const [c] = compactToolDefinitions([tool]);
  assert.equal(c.function.name, 'grep_search');
  assert.equal(c.function.description, 'Search for a pattern in files under a path.');
  assert.deepEqual(c.function.parameters.required, ['path']);
  assert.equal(c.function.parameters.properties.path.description, 'File or directory to search in');
  assert.equal(c.function.parameters.properties.edits.items.properties.old_text.description, 'Exact text.');
  assert.equal(tool.function.description.endsWith('file paths.'), true, 'input not mutated');
});
