// Speech tab recording: pure helpers that turn captured PCM into a 16 kHz mono 16-bit WAV, the
// format whisper-server reads directly (no ffmpeg needed).

/** What whisper models are trained on. */
export const WAV_RATE = 16000;

/** Joins the captured blocks into one buffer. */
export function concatChunks(chunks: Float32Array[]): Float32Array {
  let total = 0;
  for (const c of chunks) total += c.length;
  const out = new Float32Array(total);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

/**
 * Resamples to `to` Hz. Downsampling averages the source frames each output sample covers (a
 * box filter, enough to avoid aliasing for speech); upsampling interpolates linearly.
 */
export function resample(input: Float32Array, from: number, to: number): Float32Array {
  if (from === to || input.length === 0) return input;
  const outLen = Math.max(1, Math.round((input.length * to) / from));
  const out = new Float32Array(outLen);
  const ratio = from / to;
  for (let i = 0; i < outLen; i++) {
    const start = i * ratio;
    const a = Math.floor(start);
    if (ratio > 1) {
      const b = Math.min(input.length, Math.max(a + 1, Math.floor(start + ratio)));
      let sum = 0;
      for (let j = a; j < b; j++) sum += input[j];
      out[i] = sum / (b - a);
    } else {
      const x0 = input[Math.min(a, input.length - 1)];
      const x1 = input[Math.min(a + 1, input.length - 1)];
      out[i] = x0 + (x1 - x0) * (start - a);
    }
  }
  return out;
}

/** A canonical 44-byte-header PCM WAV (mono, 16-bit little endian) from float samples in [-1, 1]. */
export function encodeWav(samples: Float32Array, rate: number): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(44 + samples.length * 2);
  const view = new DataView(bytes.buffer);
  const tag = (at: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(at + i, s.charCodeAt(i));
  };
  tag(0, 'RIFF');
  view.setUint32(4, 36 + samples.length * 2, true);
  tag(8, 'WAVE');
  tag(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 2, true); // byte rate
  view.setUint16(32, 2, true); // block align
  view.setUint16(34, 16, true); // bits per sample
  tag(36, 'data');
  view.setUint32(40, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return bytes;
}

/** `kayit-YYYYMMDD-HHMMSS.wav` in local time. */
export function recordingName(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `kayit-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}.wav`;
}
