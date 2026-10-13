// The engine (runtime) families a model can need beyond the official llama.cpp build, and how
// each one is installed. Pure data and helpers, no UI state: the installer is
// `stores/engineInstall.svelte.ts`.
import type { RuntimeView } from './types';

/** How a family is installed with one click. `manual` = no managed download exists. */
export type EngineMethod = 'github' | 'onnx' | 'lemonade' | 'pyenv' | 'manual';

export interface EngineSpec {
  /** Family id as the backend names it (`runtime.family_not_installed: whisper.cpp`). */
  family: string;
  method: EngineMethod;
  /** Official GitHub repository for `github` families. */
  repo?: string;
  /** Managed Python environment of `pyenv` families. */
  env?: string;
  /** Locale key of the one-line description shown on the Installed Runtime page. */
  descKey: 'eng.desc_whisper' | 'eng.desc_sdcpp' | 'eng.desc_sherpa' | 'eng.desc_lemonade' | 'eng.desc_onnx' | 'eng.desc_mistralrs' | 'eng.desc_pyhf' | 'eng.desc_mlx';
}

export const ENGINES: readonly EngineSpec[] = [
  { family: 'whisper.cpp', method: 'github', repo: 'ggml-org/whisper.cpp', descKey: 'eng.desc_whisper' },
  { family: 'stable-diffusion.cpp', method: 'github', repo: 'leejet/stable-diffusion.cpp', descKey: 'eng.desc_sdcpp' },
  { family: 'sherpa-onnx', method: 'github', repo: 'k2-fsa/sherpa-onnx', descKey: 'eng.desc_sherpa' },
  { family: 'onnxruntime', method: 'onnx', descKey: 'eng.desc_onnx' },
  { family: 'lemonade', method: 'lemonade', descKey: 'eng.desc_lemonade' },
  { family: 'mistral.rs', method: 'manual', descKey: 'eng.desc_mistralrs' },
  { family: 'python-hf', method: 'pyenv', env: 'hf-cpu', descKey: 'eng.desc_pyhf' },
  { family: 'mlx', method: 'pyenv', env: 'mlx', descKey: 'eng.desc_mlx' },
];

/**
 * Description key of a family on this host: Lemonade on a Mac has no Ryzen AI NPU, and the Python
 * engine runs without network on Linux only (elsewhere it is reachable from this computer, with a key).
 */
export function engineDescKey(
  e: EngineSpec,
  mac: boolean,
  linux = false,
): EngineSpec['descKey'] | 'eng.desc_lemonade_mac' | 'eng.desc_pyhf_other' {
  if (mac && e.family === 'lemonade') return 'eng.desc_lemonade_mac';
  if (!linux && e.family === 'python-hf') return 'eng.desc_pyhf_other';
  return e.descKey;
}

export function engineSpec(family: string | null | undefined): EngineSpec | undefined {
  return family ? ENGINES.find((e) => e.family === family) : undefined;
}

/** The family named by a raw backend failure ("…: runtime.family_not_installed: whisper.cpp"). */
export function familyFromError(message: string): string | null {
  return /runtime\.family_not_installed:\s*([\w.+-]+)/.exec(message)?.[1] ?? null;
}

/** An available runtime of the family is registered (`github`/`local` entries carry their family). */
export function familyInstalled(family: string, runtimes: readonly RuntimeView[]): boolean {
  return runtimes.some((r) => r.available && r.entry?.family === family);
}

/** 0-100 from a byte progress, or null while the size is unknown. */
export function progressPercent(downloaded: number, total: number): number | null {
  if (!(total > 0)) return null;
  return Math.max(0, Math.min(100, Math.round((downloaded / total) * 100)));
}

/**
 * Locale key and parameters of the "this engine does not run here" line, from the OS and
 * architecture the backend reports: ONNX Runtime on an Intel Mac gets its own wording, other
 * platforms are named; MLX on a Mac whose macOS is older than its wheels says so.
 */
export function onnxUnsupportedText(p: { os: string; arch: string; runtime?: string }): {
  key: 'ort.unsupported_intel_mac' | 'ort.unsupported_platform' | 'mlx.unsupported_macos';
  params: { platform: string };
} {
  if (p.runtime === 'mlx') return { key: 'mlx.unsupported_macos', params: { platform: `${p.os}/${p.arch}` } };
  const intelMac = p.os === 'macos' && p.arch === 'x86_64';
  return { key: intelMac ? 'ort.unsupported_intel_mac' : 'ort.unsupported_platform', params: { platform: `${p.os}/${p.arch}` } };
}
