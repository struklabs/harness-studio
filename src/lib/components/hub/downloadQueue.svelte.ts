import { invoke } from '$lib/ipc';
import type { DownloadProgress, DownloadStatus, HfModelVariant } from '$lib/types';
import { t } from '$lib/i18n.svelte';
import { localizeError } from '$lib/network';
import { toast } from 'svelte-sonner';
import { listen } from '@tauri-apps/api/event';
import { onWindowActive, windowActive } from '$lib/windowActivity';

export type StatusKey = 'Idle' | 'Queued' | 'Downloading' | 'Verifying' | 'Paused' | 'Completed' | 'Error';

export function statusKey(s: DownloadStatus): StatusKey {
  return typeof s === 'object' ? 'Error' : s;
}

export function statusError(s: DownloadStatus): string | null {
  return typeof s === 'object' && 'Error' in s ? localizeError(s.Error) : null;
}

export function isRunning(s: DownloadStatus): boolean {
  return s === 'Downloading' || s === 'Verifying';
}

/** While the window is hidden, running jobs are re-read this often (to notice their completion). */
const HIDDEN_REFRESH_MS = 5_000;

/** Queued or running. */
export function isPending(s: DownloadStatus): boolean {
  return s === 'Queued' || isRunning(s);
}

export function progressPercent(job: DownloadProgress): number {
  return job.total_bytes > 0 ? Math.min(100, (job.downloaded_bytes / job.total_bytes) * 100) : 0;
}

/** Remaining seconds at the current speed, or null when unknown. */
export function etaSeconds(job: DownloadProgress): number | null {
  if (job.status !== 'Downloading' || job.speed_bytes_per_sec <= 0 || job.total_bytes <= 0) return null;
  return Math.max(0, (job.total_bytes - job.downloaded_bytes) / job.speed_bytes_per_sec);
}

export function formatDuration(sec: number): string {
  const s = Math.round(sec);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(r).padStart(2, '0')}`;
  return `${m}:${String(r).padStart(2, '0')}`;
}

/**
 * Client for the backend download queue: `list_downloads` whenever the backend reports a
 * change (`downloads-changed`, coalesced; progress about twice a second), nothing while the
 * window is hidden (read once when it shows again), plus the queue actions.
 */
export class DownloadQueue {
  jobs = $state<DownloadProgress[]>([]);
  /** Last failed action (shown by the panel). */
  actionError = $state<string | null>(null);
  /** Polite screen-reader announcement of status transitions. */
  announcement = $state<string>('');

  pendingCount = $derived(this.jobs.filter((j) => isPending(j.status)).length);
  hasActivity = $derived(this.jobs.some((j) => isPending(j.status)));
  finishedCount = $derived(this.jobs.filter((j) => j.status === 'Completed').length);

  /**
   * Called when a job newly reaches Completed (not for jobs already finished at startup).
   * Without a handler a plain success toast is shown.
   */
  onCompleted: ((job: DownloadProgress) => void) | null = null;

  private running = false;
  private stale = false;
  private offs: (() => void)[] = [];
  private lastStatus = new Map<number, StatusKey>();
  private json = '';
  private seeded = false;
  private hiddenTimer: ReturnType<typeof setTimeout> | undefined;

  start() {
    if (this.running) return () => this.stop();
    this.running = true;
    listen('downloads-changed', () => {
      if (windowActive()) {
        void this.refresh();
        return;
      }
      this.stale = true;
      // Hidden: a running job is still followed, slowly, so its completion is handled (and the
      // page, kept loaded meanwhile, can be released afterwards).
      if (this.hasActivity && !this.hiddenTimer) {
        this.hiddenTimer = setTimeout(() => {
          this.hiddenTimer = undefined;
          void this.refresh();
        }, HIDDEN_REFRESH_MS);
      }
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
    return () => this.stop();
  }

  stop() {
    this.running = false;
    clearTimeout(this.hiddenTimer);
    this.hiddenTimer = undefined;
    this.offs.forEach((fn) => fn());
    this.offs = [];
  }

  /** Fetches the queue now; re-arms the fast poll when something became active. */
  async refresh() {
    let list: DownloadProgress[];
    try {
      list = await invoke<DownloadProgress[]>('list_downloads');
    } catch {
      return; // background poll; keep the last known list
    }
    const json = JSON.stringify(list);
    if (json === this.json) return;
    this.json = json;
    this.detectTransitions(list);
    this.jobs = list;
  }

  private detectTransitions(list: DownloadProgress[]) {
    const next = new Map<number, StatusKey>();
    const messages: string[] = [];
    for (const job of list) {
      const key = statusKey(job.status);
      next.set(job.id, key);
      const prev = this.lastStatus.get(job.id);
      if (!this.seeded || prev === key) continue;
      const name = job.variant_name;
      if (key === 'Completed') {
        messages.push(t('hub.announce_completed', { name }));
        if (this.onCompleted) this.onCompleted(job);
        else toast.success(t('hub.announce_completed', { name }));
      } else if (key === 'Error') {
        messages.push(t('hub.announce_error', { name }));
        toast.error(t('hub.announce_error', { name }));
      } else if (key === 'Downloading' && prev !== 'Verifying') {
        messages.push(t('hub.announce_started', { name }));
      } else if (key === 'Paused') {
        messages.push(t('hub.announce_paused', { name }));
      }
    }
    this.lastStatus = next;
    this.seeded = true;
    if (messages.length) this.announcement = messages.join(' ');
  }

  /** The newest job for a repo variant, if any. */
  jobFor(repoId: string, variantName: string): DownloadProgress | null {
    let found: DownloadProgress | null = null;
    for (const j of this.jobs) {
      if (j.repo_id === repoId && j.variant_name === variantName && (!found || j.id > found.id)) found = j;
    }
    return found;
  }

  private async act(fn: () => Promise<unknown>) {
    this.actionError = null;
    try {
      await fn();
    } catch (e) {
      this.actionError = t('hub.queue_action_failed', { error: String(e) });
    }
    await this.kickPoll();
  }

  /** Refreshes right after an action (later changes are pushed). */
  private async kickPoll() {
    await this.refresh();
  }

  /** Queues a variant. Throws the backend error (e.g. already queued). */
  async enqueueVariant(repoId: string, variant: HfModelVariant): Promise<number> {
    const id = await invoke<number>('download_hf_variant', { repoId, variant });
    this.announcement = t('hub.announce_queued', { name: variant.name });
    await this.kickPoll();
    return id;
  }

  async enqueueUrl(url: string, customFilename: string | null): Promise<number> {
    const id = await invoke<number>('download_custom_url', { url, customFilename });
    await this.kickPoll();
    return id;
  }

  cancel(jobId: number) {
    return this.act(() => invoke('cancel_active_download', { jobId }));
  }

  resume(jobId: number) {
    return this.act(() => invoke('resume_download', { jobId }));
  }

  remove(jobId: number, deletePartial: boolean) {
    return this.act(() => invoke('remove_download', { jobId, deletePartial }));
  }

  prioritize(jobId: number) {
    return this.act(() => invoke('prioritize_download', { jobId }));
  }

  clearFinished() {
    return this.act(() => invoke('clear_finished_downloads'));
  }
}
