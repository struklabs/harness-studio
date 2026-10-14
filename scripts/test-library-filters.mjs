// Library tabs: which files each one lists.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { inTab, tabCounts } = await import('../src/lib/components/library/filters.ts');

const entry = (kind, extra = {}, runtime = null) => ({
  model: { name: 'm', kind, task: null, ...extra },
  record: { runtime },
  multimodal: false,
  draft_for: [],
});

test('mlx-audio folders are listed under Speech or Text to speech by their task', () => {
  const stt = entry('pipeline', { task: 'automatic-speech-recognition' }, 'mlx-audio');
  const tts = entry('pipeline', { task: 'text-to-speech' }, 'mlx-audio');
  assert.ok(inTab(stt, 'asr') && !inTab(stt, 'tts'));
  assert.ok(inTab(tts, 'tts') && !inTab(tts, 'asr'));
  const counts = tabCounts([stt, tts]);
  assert.equal(counts.asr, 1);
  assert.equal(counts.tts, 1);
  assert.equal(counts.all, 2);
});

test('other files keep their kind', () => {
  assert.ok(inTab(entry('asr'), 'asr'));
  assert.ok(inTab(entry('tts'), 'tts'));
  // A Python-engine speech pipeline is not an mlx-audio folder.
  const pyhf = entry('pipeline', { task: 'automatic-speech-recognition' }, 'python-hf');
  assert.ok(!inTab(pyhf, 'asr') && !inTab(pyhf, 'tts'));
  assert.ok(!inTab(entry('llm', {}, 'mlx'), 'asr'));
});
