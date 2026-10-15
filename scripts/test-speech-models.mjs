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
});

test('LFM2.5-Audio and Qwen3-ASR only transcribe; llama.cpp runs them', () => {
  assert.ok(!audioLlmTranslates(lfm, files));
  assert.ok(audioLlmTranslates(voxtral, files));
  assert.equal(dictationFamily('lfm', [], [whisper, lfm], [], files), 'llama.cpp');
  assert.equal(dictationFamily('whisper', [], [whisper, lfm], [], files), 'whisper.cpp');
});
