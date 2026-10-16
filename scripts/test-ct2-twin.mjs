import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ggmlTwinFor, isTwinFile } from '../src/lib/components/hub/ct2Twin.ts';

test('faster-whisper exports map to their ggml build', () => {
  const cases = {
    'Systran/faster-whisper-tiny': 'ggml-tiny.bin',
    'Systran/faster-whisper-tiny.en': 'ggml-tiny.en.bin',
    'Systran/faster-whisper-base': 'ggml-base.bin',
    'Systran/faster-whisper-small': 'ggml-small.bin',
    'Systran/faster-whisper-small.en': 'ggml-small.en.bin',
    'Systran/faster-whisper-medium': 'ggml-medium.bin',
    'Systran/faster-whisper-large-v2': 'ggml-large-v2.bin',
    'Systran/faster-whisper-large-v3': 'ggml-large-v3.bin',
    'mobiuslabsgmbh/faster-whisper-large-v3-turbo': 'ggml-large-v3-turbo.bin',
  };
  for (const [id, file] of Object.entries(cases)) {
    assert.deepEqual(ggmlTwinFor(id), { repo: 'ggerganov/whisper.cpp', file }, id);
  }
  assert.equal(ggmlTwinFor('systran/FASTER-WHISPER-small')?.file, 'ggml-small.bin');
});

test('repos without a ggml twin get no hint', () => {
  for (const id of ['Systran/faster-distil-whisper-large-v3', 'Systran/faster-whisper-large', 'Systran/faster-whisper-small/x', 'someone/faster-whisper-small', 'ggerganov/whisper.cpp', '', null, undefined]) {
    assert.equal(ggmlTwinFor(id), null, String(id));
  }
});

test('twin file matching ignores folders and case', () => {
  assert.equal(isTwinFile('ggml-small.bin', 'ggml-small.bin'), true);
  assert.equal(isTwinFile('sub/GGML-Small.bin', 'ggml-small.bin'), true);
  assert.equal(isTwinFile('ggml-small.en.bin', 'ggml-small.bin'), false);
  assert.equal(isTwinFile(undefined, 'ggml-small.bin'), false);
});

test('WhisperKit points to whisper.cpp with a file only for one known size', () => {
  const repo = 'ggerganov/whisper.cpp';
  assert.deepEqual(ggmlTwinFor('argmaxinc/whisperkit-coreml'), { repo });
  assert.deepEqual(ggmlTwinFor('argmaxinc/whisperkit-coreml', ['openai_whisper-large-v3/AudioEncoder.mlmodelc/weights/weight.bin']), { repo, file: 'ggml-large-v3.bin' });
  assert.deepEqual(ggmlTwinFor('argmaxinc/whisperkit-coreml', ['openai_whisper-small', 'openai_whisper-large-v3']), { repo });
  assert.deepEqual(ggmlTwinFor('argmaxinc/whisperkit-coreml', ['openai_whisper-unknown']), { repo });
  assert.deepEqual(ggmlTwinFor('owner/whisperkit-small', ['openai_whisper-small.en'], true), { repo, file: 'ggml-small.en.bin' });
});
