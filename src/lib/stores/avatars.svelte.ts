import { invoke } from '#lib/ipc.js';

/**
 * Publisher avatar URLs for Hugging Face authors. Resolved (and cached on disk with a TTL) by
 * the backend; this store only batches requests and remembers the answers for the session.
 * `null` means "no avatar" (the UI shows initials).
 */
class AvatarStore {
  urls = $state<Record<string, string | null>>({});
  private asked = new Set<string>();
  private queue = new Set<string>();
  private timer: ReturnType<typeof setTimeout> | undefined;

  get(author: string): string | null | undefined {
    return this.urls[author.toLowerCase()];
  }

  /** Queues authors not asked about yet; requests go out in one batch shortly after. */
  request(authors: string[]) {
    for (const a of authors) {
      const key = a.toLowerCase();
      if (!a || this.asked.has(key)) continue;
      this.asked.add(key);
      this.queue.add(a);
    }
    if (this.queue.size === 0 || this.timer) return;
    this.timer = setTimeout(() => void this.flush(), 60);
  }

  private async flush() {
    this.timer = undefined;
    const batch = [...this.queue];
    this.queue.clear();
    try {
      const res = await invoke<Record<string, string | null>>('get_hf_avatars', { authors: batch });
      const next = { ...this.urls };
      for (const a of batch) next[a.toLowerCase()] = res[a] ?? null;
      this.urls = next;
    } catch {
      // Offline or backend error: allow a retry on the next request.
      for (const a of batch) this.asked.delete(a.toLowerCase());
    }
  }
}

export const avatars = new AvatarStore();
