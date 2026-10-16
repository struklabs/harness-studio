import { test } from 'node:test';
import assert from 'node:assert/strict';
import { audioLlmTranslates, dictationFamily, isAudioLlmDefinition, isTranscriptionDefinition } from '../src/lib/components/playground/speechModels.ts';

const def = (alias, path, mmproj) => ({ alias, base: { kind: 'local', path }, mmproj });
const files = [
  { path: 'C:/m/mmproj-LFM2.5-Audio-1.5B-Q4_0.gguf', gguf: { has_audio_encoder: true, audio_projector: 'lfm2a' } },
  { path: 'C:/m/mmproj-voxtral.gguf', gguf: { has_audio_encoder: true, audio_projector: 'voxtral' } },
  { path: 'C:/m/mmproj-gemma.gguf', gguf: { has_audio_encoder: false } },
];
const lfm = def('lfm', 'C:/m/LFM2.5-Audio-1.5B-Q4_0.gguf', 'C:\\m\\mmproj-LFM2.5-Audio-1.5B-Q4_0.gguf');
const voxtral = def('voxtral', 'C:/m/voxtral.gguf', 'C:/m/mmproj-voxtral.gguf');
const gemma = def('gemma', 'C:/m/gemma.gguf', 'C:/m/mmproj-gemma.gguf');
const whisper = def('whisper', 'C:/m/ggml-tiny.bin', null);

test('a chat model whose projector hears is offered for transcription', () => {
  assert.ok(isAudioLlmDefinition(lfm, files), 'path separators and case do not matter');
  assert.ok(!isAudioLlmDefinition(gemma, files), 'a vision projector does not hear');
  assert.ok(!isAudioLlmDefinition(lfm, []), 'an unknown projector file is not assumed to hear');
  assert.ok(isTranscriptionDefinition(whisper, files) && isTranscriptionDefinition(voxtral, files));
  // Like the gateway's `projector_of`: the model itself is a local GGUF.
  assert.ok(!isAudioLlmDefinition({ ...lfm, base: { kind: 'local', path: 'C:/m/lfm-folder' } }, files));
  assert.ok(!isAudioLlmDefinition({ ...lfm, base: { kind: 'hub', repo: 'a/b', file: 'x.gguf' } }, files));
  // Qwen3-TTS's speaker encoder is an audio encoder that does not hear speech.
  const tts = def('tts', 'C:/m/Qwen3-TTS.gguf', 'C:/m/mmproj-Qwen3-TTS.gguf');
  assert.ok(!isAudioLlmDefinition(tts, [{ path: 'C:/m/mmproj-Qwen3-TTS.gguf', gguf: { has_audio_encoder: true, audio_projector: 'qwen3tts_spkenc' } }]));
});

test('LFM2.5-Audio and Qwen3-ASR only transcribe; llama.cpp runs them', () => {
  assert.ok(!audioLlmTranslates(lfm, files));
  assert.ok(audioLlmTranslates(voxtral, files));
  assert.equal(dictationFamily('lfm', [], [whisper, lfm], [], files), 'llama.cpp');
  assert.equal(dictationFamily('whisper', [], [whisper, lfm], [], files), 'whisper.cpp');
});

test('transcribe.cpp models transcribe on their managed engine and translate only when their header says so', async () => {
  const { dictationRequirement, isSpeechDefinition, transcribeCppCanTranslate } = await import('../src/lib/components/playground/speechModels.ts');
  const parakeet = { alias: 'parakeet', base: { kind: 'local', path: 'C:/m/parakeet-tdt-0.6b-v3-Q4_K_M.gguf' }, load: { task: 'transcribe_cpp' } };
  const canary = { alias: 'canary', base: { kind: 'local', path: 'C:/m/canary-1b-v2-Q4_K_M.gguf' }, load: { task: 'transcribe_cpp' } };
  const headers = [
    { path: 'C:/m/parakeet-tdt-0.6b-v3-Q4_K_M.gguf', gguf: { stt_translate: false } },
    { path: 'C:\\m\\canary-1b-v2-Q4_K_M.gguf', gguf: { stt_translate: true } },
  ];
  assert.ok(isSpeechDefinition(parakeet) && isTranscriptionDefinition(parakeet, headers));
  assert.equal(dictationFamily('parakeet', [], [parakeet], [], headers), 'transcribe.cpp');
  assert.ok(!transcribeCppCanTranslate(parakeet, headers));
  assert.ok(transcribeCppCanTranslate(canary, headers), 'path separators do not matter');
  assert.ok(!transcribeCppCanTranslate(canary, []), 'an unknown file does not translate');
  // Its environment is managed (Settings installs it); no runtime entry is needed, only the gateway.
  assert.equal(dictationRequirement('parakeet', [], false, [], [parakeet], [], headers), 'gateway');
  assert.equal(dictationRequirement('parakeet', [], true, [], [parakeet], [], headers), null);
});
