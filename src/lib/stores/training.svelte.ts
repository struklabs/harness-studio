import { toast } from 'svelte-sonner';
import { t } from '#lib/i18n.svelte.js';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '#lib/ipc.js';
import { localizeError } from '#lib/network.js';
import { onWindowActive, windowActive } from '#lib/windowActivity.js';
import type {
  TrainingCompatibility,
  TrainingCreateRequest,
  TrainingDevice,
  TrainingEnvStatus,
  TrainingJob,
  TrainingMetric,
} from '#lib/training-types.js';
import type { PythonEnvStatus } from '#lib/types.js';
import { library } from '#lib/stores/library.svelte.js';
import { engineInstall } from '#lib/stores/engineInstall.svelte.js';

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
  engineProgress = $state<PythonEnvStatus['progress']>(null);
  compat = $state<TrainingCompatibility | null>(null);
  selectedId = $state<number | null>(null);
  metrics = $state<TrainingMetric[]>([]);
  actionError = $state<string | null>(null);
  compareJobId = $state<number | null>(null);

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
    if (list.some(j => j.installing_engine)) void this.refreshEngineProgress();
    else this.engineProgress = null;
    const json = JSON.stringify(list);
    if (json === this.json) return;
    if (list.some((job) => job.export?.dir && job.export.status !== 'running'
      && JSON.stringify(this.jobs.find((old) => old.id === job.id)?.export) !== JSON.stringify(job.export))) {
      library.markChanged();
      void library.refresh();
    }
    for (const job of list) {
      const old = this.jobs.find(j => j.id === job.id);
      if (old?.export?.status === 'running' && job.export?.status === 'done') toast.success(t('training.export_done_toast'));
    }
    this.json = json;
    this.jobs = list;
    if (this.selectedId !== null && !list.some((j) => j.id === this.selectedId)) this.select(null);
  }

  private async refreshEngineProgress() {
    try {
      const envs = await invoke<PythonEnvStatus[]>('list_python_envs');
      if (this.jobs.some(j => j.installing_engine)) this.engineProgress = envs.find(e => e.name === 'mlx')?.progress ?? null;
    } catch {
      // The install label remains visible if progress is unavailable.
    }
  }

  async refreshEnv() {
    try {
      this.env = await invoke<TrainingEnvStatus>('training_env');
    } catch (e) {
      this.installError = localizeError(e);
    }
  }

  private compatRetry: ReturnType<typeof setTimeout> | undefined;
  /**
   * The table answers at once (inferred from the hardware while the environment's probe runs in
   * the background); an unconfirmed one is asked again until the probe has answered.
   */
  async loadCompat(tries = 40) {
    clearTimeout(this.compatRetry);
    try {
      this.compat = await invoke<TrainingCompatibility>('training_compatibility');
    } catch {
      // the card stays empty
    }
    if (this.compat && !this.compat.probed && this.envReady && tries > 1) {
      this.compatRetry = setTimeout(() => void this.loadCompat(tries - 1), 15_000);
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
      const key = JSON.stringify(list);
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

  /**
   * Installs or updates the training environment when it is missing or out of date, without
   * asking: starting a job is the consent. Resolves to whether it is ready afterwards. An MLX
   * job needs none of it: the backend installs the MLX engine itself.
   */
  async ensureEnv(device?: TrainingDevice): Promise<boolean> {
    if (device === 'mlx' || this.envReady) return true;
    await this.installEnv();
    return this.envReady;
  }

  private deviceOf(jobId: number): TrainingDevice | undefined {
    return this.jobs.find((j) => j.id === jobId)?.config.device;
  }

  startJob(jobId: number) {
    return this.act(async () => {
      if (!(await this.ensureEnv(this.deviceOf(jobId)))) throw new Error(this.installError ?? 'training.env_required');
      return invoke<TrainingJob>('training_start', { jobId });
    });
  }
  resume(jobId: number) {
    return this.act(async () => {
      if (!(await this.ensureEnv(this.deviceOf(jobId)))) throw new Error(this.installError ?? 'training.env_required');
      return invoke<TrainingJob>('training_resume', { jobId });
    });
  }
  /** Starts a failed or cancelled job that has no checkpoint over from step 0. */
  restart(jobId: number) {
    return this.act(async () => {
      if (!(await this.ensureEnv(this.deviceOf(jobId)))) throw new Error(this.installError ?? 'training.env_required');
      return invoke<TrainingJob>('training_restart', { jobId });
    });
  }
  cancel(jobId: number) {
    return this.act(() => invoke('training_cancel', { jobId }));
  }
  pause(jobId: number) {
    return this.act(() => invoke('training_pause', { jobId }));
  }
  remove(jobId: number) {
    return this.act(() => invoke('training_remove', { jobId }));
  }
  exportJob(jobId: number, request: import('#lib/training-types.js').TrainingExportRequest) {
    return this.act(async () => {
      if (!(await this.ensureEnv(this.deviceOf(jobId)))) throw new Error(this.installError ?? 'training.env_required');
      return engineInstall.runWithEngine(() => invoke<TrainingJob>('training_export', { jobId, request }));
    });
  }
}

export const training = new TrainingStore();

if (typeof window !== 'undefined') training.start();
