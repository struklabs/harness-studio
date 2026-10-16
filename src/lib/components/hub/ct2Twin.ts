// CTranslate2 (faster-whisper) and WhisperKit Core ML exports can have
// a ggml twin in ggerganov/whisper.cpp that the whisper.cpp engine runs. The Hub points to it.

export const WHISPER_CPP_REPO = 'ggerganov/whisper.cpp';

export interface GgmlTwin {
  repo: string;
  file?: string;
}

/** Size part of a faster-whisper repo name (`small`, `small.en`, `large-v3`...) to its ggml file. */
const SIZE_TO_FILE: Record<string, string> = {
  tiny: 'ggml-tiny.bin',
  'tiny.en': 'ggml-tiny.en.bin',
  base: 'ggml-base.bin',
  'base.en': 'ggml-base.en.bin',
  small: 'ggml-small.bin',
  'small.en': 'ggml-small.en.bin',
  medium: 'ggml-medium.bin',
  'medium.en': 'ggml-medium.en.bin',
  'large-v1': 'ggml-large-v1.bin',
  'large-v2': 'ggml-large-v2.bin',
  'large-v3': 'ggml-large-v3.bin',
  'large-v3-turbo': 'ggml-large-v3-turbo.bin',
};

/** Exact repo ids (lower case) of CTranslate2 exports with a ggml twin. Every file exists in ggerganov/whisper.cpp. */
const EXPORTS: Record<string, string> = {
  'mobiuslabsgmbh/faster-whisper-large-v3-turbo': 'ggml-large-v3-turbo.bin',
  'deepdml/faster-whisper-large-v3-turbo-ct2': 'ggml-large-v3-turbo.bin',
};

/** A Whisper export's whisper.cpp twin; multi-size or unknown-size WhisperKit repos link without a file. */
export function ggmlTwinFor(repoId: string | null | undefined, paths: string[] = [], coreMl = false): GgmlTwin | null {
  if (!repoId) return null;
  const id = repoId.trim().toLowerCase();
  if (id === 'argmaxinc/whisperkit-coreml' || (coreMl && /(?:^|[/_-])whisperkit(?:[/_-]|$)/.test(id))) {
    const files = new Set(paths.flatMap((path) => {
      const size = /(?:^|\/)openai_whisper-([^/]+)(?:\/|$)/.exec(path.toLowerCase())?.[1];
      const file = size ? SIZE_TO_FILE[size] : undefined;
      return file ? [file] : [];
    }));
    return { repo: WHISPER_CPP_REPO, ...(files.size === 1 ? { file: [...files][0] } : {}) };
  }
  let file: string | undefined = EXPORTS[id];
  if (!file) {
    const m = /^systran\/faster-whisper-([a-z0-9.-]+)$/.exec(id);
    file = m ? SIZE_TO_FILE[m[1]] : undefined;
  }
  return file ? { repo: WHISPER_CPP_REPO, file } : null;
}

/** True when a variant (by file path or name) is the given ggml file. */
export function isTwinFile(variantFile: string | undefined, file: string | null | undefined): boolean {
  if (!variantFile || !file) return false;
  const base = (variantFile.split(/[\/]/).pop() ?? variantFile).toLowerCase();
  return base === file.toLowerCase();
}
