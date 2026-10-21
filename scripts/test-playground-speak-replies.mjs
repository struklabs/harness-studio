import assert from 'node:assert/strict';
import { test } from 'node:test';
const { replySpeechSource, readSpeakReplies, writeSpeakReplies, SPEAK_REPLIES_KEY } =
  await import('../src/lib/components/playground/speakReplies.ts');
test('a finished reply is spoken only when the switch is on', () => {
  assert.equal(replySpeechSource(true, { content: 'Hello there' }), 'Hello there');
  assert.equal(replySpeechSource(false, { content: 'Hello there' }), null);
});
test('failed, stopped, tool-calling and blank replies stay silent', () => {
  assert.equal(replySpeechSource(true, { content: 'x', error: 'boom' }), null);
  assert.equal(replySpeechSource(true, { content: 'x', stopped: true }), null);
  assert.equal(replySpeechSource(true, { content: 'x', toolCalls: [{}] }), null);
  assert.equal(replySpeechSource(true, { content: '  \n' }), null);
});
test('the switch persists in storage and survives a blocked one', () => {
  const data = new Map();
  const storage = { getItem: k => data.get(k) ?? null, setItem: (k, v) => data.set(k, v), removeItem: k => data.delete(k) };
  assert.equal(readSpeakReplies(storage), false);
  writeSpeakReplies(storage, true);
  assert.equal(data.get(SPEAK_REPLIES_KEY), '1');
  assert.equal(readSpeakReplies(storage), true);
  writeSpeakReplies(storage, false);
  assert.equal(readSpeakReplies(storage), false);
  const blocked = { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('blocked'); }, removeItem() { throw new Error('blocked'); } };
  assert.equal(readSpeakReplies(blocked), false);
  assert.doesNotThrow(() => writeSpeakReplies(blocked, true));
  assert.equal(readSpeakReplies(undefined), false);
});
