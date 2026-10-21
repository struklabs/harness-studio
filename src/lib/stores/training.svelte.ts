import { listen } from '@tauri-apps/api/event';
import { invoke } from '#lib/ipc.js';
import { localizeError } from '#lib/network.js';
import { onWindowActive, windowActive } from '#lib/windowActivity.js';
import type {
  TrainingCompatibility,
  TrainingCreateRequest,
  TrainingEnvStatus,
  TrainingJob,
  TrainingMetric,
} from '#lib/training-types.js';
import { library } from '#lib/stores/library.svelte.js';

export const isTrainingActive = (j: TrainingJob) => j.status === 'queued' || j.status === 'running';

/** How often the loss series of a running job is re-read. */
const METRICS_POLL_MS = 1500;

/**
 * App-wide training jobs (the Training page): the list the backend keeps, refreshed when it
 * pushes `training-changed`, plus the loss series of the selected job (polled while it runs).
 */
export class TrainingStore {
  jobs = $state<TrainingJob[]>([]);
  env = $state<TrainingEnvStatus | null>(null);
  installing = $state(false);
  installError = $state<string | null>(null);
  compat = $state<TrainingCompatibility | null>(null);
  selectedId = $state<number | null>(null);
  metrics = $state<TrainingMetric[]>([]);
  actionError = $state<string | null>(null);

  selected = $derived(this.jobs.find((j) => j.id === this.selectedId) ?? null);
  activeCount = $derived(this.jobs.filter(isTrainingActive).length);
  envReady = $derived(this.env?.ready === true);

  private offs: (() => void)[] = [];
  private started = false;
  private stale = false;
  private json = '';
  private timer: ReturnType<typeof setInterval> | null = null;
  private metricsKey = '';

  start() {
    if (this.started) return;
    this.started = true;
    listen('training-changed', () => {
      if (windowActive()) void this.refresh();
      else this.stale = true;
    })
      .then((fn) => (this.started ? this.offs.push(fn) : fn()))
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
    let list: TrainingJob[];
    try {
      list = await invoke<TrainingJob[]>('training_list');
    } catch {
      return;
    }
    const json = JSON.stringify(list);
    if (json === this.json) return;
    if (list.some((job) => job.export?.dir && job.export.status !== 'running'
      && JSON.stringify(this.jobs.find((old) => old.id === job.id)?.export) !== JSON.stringify(job.export))) {
      library.markChanged();
      void library.refresh();
    }
    this.json = json;
    this.jobs = list;
    if (this.selectedId !== null && !list.some((j) => j.id === this.selectedId)) this.select(null);
  }

  async refreshEnv() {
    try {
      this.env = await invoke<TrainingEnvStatus>('training_env');
    } catch (e) {
      this.installError = localizeError(e);
    }
  }

  async loadCompat() {
    try {
      this.compat = await invoke<TrainingCompatibility>('training_compatibility');
    } catch {
      // the card stays empty
    }
  }

  /** Builds the environment; progress shows through `refreshEnv` polling while it runs. */
  async installEnv() {
    if (this.installing) return;
    this.installing = true;
    this.installError = null;
    const poll = setInterval(() => void this.refreshEnv(), 1000);
    try {
      this.env = await invoke<TrainingEnvStatus>('training_install_env');
      void this.loadCompat();
    } catch (e) {
      this.installError = localizeError(e);
      await this.refreshEnv();
    } finally {
      clearInterval(poll);
      this.installing = false;
    }
  }

  select(id: number | null) {
    this.selectedId = id;
    this.metrics = [];
    this.metricsKey = '';
    void this.loadMetrics();
  }

  async loadMetrics() {
    const id = this.selectedId;
    if (id === null) return;
    try {
      const list = await invoke<TrainingMetric[]>('training_metrics', { jobId: id });
      if (id !== this.selectedId) return;
      const last = list[list.length - 1];
      const key = `${list.length}:${last?.step ?? ''}:${last?.loss ?? ''}:${last?.eval_loss ?? ''}`;
      if (key === this.metricsKey) return;
      this.metricsKey = key;
      this.metrics = list;
    } catch {
      // the series is not there yet
    }
  }

  readLog(jobId: number): Promise<string> {
    return invoke<string>('training_log', { jobId });
  }

  /** Polls the selected job's metrics while the page is open and the job is running. */
  startPolling() {
    if (this.timer) return;
    this.timer = setInterval(() => {
      const job = this.selected;
      if (job && isTrainingActive(job) && windowActive()) void this.loadMetrics();
    }, METRICS_POLL_MS);
  }

  stopPolling() {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  private async act<T>(fn: () => Promise<T>): Promise<T | undefined> {
    this.actionError = null;
    try {
      return await fn();
    } catch (e) {
      this.actionError = localizeError(e);
      return undefined;
    } finally {
      await this.refresh();
    }
  }

  /** Throws the backend error (invalid parameter, device unavailable...): the wizard shows it. */
  async create(request: TrainingCreateRequest): Promise<TrainingJob> {
    const job = await invoke<TrainingJob>('training_create', { request });
    await this.refresh();
    return job;
  }

  startJob(jobId: number) {
    return this.act(() => invoke<TrainingJob>('training_start', { jobId }));
  }
  resume(jobId: number) {
    return this.act(() => invoke<TrainingJob>('training_resume', { jobId }));
  }
  cancel(jobId: number) {
    return this.act(() => invoke('training_cancel', { jobId }));
  }
  remove(jobId: number) {
    return this.act(() => invoke('training_remove', { jobId }));
  }
  exportJob(jobId: number, request: import('#lib/training-types.js').TrainingExportRequest) {
    return this.act(() => invoke<TrainingJob>('training_export', { jobId, request }));
  }
}

export const training = new TrainingStore();

if (typeof window !== 'undefined') training.start();
