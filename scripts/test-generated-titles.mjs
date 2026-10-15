import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildTitleMessages, cleanGeneratedTitle, detectTitleLanguage, fallbackTitle, resolveGeneratedTitle } from '../src/lib/components/playground/titles.ts';

const turkish = '1. tur: Bu sayının iki katı nedir: 7? Yalnızca sayıyı yaz.';
const english = 'What is twice this number: 7? Write only the number.';

test('title language detects Turkish and English independently of the UI', () => {
  for (const text of [turkish, 'Bana bunu bir daha anlat', 'Bu iki sayi nedir', 'İŞ', 'ç', 'ğıöşü']) {
    assert.equal(detectTitleLanguage(text, 'en-US'), 'tr', text);
  }
  for (const text of [english, 'The history of computing', 'How do I sort a list in Python', 'THIS IS MY QUESTION']) {
    assert.equal(detectTitleLanguage(text, 'tr-TR'), 'en', text);
  }
  assert.equal(detectTitleLanguage('Suggest three weekend activities in London.', 'tr-TR'), 'en');
  assert.equal(detectTitleLanguage("Python'da listeyi sirala", 'en-US'), 'tr');
  assert.equal(detectTitleLanguage("What's London's best museum?", 'tr-TR'), 'en');
  assert.equal(detectTitleLanguage('```js\nconst a = "ş";\n``` What is this?', 'tr'), 'en');
});

test('short or ambiguous text inherits the UI language without inventing validation evidence', () => {
  for (const text of ['', '7', 'Hi', 'Merhaba', 'Rust', 'test', 'bir', 'the', 'bir the']) {
    assert.equal(detectTitleLanguage(text, 'tr-TR'), 'tr', text);
    assert.equal(detectTitleLanguage(text, 'en-US'), 'en', text);
    assert.equal(detectTitleLanguage(text, ''), '', text);
  }
  assert.equal(detectTitleLanguage('123', 'de-DE'), 'de');
});

test('title requests explicitly name the language in a separate system instruction', () => {
  const tr = buildTitleMessages(turkish, '14', 'en');
  assert.equal(tr[0].role, 'system');
  assert.match(tr[0].content, /Türkçe/);
  assert.match(tr[0].content, /Yalnızca başlığı yaz/);
  assert.deepEqual(tr[1], { role: 'user', content: `User: ${turkish}\n\nAssistant: 14` });
  assert.match(buildTitleMessages(english, '14', 'tr')[0].content, /Write the title in English/);
  assert.match(buildTitleMessages('7', '14', 'tr-TR')[0].content, /Türkçe/);
  assert.match(buildTitleMessages('7', '14', 'de-DE')[0].content, /Write the title in German/);
  assert.equal(buildTitleMessages('x'.repeat(3000), 'y'.repeat(2000), 'en')[1].content,
    `User: ${'x'.repeat(2000)}\n\nAssistant: ${'y'.repeat(1000)}`);
});

test('cleaning accepts titles and rejects answer bodies', () => {
  assert.equal(cleanGeneratedTitle('<think>reasoning</think>Başlık: **İki katını bulma**'), 'İki katını bulma');
  assert.equal(cleanGeneratedTitle('Title: "Doubling seven"'), 'Doubling seven');
  assert.equal(cleanGeneratedTitle('TCP vs. UDP Explained'), 'TCP vs. UDP Explained');
  for (const raw of ['', '<think>unfinished', 'First line\nSecond line', 'An answer. More text', '14.', '12:30', 'one two three four five six seven', 'x'.repeat(61), '14', '3 + 4 = 7']) {
    assert.equal(cleanGeneratedTitle(raw), '', raw);
  }
});

test('wrong-language and invalid replies use the first user message with fallback provenance', () => {
  assert.deepEqual(resolveGeneratedTitle('Two times seven is fourteen', turkish, 'en'), {
    title: 'Bu sayının iki katı nedir', source: 'fallback',
  });
  assert.deepEqual(resolveGeneratedTitle('Bu sayının iki katı', english, 'tr'), {
    title: 'What is twice this number', source: 'fallback',
  });
  for (const raw of ['', '14.', 'Answer\nExplanation', '<think>unfinished']) {
    assert.deepEqual(resolveGeneratedTitle(raw, turkish, 'tr'), { title: fallbackTitle(turkish), source: 'fallback' });
  }
  assert.deepEqual(resolveGeneratedTitle('İki katını bulma', turkish, 'en'), { title: 'İki katını bulma', source: 'auto' });
  assert.deepEqual(resolveGeneratedTitle('Two times seven', english, 'tr'), { title: 'Two times seven', source: 'auto' });
  assert.deepEqual(resolveGeneratedTitle('Rust', turkish, 'tr'), { title: 'Rust', source: 'auto' });
});

test('fallback titles strip formatting, code, links, URLs and meaningful numbered prefixes', () => {
  const samples = [
    [turkish, 'Bu sayının iki katı nedir'],
    [english, 'What is twice this number'],
    ['1. turn: Explain this error. Then fix it.', 'Explain this error'],
    ['2) **Bu hatayı** açıkla; sonra düzelt.', 'Bu hatayı açıkla'],
    ['# Explain [this issue](https://example.org/a) https://example.org/b\n```js\nthrow new Error();\n``` please.', 'Explain this issue please'],
    ['~~~python\nprint("answer")\n~~~\nBu kodu açıkla: ayrıntıları anlat.', 'Bu kodu açıkla'],
    ['Explain `private_code()` and ![image](https://example.org/image.png) this error!', 'Explain and this error'],
    ['  First   clause — another clause.', 'First clause'],
    ['Explain this error, then fix it.', 'Explain this error'],
    ['1. 7', '1. 7'],
    ['1. Hello', 'Hello'],
    ['Merhaba, kısa bir şiir yazar mısın?', 'Kısa bir şiir yazar mısın'],
    ['Hi! can you explain recursion?', 'Can you explain recursion'],
    ['Merhaba', 'Merhaba'],
    ['Python, Rust or Go?', 'Python'],
    ['```js\nonlyCode();\n``` https://example.org', ''],
  ];
  for (const [input, expected] of samples) assert.equal(fallbackTitle(input), expected, input);
});

test('fallback titles stay within six words and sixty characters without trailing punctuation', () => {
  for (const input of [
    'one two three four five six seven eight nine',
    'Bu bir çok uzun konuşma mesajı ve devam ediyor '.repeat(1000),
    'x'.repeat(2000),
    'x'.repeat(59) + ', and more words',
  ]) {
    const title = fallbackTitle(input);
    assert.ok(title.length <= 60);
    assert.ok(title.split(/\s+/).length <= 6);
    assert.doesNotMatch(title, /[\p{P}\s]$/u);
  }
});
