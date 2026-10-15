// Pure transcript window and bounded render-cache helpers. No UI or browser imports.
export const TAIL_MESSAGES = 32;
export const HISTORY_BATCH = 16;
/** Smaller batches while idle keep each mount well under a long task. */
export const IDLE_HISTORY_BATCH = 6;

export function tailStart(length: number): number {
  return Math.max(0, length - TAIL_MESSAGES);
}

export function previousBatch(start: number, target = 0): number {
  return Math.max(target, start - HISTORY_BATCH, 0);
}

/** Keep an existing path position when a branch changes, otherwise show its new tail. */
export function pathStart(ids: readonly string[], firstId: string | undefined): number {
  const index = firstId === undefined ? -1 : ids.indexOf(firstId);
  return index < 0 ? tailStart(ids.length) : index;
}

/** True while the list is still near its top and older messages are waiting to be mounted. */
export function needsOlder(start: number, scrollTop: number, threshold = 600): boolean {
  return start > 0 && scrollTop < threshold;
}

/** Rough rendered height (px) of a finished message, the placeholder size of content-visibility. */
export function estimateMessageHeight(contentLength: number): number {
  const lines = Math.ceil(Math.max(0, contentLength) / 80);
  return Math.min(4000, Math.max(72, 48 + lines * 24));
}

/**
 * Wraps a derived computation so it is held at its last value while `hold()` is true. The
 * computation's own dependencies are not read meanwhile, so per-token changes of the streaming
 * message do not re-run work over the whole transcript. The first call always computes.
 */
export function settledWhile<T>(hold: () => boolean, compute: () => T): () => T {
  let last: T;
  let has = false;
  return () => {
    if (has && hold()) return last;
    last = compute();
    has = true;
    return last;
  };
}

export function anchoredScroll(top: number, before: number, after: number): number {
  return top + after - before;
}

/** Tuple encoding avoids collisions between ids, round/segment slots, and arbitrary content. */
export function renderCacheKey(variantId: string, content: string): string {
  return JSON.stringify([variantId, content]);
}

/** LRU bounded by both entry count and approximate UTF-16 bytes (keys + rendered HTML).
 * Streaming callers bypass this cache, so intermediate prefixes never evict finished replies.
 */
export class RenderCache<T> {
  private entries = new Map<string, { value: T; bytes: number }>();
  private bytes = 0;
  private maxEntries: number;
  private maxBytes: number;
  constructor(maxEntries = 2048, maxBytes = 8 * 1024 * 1024) {
    this.maxEntries = maxEntries;
    this.maxBytes = maxBytes;
  }

  get(key: string): T | undefined {
    const entry = this.entries.get(key);
    if (!entry) return undefined;
    this.entries.delete(key);
    this.entries.set(key, entry);
    return entry.value;
  }

  set(key: string, value: T, renderedLength: number): void {
    const previous = this.entries.get(key);
    if (previous) this.bytes -= previous.bytes;
    this.entries.delete(key);
    const bytes = 2 * (key.length + renderedLength);
    if (bytes > this.maxBytes) return;
    this.entries.set(key, { value, bytes });
    this.bytes += bytes;
    while (this.entries.size > this.maxEntries || this.bytes > this.maxBytes) {
      const oldest = this.entries.keys().next().value!;
      this.bytes -= this.entries.get(oldest)!.bytes;
      this.entries.delete(oldest);
    }
  }
}
