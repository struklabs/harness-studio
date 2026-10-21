export const VIDEO_TASKS = ['text-to-video', 'image-to-video', 'video-text-to-text'];
export function isVideoTask(task?: string): boolean { return VIDEO_TASKS.includes(task ?? ''); }
export function videoQuestion(model: string, prompt: string, media: string) {
  return { model, stream: false, max_tokens: 128, messages: [{ role: 'user', content: [
    { type: 'text', text: prompt }, { type: 'video_url', video_url: { url: media } },
  ] }] };
}

/** Containers `POST /v1/videos` accepts (`webp` is animated). */
export const VIDEO_FORMATS = ['mp4', 'webm', 'webp', 'avi'] as const;
export type VideoFormat = (typeof VIDEO_FORMATS)[number];

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
