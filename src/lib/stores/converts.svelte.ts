import { toast } from 'svelte-sonner';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '#lib/ipc.js';
import { t } from '#lib/i18n.svelte.js';
import { localizeError } from '#lib/network.js';
import { ACTION_TOAST_DURATION } from '#lib/toast.js';
import { app } from '#lib/stores/app.svelte.js';
import { library } from '#lib/stores/library.svelte.js';
import { samePath } from '#lib/stores/domain.svelte.js';
import { ensureDefinition } from '#lib/stores/downloads.svelte.js';
import { holdReleaseWhile, onWindowActive, windowActive } from '#lib/windowActivity.js';
import type { ConvertJob } from '#lib/types.js';

export const isConvertActive = (j: ConvertJob) => j.status === 'queued' || j.status === 'running';

/**
 * App-wide safetensors to GGUF conversions (the Model Hub's "Convert to GGUF"): the jobs the
 * backend runs, refreshed when it pushes `converts-changed`. Lives outside the Hub component so a
 * finished conversion is followed up (library rescan, model definition, toast) on any tab.
 */
export class ConvertQueue {
  jobs = $state<ConvertJob[]>([]);
  actionError = $state<string | null>(null);

  activeCount = $derived(this.jobs.filter(isConvertActive).length);
  hasActivity = $derived(this.activeCount > 0);

  private offs: (() => void)[] = [];
  private running = false;
  private stale = false;
  private json = '';
  private statuses = new Map<number, string>();
  private seeded = false;

  start() {
    if (this.running) return;
    this.running = true;
    listen('converts-changed', () => {
      if (windowActive()) void this.refresh();
      else this.stale = true;
    })
      .then((fn) => (this.running ? this.offs.push(fn) : fn()))
      .catch(() => {});
    this.offs.push(
      onWindowActive(() => {
        if (!this.stale) return;
        this.stale = false;
        void this.refresh();
      }),
    );
    void this.refresh();
  }

  async refresh() {
    let list: ConvertJob[];
    try {
      list = await invoke<ConvertJob[]>('list_convert_jobs');
    } catch {
      return;
    }
    const json = JSON.stringify(list);
    if (json === this.json) return;
    this.json = json;
    this.followUp(list);
    this.jobs = list;
  }

  /** Toasts and the library follow-up for jobs that just ended. */
  private followUp(list: ConvertJob[]) {
    const next = new Map<number, string>();
    for (const job of list) {
      next.set(job.id, job.status);
      const prev = this.statuses.get(job.id);
      if (!this.seeded || prev === job.status) continue;
      if (job.status === 'completed') void this.completed(job);
      else if (job.status === 'failed') toast.error(`${t('cvt.failed_toast', { repo: job.repo_id })}${job.error ? `: ${localizeError(job.error)}` : ''}`);
    }
    this.statuses = next;
    this.seeded = true;
  }

  private async completed(job: ConvertJob) {
    library.markChanged();
    const list = await library.refresh();
    const message = t('cvt.done_toast', { repo: job.repo_id });
    const model = job.output ? list.find((m) => samePath(m.path, job.output as string)) : undefined;
    if (!model) {
      toast.success(message);
      return;
    }
    // The Playground picker and the gateway list definitions, not files: make one right away.
    try {
      await ensureDefinition(model);
    } catch (e) {
      console.warn('definition for converted model', e);
    }
    toast.success(message, {
      duration: ACTION_TOAST_DURATION,
      closeButton: true,
      action: {
        label: t('hub.open_in_dashboard'),
        onClick: () => {
          library.pendingSelectId = model.id;
          app.openTab('dashboard');
        },
      },
    });
  }

  /** Queues a conversion. Throws the backend error (not convertible, not enough disk...). */
  async enqueue(repoId: string, quant: string, keepOriginal: boolean): Promise<ConvertJob> {
    const job = await invoke<ConvertJob>('convert_start', { repoId, quant, keepOriginal });
    await this.refresh();
    return job;
  }

  private async act(fn: () => Promise<unknown>) {
    this.actionError = null;
    try {
      await fn();
    } catch (e) {
      this.actionError = localizeError(e);
    }
    await this.refresh();
  }

  cancel(jobId: number) {
    return this.act(() => invoke('cancel_convert_job', { jobId }));
  }

  remove(jobId: number) {
    return this.act(() => invoke('remove_convert_job', { jobId }));
  }

  clearFinished() {
    return this.act(() => invoke('clear_finished_conversions'));
  }

  /** The newest job of a repo, if any. */
  jobFor(repoId: string): ConvertJob | null {
    let found: ConvertJob | null = null;
    for (const j of this.jobs) {
      if (j.repo_id === repoId && (!found || j.id > found.id)) found = j;
    }
    return found;
  }
}

export const converts = new ConvertQueue();

if (typeof window !== 'undefined') {
  converts.start();
  // A conversion finishing is followed up here (rescan, definition): keep the page loaded while one runs.
  holdReleaseWhile('converts', () => converts.hasActivity);
}
