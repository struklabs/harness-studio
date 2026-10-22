import assert from 'node:assert/strict';
import { test } from 'node:test';
import { registerHooks } from 'node:module';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
registerHooks({ resolve(specifier, context, next) {
  if (specifier.startsWith('./') && context.parentURL && !specifier.endsWith('.ts')) {
    const url = new URL(`${specifier}.ts`, context.parentURL);
    if (existsSync(fileURLToPath(url))) return next(url.href, context);
  }
  return next(specifier, context);
} });
const { sniffAudioFormat, audioPart, audioOfMeta, loadAudio, MAX_AUDIO_BYTES } = await import('../src/lib/components/playground/audio.ts');
const { userContent } = await import('../src/lib/components/playground/images.ts');
const { flattenToolHistory } = await import('../src/lib/components/playground/toolHistory.ts');
const { AudioInputError, chatRequest, prepareAudioRequest } = await import('../src/lib/components/playground/chatRequest.ts');
const bytes = text => Uint8Array.from(Buffer.from(text));
const audio = { name: 'speech.wav', format: 'wav', data: 'UklGRg==' };
test('sniffs WAV, MP3 and FLAC signatures', () => {
  assert.equal(sniffAudioFormat(bytes('RIFF0000WAVE')), 'wav');
  assert.equal(sniffAudioFormat(bytes('RIFF0000AVI ')), null);
  assert.equal(sniffAudioFormat(bytes('ID3')), 'mp3');
  assert.equal(sniffAudioFormat(new Uint8Array([0xff, 0xfb])), 'mp3');
  assert.equal(sniffAudioFormat(bytes('fLaC')), 'flac');
  assert.equal(sniffAudioFormat(bytes('OggS')), null);
  assert.equal(sniffAudioFormat(new Uint8Array()), null);
});
test('builds audio content without a data URL prefix, alongside text and images', () => {
  const part = { type: 'input_audio', input_audio: { data: audio.data, format: 'wav' } };
  assert.deepEqual(audioPart(audio), part);
  assert.deepEqual(userContent('Listen', [], [audio]), [{ type: 'text', text: 'Listen' }, part]);
  assert.deepEqual(userContent('', [], [audio]), [part]);
  assert.equal(userContent('Text'), 'Text');
  const image = { name: 'x', mime: 'image/png', data: 'data:image/png;base64,AAAA' };
  assert.deepEqual(userContent('Both', [image], [audio]).map(p => p.type), ['text', 'image_url', 'input_audio']);
});
test('restores saved audio and preserves parts while flattening tools', () => {
  assert.deepEqual(audioOfMeta({ audio: [audio] }), [audio]);
  assert.equal(audioOfMeta({ audio: [{ ...audio, format: 'ogg' }] }), undefined);
  assert.equal(audioOfMeta({ audio: [{ ...audio, data: 'A'.repeat(MAX_AUDIO_BYTES * 2) }] }), undefined);
  const parts = userContent('Listen', [], [audio]);
  const messages = [{ role: 'user', content: parts }, { role: 'assistant', content: '', tool_calls: [{ id: '1', function: { name: 'lookup', arguments: '{}' } }] }, { role: 'tool', tool_call_id: '1', content: 'Done' }];
  assert.deepEqual(flattenToolHistory(messages)[0].content, parts);
  assert.equal(messages.length, 3);
});

test('loads signed files, rejects spoofed extensions and oversized files', async () => {
  const wav = bytes('RIFF0000WAVE');
  const clip = await loadAudio(new File([wav], 'speech.mp3', { type: 'audio/mpeg' }));
  assert.equal(clip.format, 'wav');
  assert.equal(clip.data, Buffer.from(wav).toString('base64'));
  await assert.rejects(loadAudio(new File([bytes('OggS')], 'fake.wav')), e => e.code === 'type');
  await assert.rejects(loadAudio(new File([new Uint8Array(MAX_AUDIO_BYTES + 1)], 'large.wav')), e => e.code === 'too_large');
  for (const format of ['wav', 'mp3', 'flac']) assert.equal(audioPart({ ...audio, format }).input_audio.format, format);
});

const audioTurn = () => ({ role: 'user', content: userContent('What does the speaker say?', [], [{ ...audio, name: 'asr-in.wav' }]) });
const signal = () => new AbortController().signal;
const tools = [{ type: 'function', function: { name: 'lookup', parameters: { type: 'object' } } }];

test('blocks current audio before discovery or completion for a model without audio input', async () => {
  let sends = 0;
  const send = async () => { sends++; return Response.json({}); };
  for (const messages of [
    [audioTurn()],
    [audioTurn(), { role: 'assistant', content: 'Earlier answer to regenerate' }],
    [audioTurn(), { role: 'assistant', tool_calls: [{ id: '1', function: { name: 'lookup', arguments: '{}' } }] }, { role: 'tool', content: 'done', tool_call_id: '1' }],
    [{ role: 'user', content: [audioPart(audio)] }],
  ]) {
    const body = { model: 'SmolVLM2', messages, tools };
    const before = structuredClone(body);
    await assert.rejects(chatRequest('local', body, {}, signal(), send, { audioInput: false }), AudioInputError);
    assert.deepEqual(body, before);
  }
  // Unknown capability must not accidentally enable audio either.
  await assert.rejects(chatRequest('local', { messages: [audioTurn()] }, {}, signal(), send), AudioInputError);
  assert.equal(sends, 0);
});

test('audio-capable models receive the original clip, with no transcript or filename conversion', async () => {
  const body = { model: 'audio-model', messages: [audioTurn()] };
  const sent = [];
  await chatRequest('local', body, {}, signal(), async (_, init) => {
    sent.push(JSON.parse(init.body));
    return Response.json({});
  }, { audioInput: true });
  assert.deepEqual(sent, [body]);
  assert.equal(sent[0].messages[0].content[1].input_audio.data, audio.data);
  assert.ok(!JSON.stringify(sent).includes('asr-in.wav'));
});

test('discovery hears no clip and its guessed answer is never injected as a transcript', async () => {
  const body = { model: 'audio-model', messages: [audioTurn()], tools };
  const sent = [];
  const result = await chatRequest('local', body, {}, signal(), async (_, init) => {
    const request = JSON.parse(init.body);
    sent.push(request);
    return sent.length === 1
      ? Response.json({ choices: [{ message: { content: 'The speaker says DISCOVERY_GUESS.' } }] })
      : Response.json({});
  }, { audioInput: true });
  assert.equal(sent.length, 2);
  assert.ok(!JSON.stringify(sent[0]).includes(audio.data));
  assert.ok(!JSON.stringify(sent[0]).includes('asr-in.wav'));
  assert.deepEqual(sent[1].messages, body.messages);
  assert.ok(!JSON.stringify(sent[1]).includes('DISCOVERY_GUESS'));
  assert.equal(result.routingFallback, true);
  assert.equal(result.routingWithoutTools, true);
});

test('text follow-ups omit only old audio, notify before sending, and keep text, images and earlier answers', async () => {
  const image = { name: 'photo.png', mime: 'image/png', data: 'data:image/png;base64,AAAA' };
  const body = { model: 'SmolVLM2', messages: [
    { role: 'user', content: userContent('Listen and look', [image], [audio]) },
    { role: 'assistant', content: 'The speaker says hello.' },
    { role: 'user', content: 'Summarize that answer.' },
  ] };
  const before = structuredClone(body);
  let notices = 0;
  const result = await chatRequest('local', body, {}, signal(), async (_, init) => {
    assert.equal(notices, 1);
    const sent = JSON.parse(init.body);
    assert.deepEqual(sent.messages[0].content, userContent('Listen and look', [image]));
    assert.deepEqual(sent.messages.slice(1), body.messages.slice(1));
    assert.ok(!JSON.stringify(sent).includes('input_audio'));
    return Response.json({});
  }, { audioInput: false, onAudioHistoryOmitted: () => { notices++; } });
  assert.equal(result.response.ok, true);
  assert.deepEqual(body, before);
  // Switching back to a model that hears must restore every original attachment.
  assert.equal(prepareAudioRequest(body, true).body, body);
});

test('old audio-only turns become valid empty text and omitted audio stays absent through routing retries', async () => {
  const body = { model: 'vision-only', tools, messages: [
    { role: 'user', content: [audioPart(audio), audioPart(audio)] },
    { role: 'assistant', content: 'An earlier answer' },
    { role: 'user', content: 'Continue in text' },
  ] };
  let sends = 0, notices = 0;
  const result = await chatRequest('local', body, {}, signal(), async (_, init) => {
    const sent = JSON.parse(init.body);
    assert.ok(!JSON.stringify(sent).includes('input_audio'));
    assert.equal(notices, 1);
    sends++;
    if (sends === 1) return Response.json({ choices: [{ message: { content: '["lookup"]' } }] });
    assert.equal(sent.messages[0].content, '');
    if (sends === 2) return new Response('Tools are not supported', { status: 400 });
    return Response.json({});
  }, { audioInput: false, onAudioHistoryOmitted: () => { notices++; } });
  assert.equal(sends, 3);
  assert.equal(result.response.ok, true);
  assert.equal(notices, 1);
});

test('a new audio turn is blocked even when earlier audio could otherwise be omitted', async () => {
  assert.throws(() => prepareAudioRequest({ messages: [audioTurn(), { role: 'assistant', content: 'ok' }, audioTurn()] }, false), AudioInputError);
  assert.throws(() => prepareAudioRequest({ messages: [{ role: 'assistant', content: [audioPart(audio)] }] }, false), AudioInputError);
  const body = { messages: [{ role: 'user', content: 'Text only' }] };
  assert.deepEqual(prepareAudioRequest(body, false), { body, audioHistoryOmitted: false });
});
