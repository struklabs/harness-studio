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
  /** Managed Python environment of `pyenv` families (its CPU build when it has a GPU one). */
  env?: string;
  /** Locale key of the one-line description shown on the Installed Runtime page. */
  descKey: 'eng.desc_audio_cpp' | 'eng.desc_transcribe_cpp' | 'eng.desc_crispasr' | 'eng.desc_whisper' | 'eng.desc_sdcpp' | 'eng.desc_sherpa' | 'eng.desc_lemonade' | 'eng.desc_onnx' | 'eng.desc_mistralrs' | 'eng.desc_pyhf' | 'eng.desc_mlx' | 'eng.desc_mlx_vlm' | 'eng.desc_mlx_audio' | 'eng.desc_mflux';
}

export const ENGINES: readonly EngineSpec[] = [
  { family: 'audio.cpp', method: 'github', repo: '0xShug0/audio.cpp', descKey: 'eng.desc_audio_cpp' },
  // Speech recognition GGUFs of handy-computer/transcribe.cpp: its Python binding in a managed
  // environment (the NVIDIA build where there is a CUDA driver).
  { family: 'transcribe.cpp', method: 'pyenv', env: 'transcribe-cpp', descKey: 'eng.desc_transcribe_cpp' },
  { family: 'crispasr', method: 'github', repo: 'CrispStrobe/CrispASR', descKey: 'eng.desc_crispasr' },
  { family: 'whisper.cpp', method: 'github', repo: 'ggml-org/whisper.cpp', descKey: 'eng.desc_whisper' },
  { family: 'stable-diffusion.cpp', method: 'github', repo: 'leejet/stable-diffusion.cpp', descKey: 'eng.desc_sdcpp' },
  { family: 'sherpa-onnx', method: 'github', repo: 'k2-fsa/sherpa-onnx', descKey: 'eng.desc_sherpa' },
  { family: 'onnxruntime', method: 'onnx', descKey: 'eng.desc_onnx' },
  { family: 'lemonade', method: 'lemonade', descKey: 'eng.desc_lemonade' },
  { family: 'mistral.rs', method: 'manual', descKey: 'eng.desc_mistralrs' },
  { family: 'python-hf', method: 'pyenv', env: 'hf-cpu', descKey: 'eng.desc_pyhf' },
  { family: 'mlx', method: 'pyenv', env: 'mlx', descKey: 'eng.desc_mlx' },
  { family: 'mlx-vlm', method: 'pyenv', env: 'mlx-vlm', descKey: 'eng.desc_mlx_vlm' },
  // Text to speech with mlx-audio: it runs in the mlx-vlm environment (one install for both).
  { family: 'mlx-audio', method: 'pyenv', env: 'mlx-vlm', descKey: 'eng.desc_mlx_audio' },
  // FLUX.2 [klein] image generation and editing with mflux (its own environment).
  { family: 'mflux', method: 'pyenv', env: 'mflux', descKey: 'eng.desc_mflux' },
];

/**
 * Description key of a family on this host: Lemonade on a Mac has no Ryzen AI NPU, and the Python
 * engine runs without network on Linux only (elsewhere it is reachable from this computer, with a key).
 */
export function engineDescKey(
  e: EngineSpec,
  mac: boolean,
  linux = false,
  /** The env the family uses or would install (`hf-cuda` for the Python engine's CUDA build). */
  env?: string,
): EngineSpec['descKey'] | 'eng.desc_lemonade_mac' | 'eng.desc_pyhf_other' | 'eng.desc_pyhf_cuda' | 'eng.desc_pyhf_cuda_other' {
  if (mac && e.family === 'lemonade') return 'eng.desc_lemonade_mac';
  if (e.family === 'python-hf' && env === 'hf-cuda') return linux ? 'eng.desc_pyhf_cuda' : 'eng.desc_pyhf_cuda_other';
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
  if (p.runtime === 'mlx' || p.runtime === 'mlx-vlm' || p.runtime === 'mlx-audio' || p.runtime === 'mflux') return { key: 'mlx.unsupported_macos', params: { platform: `${p.os}/${p.arch}` } };
  const intelMac = p.os === 'macos' && p.arch === 'x86_64';
  return { key: intelMac ? 'ort.unsupported_intel_mac' : 'ort.unsupported_platform', params: { platform: `${p.os}/${p.arch}` } };
}

/** One route a server serves directly (`path` under `/v1`, or under the host when `root`). */
export interface DirectRoute {
  method: 'GET' | 'POST' | 'GET · POST';
  labelKey: 'conn.ep_base_url_label' | 'conn.ep_models_label' | 'conn.ep_chat_label' | 'conn.ep_completions_label' | 'conn.ep_speech_label' | 'conn.ep_images_label' | 'conn.ep_image_edits_label';
  path: string;
}

/**
 * Routes of the MLX workers, which serve only these (anything else is 404). `null` for the
 * other families: they keep the llama-server list.
 */
export function workerRoutes(family: string | null | undefined): DirectRoute[] | null {
  const base: DirectRoute[] = [
    { method: 'GET · POST', labelKey: 'conn.ep_base_url_label', path: '' },
    { method: 'GET', labelKey: 'conn.ep_models_label', path: '/models' },
  ];
  switch (family) {
    case 'mlx':
      return [...base, { method: 'POST', labelKey: 'conn.ep_chat_label', path: '/chat/completions' }, { method: 'POST', labelKey: 'conn.ep_completions_label', path: '/completions' }];
    case 'mlx-vlm':
      return [...base, { method: 'POST', labelKey: 'conn.ep_chat_label', path: '/chat/completions' }];
    case 'mlx-audio':
      return [...base, { method: 'POST', labelKey: 'conn.ep_speech_label', path: '/audio/speech' }];
    case 'mflux':
      return [...base, { method: 'POST', labelKey: 'conn.ep_images_label', path: '/images/generations' }, { method: 'POST', labelKey: 'conn.ep_image_edits_label', path: '/images/edits' }];
    default:
      return null;
  }
}

/** Display names of engine families (`harness_core::engine::display_name` has the same). */
const ENGINE_LABELS: Record<string, string> = {
  'llama.cpp': 'llama.cpp',
  mlx: 'MLX',
  'mlx-vlm': 'MLX-VLM',
  'mlx-audio': 'MLX Audio',
  mflux: 'mflux',
  'python-hf': 'Python (HF)',
  'audio.cpp': 'audio.cpp',
  crispasr: 'CrispASR',
  'transcribe.cpp': 'transcribe.cpp',
  'mistral.rs': 'mistral.rs',
  'whisper.cpp': 'whisper.cpp',
  'stable-diffusion.cpp': 'sd.cpp',
  onnxruntime: 'ONNX Runtime',
  'sherpa-onnx': 'sherpa-onnx',
  lemonade: 'Lemonade',
};

/** Display name of an engine family (an unknown one as it is). */
export function engineLabel(family: string): string {
  return ENGINE_LABELS[family] ?? family;
}
