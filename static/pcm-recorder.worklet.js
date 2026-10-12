// AudioWorklet for the Speech tab's microphone recording (src/lib/components/playground/recorder.ts).
// Served from the app's own origin, so the CSP (`script-src 'self'`) needs no blob: exception.
// Posts every 128-frame block of the first input channel to the main thread.
class PcmRecorder extends AudioWorkletProcessor {
  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (channel && channel.length) this.port.postMessage(channel.slice());
    return true;
  }
}
registerProcessor('pcm-recorder', PcmRecorder);
