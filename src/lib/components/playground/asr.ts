// Speech recognition with sherpa-onnx models (Parakeet, SenseVoice, Omnilingual...): the shapes of
// the Tauri commands and the pure helpers of the model picker. The text itself is made by the
// gateway's /v1/audio/transcriptions (a real sherpa-onnx run behind it; nothing is simulated).

/** An installed recognition model (`harness_core::asr::AsrModel`). */
export interface AsrModelInfo {
  id: string;
  name: string;
  family: string;
  languages: string[];
  quantization: string;
  size_bytes: number;
}

/** A recommended download (`harness_core::asr::catalog::CatalogEntry` plus `installed`). */
export interface AsrCatalogRow {
  id: string;
  title: string;
  family: string;
  languages: string[];
  size_bytes: number;
  license: string;
  url: string;
  sha256: string;
  recommended: boolean;
  installed: boolean;
}

export interface AsrOverview {
  models: AsrModelInfo[];
  catalog: AsrCatalogRow[];
  runtime_installed: boolean;
}

/** Runtime family that runs these models (the one text to speech uses too). */
export const SHERPA_FAMILY = 'sherpa-onnx';

/** Names a request's `model` can use for the installed sherpa-onnx recognition models. */
export function sherpaNames(models: AsrModelInfo[]): string[] {
  return models.map((m) => m.name);
}

/** A library file that sherpa-onnx runs: speech recognition, but ONNX (whisper.cpp's are ggml `.bin`). */
export function isSherpaAsrFile(file: { kind?: string | null; path: string }): boolean {
  return file.kind === 'asr' && /\.onnx$/i.test(file.path);
}

/** `en, de, fr, es, it, pt and 19 more` for the long language lists of a catalog row. */
export function languagesText(languages: string[], more: (list: string, count: number) => string, shown = 6): string {
  if (languages.length <= shown) return languages.join(', ');
  return more(languages.slice(0, shown).join(', '), languages.length - shown);
}
