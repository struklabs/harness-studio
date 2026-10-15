import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildTitleMessages, cleanGeneratedTitle, detectTitleLanguage, fallbackTitle, generateTitleWithRetry, resolveGeneratedTitle } from '../src/lib/components/playground/titles.ts';

const turkish = '1. tur: Bu sayının iki katı nedir: 7? Yalnızca sayıyı yaz.';
const english = 'What is twice this number: 7? Write only the number.';

test('title language detects Turkish and English independently of the UI', () => {
  for (const text of [turkish, 'Bana bunu bir daha anlat', 'Bu iki sayi nedir', 'İŞ', 'ğıöşü']) {
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

test('other conversation languages retain the same-language instruction and accept their titles', () => {
  for (const [message, title] of [
    ['日本語で並べ替えを説明してください', '並べ替えの方法'],
    ['Explica cómo ordenar una lista', 'Métodos de ordenación'],
    ['Erkläre die Sortierung einer Liste', 'Sortierung einer Liste'],
    ['Explique les méthodes de tri', 'Méthodes de tri'],
    ['Necesito ayuda sobre ordenamiento', 'Métodos de ordenación'],
  ]) {
    for (const ui of ['tr', 'en']) {
      assert.equal(detectTitleLanguage(message, ui), 'other');
      assert.match(buildTitleMessages(message, '', ui)[0].content, /Write the title in the same language as the user's message/);
      assert.deepEqual(resolveGeneratedTitle(title, message, ui), { title, source: 'auto' });
      assert.equal(resolveGeneratedTitle('The Sorting Methods', message, ui).source, 'auto');
    }
  }
  for (const text of ['oneword', 'two words', '12345', '```js\nconst a = 1;\n```', 'const count = items.length;']) {
    assert.equal(detectTitleLanguage(text, 'tr'), 'tr');
  }
});

test('shared Latin diacritics and dotted capital I do not imply Turkish', () => {
  for (const text of ['Tell me about İstanbul', 'Zürich', 'für Müller', 'ça', 'ç', 'ö', 'ü']) {
    assert.equal(detectTitleLanguage(text, 'en'), 'en', text);
  }
  for (const text of ['Bu çorba', 'Bu ürün', 'Bu örnek', 'ışık', 'dağ', 'Şehir']) {
    assert.equal(detectTitleLanguage(text, 'en'), 'tr', text);
  }
  for (const title of ['Zürich Travel Tips', 'About İstanbul', 'für Müller', 'ça']) {
    assert.deepEqual(resolveGeneratedTitle(title, english, 'tr'), { title, source: 'auto' });
  }
  for (const title of ['Bu çorba', 'Bu ürün', 'Bu örnek', 'ışık', 'dağ', 'Şehir']) {
    assert.equal(resolveGeneratedTitle(title, english, 'en').source, 'fallback');
  }
});

test('fallback truncation preserves graphemes and surrogate pairs with and without Segmenter', () => {
  const segmenter = Intl.Segmenter;
  try {
    for (const implementation of [segmenter, undefined]) {
      Intl.Segmenter = implementation;
      for (const suffix of ['😀more', '👩‍💻more', '🇹🇷more']) {
        const title = fallbackTitle('x'.repeat(59) + suffix);
        assert.equal(title, 'x'.repeat(59));
        assert.ok(title.isWellFormed());
      }
    }
  } finally {
    Intl.Segmenter = segmenter;
  }
  assert.equal(fallbackTitle('x'.repeat(59) + 'e\u0301more'), 'x'.repeat(59));
});

test('fallback cleanup preserves meaningful symbols and removes sentence endings and emoji', () => {
  for (const title of ['C++', 'f(x)', '$', 'Explain C++', 'Compute f(x)', 'Explain $']) {
    assert.equal(fallbackTitle(title), title);
  }
  for (const ending of ['.', ',', ';', ':', '!', '?', '…', '。', '．', '，', '；', '：', '！', '？', ' 😀', ' 👩‍💻', ' 🇹🇷', ' 1️⃣']) {
    assert.equal(fallbackTitle('Explain C++' + ending), 'Explain C++', ending);
  }
});

test('fallback clause selection keeps one-word colon labels and skips only substantive greetings', () => {
  assert.equal(fallbackTitle('Error: cannot find module x'), 'Error: cannot find module x');
  assert.equal(fallbackTitle('Error: cannot find module x in this project'), 'Error: cannot find module x in');
  assert.equal(fallbackTitle('Hello, world program'), 'Hello');
  assert.equal(fallbackTitle('Hello, world program needs fixing'), 'World program needs fixing');
  assert.equal(fallbackTitle('Hello, world program in C'), 'World program in C');
  assert.equal(fallbackTitle('Örn. Başlık x'), 'Örn. Başlık x');
  assert.equal(fallbackTitle('Use vs. code for this. Then more.'), 'Use vs. code for this');
  assert.equal(fallbackTitle('Hello, explain recursion'), 'Hello');
  assert.equal(fallbackTitle('Hello. can you explain recursion'), 'Hello');
  assert.equal(fallbackTitle('Hello! can you explain recursion'), 'Can you explain recursion');
});

test('Unicode abbreviation boundaries and English content words are validated conservatively', () => {
  assert.equal(cleanGeneratedTitle('Örnekler örn. Python'), 'Örnekler örn. Python');
  assert.equal(cleanGeneratedTitle('Karşılaştırma vb. örnekler'), 'Karşılaştırma vb. örnekler');
  for (const raw of ['Örnekler xörn. Python', 'Examples avs. Python', 'Examples 1etc. Python']) {
    assert.equal(cleanGeneratedTitle(raw), '');
  }
  for (const title of ['Sorting Methods', 'Travel Tips', 'The Guide', 'IS Python Fast']) {
    assert.equal(resolveGeneratedTitle(title, turkish, 'en').source, 'fallback', title);
  }
  for (const title of ['Python', 'Kubernetes CrashLoopBackOff', 'Python Sorting', "Python'da Sorting Methods", 'Sıralama Methods']) {
    assert.deepEqual(resolveGeneratedTitle(title, turkish, 'en'), { title, source: 'auto' });
  }
});

test('title requests retry a rejected system role with one user message', async () => {
  for (const failure of ['http', 'throw']) {
    const calls = [];
    const result = await generateTitleWithRetry(english, '14', 'tr', async messages => {
      calls.push(messages);
      if (calls.length === 1) {
        if (failure === 'throw') throw new Error('transport failure');
        return new Response('', { status: 400 });
      }
      return Response.json({ choices: [{ message: { content: 'Doubling Seven' } }] });
    });
    assert.deepEqual(result, { title: 'Doubling Seven', source: 'auto' });
    assert.deepEqual(calls[0], buildTitleMessages(english, '14', 'tr'));
    assert.deepEqual(calls[1], [{ role: 'user', content: `${calls[0][0].content}\n\n${calls[0][1].content}` }]);
  }
});

test('failed title requests always resolve a fallback after at most two attempts', async () => {
  for (const failure of ['http', 'throw', 'abort', 'json']) {
    let calls = 0;
    const result = await generateTitleWithRetry(turkish, '14', 'en', async () => {
      calls++;
      if (failure === 'throw') throw new Error('offline');
      if (failure === 'abort') throw new DOMException('aborted', 'AbortError');
      if (failure === 'json') return new Response('invalid json');
      return new Response('', { status: 500 });
    });
    assert.equal(calls, 2);
    assert.deepEqual(result, { title: fallbackTitle(turkish), source: 'fallback' });
  }
  let calls = 0;
  await generateTitleWithRetry(english, '14', 'en', async () => {
    calls++;
    return Response.json({ choices: [{ message: { content: 'Doubling Seven' } }] });
  });
  assert.equal(calls, 1);
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
  for (const raw of ['Two times seven', 'Twice seven doubled', 'Number doubling result']) {
    assert.equal(resolveGeneratedTitle(raw, turkish, 'en').source, 'fallback', raw);
  }
  assert.deepEqual(resolveGeneratedTitle('Kubernetes CrashLoopBackOff', turkish, 'en'), { title: 'Kubernetes CrashLoopBackOff', source: 'auto' });
  assert.deepEqual(resolveGeneratedTitle('İki katı 14', turkish, 'en'), { title: 'İki katı 14', source: 'auto' });
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
