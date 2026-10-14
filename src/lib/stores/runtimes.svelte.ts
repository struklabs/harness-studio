import { invoke } from '#lib/ipc.js';
import type { RuntimeView } from '#lib/types.js';

/**
 * Runtime registry (Settings > Runtime, Dashboard runtime selector): the official runtime
 * first, then the registered forks / local builds, each with its capability summary.
 */
class RuntimesState {
  list = $state<RuntimeView[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);
  private loaded = false;
  private inflight: Promise<void> | null = null;

  /** Loads once; `force` re-reads (after add/verify/remove). */
  load(force = false): Promise<void> {
    if (this.loaded && !force) return Promise.resolve();
    if (this.inflight && !force) return this.inflight;
    this.loading = true;
    const p = invoke<RuntimeView[]>('list_runtimes')
      .then((list) => {
        this.list = list;
        this.error = null;
        this.loaded = true;
      })
      .catch((e) => {
        this.error = String(e);
      })
      .finally(() => {
        this.loading = false;
        if (this.inflight === p) this.inflight = null;
      });
    this.inflight = p;
    return p;
  }

  get(id: string | null | undefined): RuntimeView | undefined {
    const key = id && id !== '' ? id : 'official';
    return this.list.find((r) => r.id === key);
  }

  /** There is more than the official runtime to choose from. */
  get hasExtra(): boolean {
    return this.list.some((r) => r.id !== 'official');
  }
}

export const runtimesStore = new RuntimesState();

/** `b11236 · vulkan` style label of a runtime's build. */
export function runtimeBuildLabel(r: RuntimeView | undefined): string {
  if (!r) return '';
  const v = r.version;
  const build = v?.build != null ? `b${v.build}` : (v?.version ?? r.entry?.version ?? '');
  return [build, r.backend].filter(Boolean).join(' · ');
}
