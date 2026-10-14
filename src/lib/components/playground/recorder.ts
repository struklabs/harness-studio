// Speech tab: microphone capture with Web Audio. PCM is collected by an AudioWorklet and encoded
// to a 16 kHz mono WAV in the browser, so recording works without ffmpeg (no MediaRecorder).
import { concatChunks, encodeWav, recordingName, resample, WAV_RATE } from './wav';

/** Why recording could not start; the panel shows the localized text. */
export type RecordErrorCode = 'insecure' | 'unsupported' | 'denied' | 'no_device' | 'failed';

export class RecordError extends Error {
  constructor(
    readonly code: RecordErrorCode,
    detail?: string,
  ) {
    super(detail ?? code);
  }
}

/** Auto-stop after this long (a 10 minute WAV is about 19 MB). */
export const MAX_RECORD_SECONDS = 600;

export interface Recording {
  /** Stops, releases the microphone and returns the WAV as a file. */
  stop(): File;
  /** Stops and drops everything (tab switch, destroy). */
  cancel(): void;
}

function mapError(e: unknown): RecordError {
  const name = e instanceof DOMException ? e.name : '';
  if (name === 'NotAllowedError' || name === 'SecurityError') return new RecordError('denied');
  if (name === 'NotFoundError' || name === 'OverconstrainedError') return new RecordError('no_device');
  return new RecordError('failed', e instanceof Error ? e.message : String(e));
}

/**
 * Starts recording. `onLevel` gets the peak (0..1) of each audio block; `onLimit` runs once when
 * the length limit is reached (the caller then calls `stop()` for the file).
 */
export async function startRecording(onLevel: (peak: number) => void, onLimit: () => void, outputRate = WAV_RATE): Promise<Recording> {
  if (!window.isSecureContext) throw new RecordError('insecure');
  if (!navigator.mediaDevices?.getUserMedia || typeof AudioContext === 'undefined' || !('audioWorklet' in AudioContext.prototype)) {
    throw new RecordError('unsupported');
  }
  let stream: MediaStream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: false, noiseSuppression: false } });
  } catch (e) {
    throw mapError(e);
  }
  let ctx: AudioContext | null = null;
  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    stream.getTracks().forEach((tr) => tr.stop());
    void ctx?.close().catch(() => {});
  };
  const chunks: Float32Array[] = [];
  let frames = 0;
  let limited = false;
  try {
    // Ask for 16 kHz; a context that cannot (older engines) runs at its own rate and is resampled.
    try {
      ctx = new AudioContext({ sampleRate: outputRate });
    } catch {
      ctx = new AudioContext();
    }
    const rate = ctx.sampleRate;
    await ctx.audioWorklet.addModule('/pcm-recorder.worklet.js');
    const source = ctx.createMediaStreamSource(stream);
    const node = new AudioWorkletNode(ctx, 'pcm-recorder', { numberOfInputs: 1, numberOfOutputs: 0 });
    node.port.onmessage = (ev: MessageEvent<Float32Array>) => {
      if (released || limited) return;
      const block = ev.data;
      chunks.push(block);
      frames += block.length;
      let peak = 0;
      for (let i = 0; i < block.length; i++) peak = Math.max(peak, Math.abs(block[i]));
      onLevel(peak);
      if (frames >= rate * MAX_RECORD_SECONDS) {
        limited = true;
        onLimit();
      }
    };
    source.connect(node);
    await ctx.resume();
    const halt = () => {
      if (released) return;
      node.port.onmessage = null;
      source.disconnect();
      release();
    };
    return {
      stop() {
        halt();
        const wav = encodeWav(resample(concatChunks(chunks), rate, outputRate), outputRate);
        return new File([wav], recordingName(), { type: 'audio/wav' });
      },
      cancel: halt,
    };
  } catch (e) {
    release();
    throw mapError(e);
  }
}
