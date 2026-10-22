import type { LocalModel, ModelDefinition } from '#lib/types.js';

export const VIDEO_TASKS = ['text-to-video', 'image-to-video', 'video-text-to-text'];
export function isVideoTask(task?: string): boolean { return VIDEO_TASKS.includes(task ?? ''); }
/** GGUF architectures of the video models stable-diffusion.cpp runs (`POST /v1/videos` through sd-server). */
export const VIDEO_GGUF_ARCHS = ['wan', 'ltxv', 'hyvid', 'cosmos'];
/**
 * The video task of a library file: a Python pipeline's own task, or for an sd.cpp video GGUF
 * (no task of its own) image-to-video when its name says I2V, text-to-video otherwise.
 */
export function fileVideoTask(file?: { kind?: string; task?: string; name?: string; path?: string; gguf?: { architecture?: string | null } | null }): string | undefined {
  if (!file) return undefined;
  if (isVideoTask(file.task)) return file.task;
  if (file.kind !== 'image' || !VIDEO_GGUF_ARCHS.includes(file.gguf?.architecture ?? '')) return undefined;
  return /(^|[^t])i2v/i.test(file.name ?? file.path ?? '') ? 'image-to-video' : 'text-to-video';
}
/** GGUF VLMs keep their Chat entry and also answer sampled video questions in Tasks/Video. */
export function definitionVideoTask(def: ModelDefinition, file?: LocalModel, files: LocalModel[] = []): string | undefined {
  const task = fileVideoTask(file);
  if (task) return task;
  const path = def.base.kind === 'local' ? def.base.path : def.base.file;
  if (!/\.gguf$/i.test(path) || !def.mmproj || (file && file.kind !== 'llm')) return undefined;
  const norm = (p: string) => p.replace(/\\/g, '/').toLowerCase();
  const projector = files.find(f => norm(f.path) === norm(def.mmproj!));
  if (projector?.gguf?.has_audio_encoder && !projector.gguf.has_vision_encoder) return undefined;
  return 'video-text-to-text';
}

export function videoQuestion(model: string, prompt: string, media: string) {
  return { model, stream: false, max_tokens: 128, messages: [{ role: 'user', content: [
    { type: 'text', text: prompt }, { type: 'video_url', video_url: { url: media } },
  ] }] };
}

/** Containers `POST /v1/videos` accepts (`webp` is animated). */
export const VIDEO_FORMATS = ['mp4', 'webm', 'webp', 'avi'] as const;
export type VideoFormat = (typeof VIDEO_FORMATS)[number];

/** The containers a model's engine writes, as the gateway accepts them: sd.cpp video GGUFs no mp4,
 *  Python diffusers pipelines mp4 or webm. The first one is the default. */
export function videoFormatsFor(file?: Parameters<typeof fileVideoTask>[0]): readonly VideoFormat[] {
  const sdcpp = file?.kind === 'image' && VIDEO_GGUF_ARCHS.includes(file.gguf?.architecture ?? '');
  return sdcpp ? ['webm', 'webp', 'avi'] : ['mp4', 'webm'];
}

const VIDEO_MIME: Record<VideoFormat, string> = { mp4: 'video/mp4', webm: 'video/webm', webp: 'image/webp', avi: 'video/x-msvideo' };
export function videoMime(format: string): string { return VIDEO_MIME[format as VideoFormat] ?? 'video/webm'; }

/** How a finished file is previewed: a player, an image (animated webp), or only saved (avi has no web player). */
export function videoPreview(format: string): 'video' | 'image' | 'file' {
  return format === 'webp' ? 'image' : format === 'avi' ? 'file' : 'video';
}

export interface VideoJobStatus { status: string; progress?: number; error?: { message?: string } }

/** Progress shown for a job: 0-100, never NaN. */
export function jobPercent(status: Pick<VideoJobStatus, 'progress'>): number {
  const value = Number(status.progress);
  return Number.isFinite(value) ? Math.min(100, Math.max(0, Math.round(value))) : 0;
}

/** Polls a video job until it completes; throws on `failed` and when `signal` aborts. */
export async function waitForVideo(
  poll: () => Promise<VideoJobStatus>,
  opts: { signal: AbortSignal; onProgress?: (percent: number, status: string) => void; intervalMs?: number; sleep?: (ms: number) => Promise<void> },
): Promise<void> {
  const sleep = opts.sleep ?? ((ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms)));
  for (;;) {
    opts.signal.throwIfAborted();
    const status = await poll();
    opts.onProgress?.(jobPercent(status), status.status);
    if (status.status === 'failed') throw new Error(status.error?.message ?? status.status);
    if (status.status === 'completed') return;
    await sleep(opts.intervalMs ?? 1000);
  }
}
