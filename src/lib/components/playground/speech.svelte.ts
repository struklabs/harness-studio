// One model selection for the Speech tab and chat dictation. The gateway resolves the
// same definition to its whisper.cpp runtime in both entry points.
export const speech = $state({ model: '' });
