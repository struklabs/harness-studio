export const VIDEO_TASKS = ['text-to-video', 'image-to-video', 'video-text-to-text'];
export function isVideoTask(task?: string): boolean { return VIDEO_TASKS.includes(task ?? ''); }
export function videoQuestion(model: string, prompt: string, media: string) {
  return { model, stream: false, max_tokens: 128, messages: [{ role: 'user', content: [
    { type: 'text', text: prompt }, { type: 'video_url', video_url: { url: media } },
  ] }] };
}
