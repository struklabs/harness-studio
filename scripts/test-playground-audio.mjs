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
