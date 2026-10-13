import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildRequest, highlight, isFormTask, parseResponse, splitLabels } from '../src/lib/components/playground/pipeline.ts';

const form = { text: ' Wolfgang lives in Berlin. ', labels: 'sports, , politics ,cooking', multiLabel: true, question: 'Where?', context: 'Berlin.' };

test('form tasks and label splitting', () => {
  assert.ok(isFormTask('fill-mask'));
  assert.ok(!isFormTask('image-classification'));
  assert.ok(!isFormTask(undefined));
  assert.deepEqual(splitLabels(form.labels), ['sports', 'politics', 'cooking']);
});

test('request bodies follow the Inference API', () => {
  assert.deepEqual(buildRequest('text-classification', form), { inputs: 'Wolfgang lives in Berlin.' });
  assert.deepEqual(buildRequest('zero-shot-classification', form), {
    inputs: 'Wolfgang lives in Berlin.',
    parameters: { candidate_labels: ['sports', 'politics', 'cooking'], multi_label: true },
  });
  assert.equal(buildRequest('zero-shot-classification', { ...form, labels: 'one' }), null);
  assert.deepEqual(buildRequest('token-classification', form).parameters, { aggregation_strategy: 'simple' });
  assert.deepEqual(buildRequest('question-answering', form), { inputs: { question: 'Where?', context: 'Berlin.' }, parameters: { top_k: 3 } });
  assert.equal(buildRequest('question-answering', { ...form, context: ' ' }), null);
  assert.equal(buildRequest('fill-mask', { ...form, text: '' }), null);
});

test('responses of every task are read', () => {
  // A single text is answered with a list holding one list.
  assert.deepEqual(parseResponse('text-classification', [[{ label: 'POSITIVE', score: 0.9 }, { label: 'NEGATIVE', score: 0.1 }]]), {
    kind: 'labels',
    items: [{ label: 'POSITIVE', score: 0.9 }, { label: 'NEGATIVE', score: 0.1 }],
  });
  assert.deepEqual(parseResponse('zero-shot-classification', { sequence: 's', labels: ['a', 'b'], scores: [0.7, 0.3] }).items, [
    { label: 'a', score: 0.7 },
    { label: 'b', score: 0.3 },
  ]);
  const ner = parseResponse('token-classification', [{ entity_group: 'PER', score: 0.99, word: 'Wolfgang', start: 0, end: 8 }]);
  assert.equal(ner.kind, 'entities');
  assert.equal(ner.items[0].entity, 'PER');
  assert.deepEqual(parseResponse('token-classification', []), { kind: 'entities', items: [] });
  const fill = parseResponse('fill-mask', [{ score: 0.98, token: 3007, token_str: ' capital', sequence: 'paris is the capital of france.' }]);
  assert.deepEqual(fill.items[0], { token: 'capital', score: 0.98, sequence: 'paris is the capital of france.' });
  assert.deepEqual(parseResponse('question-answering', { answer: 'Berlin', score: 0.99, start: 33, end: 39 }), {
    kind: 'answers',
    items: [{ answer: 'Berlin', score: 0.99, start: 33, end: 39 }],
  });
  assert.equal(parseResponse('question-answering', [{ answer: 'a', score: 0.5, start: 0, end: 1 }]).kind, 'answers');
  assert.deepEqual(parseResponse('image-classification', { foo: 1 }), { kind: 'raw', value: { foo: 1 } });
});

test('entity highlighting skips overlaps and out-of-range spans', () => {
  const text = 'Wolfgang lives in Berlin.';
  const spans = [
    { entity: 'LOC', word: 'Berlin', score: 1, start: 18, end: 24 },
    { entity: 'PER', word: 'Wolfgang', score: 1, start: 0, end: 8 },
    { entity: 'X', word: 'lf', score: 1, start: 2, end: 4 },
    { entity: 'Y', word: '', score: 1, start: 20, end: 99 },
  ];
  assert.deepEqual(highlight(text, spans), [
    { text: 'Wolfgang', entity: 'PER' },
    { text: ' lives in ' },
    { text: 'Berlin', entity: 'LOC' },
    { text: '.' },
  ]);
});
