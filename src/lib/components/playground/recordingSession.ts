// Microphone lifecycle shared by Speech and chat dictation. The recorder owns PCM/WAV;
// this session owns permission races, elapsed time, automatic stop and cleanup.
import { MAX_RECORD_SECONDS, startRecording, type Recording } from './recorder';

export type RecordingState = 'idle' | 'starting' | 'recording';

export function recordingClock(seconds: number): string {
  return `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
}

export function appendTranscript(draft: string, transcript: string): string {
  const text = transcript.trim();
  if (!text) return draft;
  return `${draft}${draft && !/\s$/.test(draft) ? '\n' : ''}${text}`;
}

export function createRecordingSession(callbacks: {
  state: (state: RecordingState) => void;
  seconds: (seconds: number) => void;
  level: (peak: number) => void;
  file: (file: File) => void;
  error: (error: unknown) => void;
}, start: typeof startRecording = startRecording) {
  let generation = 0;
  let state: RecordingState = 'idle';
  let recording: Recording | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  const setState = (next: RecordingState) => { state = next; callbacks.state(next); };
  const clearTimer = () => {
    if (timer !== null) clearInterval(timer);
    timer = null;
    callbacks.level(0);
  };
  function cancel() {
    generation++;
    recording?.cancel();
    recording = null;
    clearTimer();
    setState('idle');
  }
  function finish() {
    const r = recording;
    recording = null;
    clearTimer();
    setState('idle');
    if (!r) return;
    try { callbacks.file(r.stop()); } catch (e) { r.cancel(); callbacks.error(e); }
  }
  async function record() {
    if (state !== 'idle') return;
    const token = ++generation;
    setState('starting');
    try {
      const r = await start(callbacks.level, finish);
      // A permission dialog can outlive cancellation, navigation or the component.
      if (token !== generation) { r.cancel(); return; }
      recording = r;
      callbacks.seconds(0);
      const started = performance.now();
      timer = setInterval(() => {
        const seconds = (performance.now() - started) / 1000;
        callbacks.seconds(seconds);
        if (seconds >= MAX_RECORD_SECONDS + 5) finish();
      }, 250);
      setState('recording');
    } catch (e) {
      if (token !== generation) return;
      clearTimer();
      setState('idle');
      callbacks.error(e);
    }
  }
  return { record, finish, cancel };
}
