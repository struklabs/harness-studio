// Studios: task-specific areas of the Playground. Each one names the existing surface it
// reuses (a tab, panel or module) so nothing is built twice; this file holds the pure
// pieces (no runtime imports, so node tests load it): the list, the resource facts and the
// memory estimate. Icons live in StudioPanel.
import type { LocalModel, PlaygroundMode } from '#lib/types.js';
import type { TKey } from '#lib/i18n.svelte.js';
import { engineLabel } from '../../engines.ts';

export type StudioId = 'image' | 'audio' | 'video' | 'voice' | 'document' | 'coding' | 'analysis' | 'agent';

export interface StudioSpec {
  id: StudioId;
  label: TKey;
  desc: TKey;
  /** Playground mode whose model picker (and model list) the studio uses. */
  mode: PlaygroundMode;
  /** Gateway route the studio calls, shown as the API hint. */
  api: string;
  streaming: boolean;
  /** RAM/VRAM on top of the model file: activations, caches, decoded frames. */
  memoryFactor: number;
}

export const STUDIOS: StudioSpec[] = [
  { id: 'image', label: 'studio.image', desc: 'studio.image_desc', mode: 'image', api: '/v1/images/generations', streaming: false, memoryFactor: 1.3 },
  { id: 'audio', label: 'studio.audio', desc: 'studio.audio_desc', mode: 'transcription', api: '/v1/audio/transcriptions, /v1/audio/speech', streaming: true, memoryFactor: 1.2 },
  { id: 'video', label: 'studio.video', desc: 'studio.video_desc', mode: 'pipeline', api: '/v1/videos', streaming: false, memoryFactor: 1.6 },
  { id: 'voice', label: 'studio.voice', desc: 'studio.voice_desc', mode: 'transcription', api: 'local audio.cpp / mlx-audio', streaming: false, memoryFactor: 1.25 },
  { id: 'document', label: 'studio.document', desc: 'studio.document_desc', mode: 'ocr', api: '/v1/chat/completions', streaming: false, memoryFactor: 1.25 },
  { id: 'coding', label: 'studio.coding', desc: 'studio.coding_desc', mode: 'chat', api: '/v1/chat/completions', streaming: true, memoryFactor: 1.15 },
  { id: 'analysis', label: 'studio.analysis', desc: 'studio.analysis_desc', mode: 'chat', api: '/v1/chat/completions', streaming: true, memoryFactor: 1.15 },
  { id: 'agent', label: 'studio.agent', desc: 'studio.agent_desc', mode: 'chat', api: '/v1/responses', streaming: true, memoryFactor: 1.15 },
];

export function studioById(id: string | null | undefined): StudioSpec | undefined {
  return STUDIOS.find(studio => studio.id === id);
}

/** Starter text a chat studio puts into the draft (kept short; the user edits it). */
export const STUDIO_STARTERS: Partial<Record<StudioId, string>> = {
  coding: 'You are a careful pair programmer. Explain the approach briefly, then show the code.\n\n',
  analysis: 'Analyze the data below. State the question, the method, the result and the caveats.\n\n',
};

/** Estimated RAM/VRAM in bytes for a model file in a studio; null when the size is unknown. */
export function studioMemoryEstimate(studio: StudioSpec, sizeBytes: number | null | undefined, opts: { frames?: number; size?: string } = {}): number | null {
  if (!sizeBytes || sizeBytes <= 0) return null;
  let factor = studio.memoryFactor;
  if (studio.id === 'video') {
    // Every extra frame and pixel keeps latents around: grow with the request, capped.
    const frames = Math.min(Math.max(opts.frames ?? 9, 1), 129);
    const [w, h] = (opts.size ?? '256x256').split('x').map(Number);
    const pixels = Number.isFinite(w) && Number.isFinite(h) && w > 0 && h > 0 ? w * h : 256 * 256;
    factor += Math.min(2, (frames / 9) * (pixels / (256 * 256)) * 0.1);
  }
  return Math.round(sizeBytes * factor);
}

export type StudioLocation = 'local' | 'remote' | 'cloud';

/** What a studio shows about the model it runs: engine family, where, and the memory estimate. */
export interface StudioResources {
  alias: string;
  engine: string | null;
  location: StudioLocation;
  sizeBytes: number | null;
  memoryBytes: number | null;
}

/** Engine family of a library file, from its kind and task (the name the Engines page uses). */
export function engineOfFile(file: LocalModel | undefined): string | null {
  if (!file) return null;
  switch (file.kind) {
    case 'image': return 'stable-diffusion.cpp';
    case 'asr': return /\.bin$/i.test(file.path) ? 'whisper.cpp' : 'CrispASR';
    case 'pipeline': return /video|image/.test(file.task ?? '') ? 'Python (diffusers)' : 'Python (transformers)';
    case 'llm': case 'embedding': case 'reranker': return 'llama.cpp';
    default: return null;
  }
}

/**
 * `file` is the library file of the alias's definition (`definitionFile`), when it has one;
 * `companionBytes` the definition's VAE and text encoders, loaded with it (Wan's umt5 is larger
 * than the model); `runtime` the engine family the model registry gives the file (an MLX folder's
 * kind and task look like llama.cpp's or diffusers').
 */
export function studioResources(studio: StudioSpec, alias: string, file: LocalModel | undefined, opts: { frames?: number; size?: string; companionBytes?: number; runtime?: string | null } = {}): StudioResources {
  const location: StudioLocation = alias.startsWith('remote:') ? 'remote' : alias.startsWith('cloud:') ? 'cloud' : 'local';
  const sizeBytes = location === 'local' && file ? file.size_bytes + Math.max(0, opts.companionBytes ?? 0) : null;
  return {
    alias,
    engine: location !== 'local' ? null : opts.runtime && file ? engineLabel(opts.runtime) : engineOfFile(file),
    location,
    sizeBytes,
    memoryBytes: location === 'local' ? studioMemoryEstimate(studio, sizeBytes, opts) : null,
  };
}

/** `#playground/studio` or `#playground/studio/video` -> the studio to open (default image). */
export function studioFromHash(hash: string): StudioId | undefined {
  const m = hash.match(/^#playground\/studio(?:\/(.+))?$/);
  if (!m) return undefined;
  return studioById(m[1])?.id ?? 'image';
}
