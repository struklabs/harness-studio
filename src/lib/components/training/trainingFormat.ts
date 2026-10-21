import type { TrainingJob, TrainingStatus } from '#lib/training-types.js';

/** Planning estimate only: tensor mix, tokenizer and small-model overhead vary. */
export function estimateGgufBytes(params: number, quant: import('#lib/training-types.js').TrainingQuant): number {
  const bits = { F16: 16, Q8_0: 8.5, Q4_K_M: 4.9, Q5_K_M: 5.7 }[quant];
  return Math.ceil(params * bits / 8 + 4 * 1024 * 1024);
}

export const STATUS_KEY = {
  draft: 'training.status_draft',
  queued: 'training.status_queued',
  running: 'training.status_running',
  done: 'training.status_done',
  failed: 'training.status_failed',
  cancelled: 'training.status_cancelled',
} as const satisfies Record<TrainingStatus, string>;

export const STATUS_CLASS: Record<TrainingStatus, string> = {
  draft: 'bg-muted text-muted-foreground',
  queued: 'bg-muted text-foreground',
  running: 'bg-primary/15 text-primary',
  done: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
  failed: 'bg-destructive/15 text-destructive',
  cancelled: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
};

export function fmtDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.round(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
  return `${m}:${String(s % 60).padStart(2, '0')}`;
}

/** Seconds a job has been (or was) running; null before it started. */
export function elapsedSeconds(job: TrainingJob, nowSeconds: number): number | null {
  if (job.started_at === null) return null;
  return (job.finished_at ?? nowSeconds) - job.started_at;
}

/** Backend params with decimal numbers (`max_params`: "0.5") written in `locale` ("0,5"). */
export function localParams(params: Record<string, string> | undefined, locale?: string): Record<string, string> {
  const out = { ...(params ?? {}) };
  const n = Number(out.max_params);
  if (out.max_params !== undefined && Number.isFinite(n)) out.max_params = n.toLocaleString(locale, { maximumFractionDigits: 1, useGrouping: false });
  return out;
}

/** A loss value with three decimals in `locale` ("2,453" in Turkish). */
export function fmtLoss(v: number | null | undefined, locale?: string): string {
  return v === null || v === undefined ? '-' : v.toLocaleString(locale, { minimumFractionDigits: 3, maximumFractionDigits: 3, useGrouping: false });
}

export function fmtInt(n: number): string {
  return n.toLocaleString();
}

export function canExport(job: TrainingJob): boolean {
  const idle = job.export?.status !== 'running';
  return idle && (job.status === 'done' || ((job.status === 'failed' || job.status === 'cancelled') && job.checkpoint_step !== null));
}

/** Last path segment of a Windows or POSIX path. */
export function baseName(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? path;
}

export interface LiveStats {
  stepsPerSecond: number;
  tokensPerSecond: number;
  etaSeconds: number;
}

/**
 * Throughput and time left of a running job, from this run's training time (model loading
 * left out when the job reports when it began). `null` until a few steps have run.
 */
export function liveStats(job: TrainingJob, nowSeconds: number): LiveStats | null {
  if (job.status !== 'running' || !job.training || job.step < 3) return null;
  // The rate counts training time only: model load and earlier runs of a resumed job stay out.
  const since = job.training_since ?? null;
  const elapsed = since !== null ? nowSeconds - since : elapsedSeconds(job, nowSeconds);
  const steps = since !== null ? job.step - (job.training_from_step ?? 0) : job.step;
  if (elapsed === null || elapsed <= 0 || steps <= 0) return null;
  const stepsPerSecond = steps / elapsed;
  return {
    stepsPerSecond,
    tokensPerSecond: stepsPerSecond * job.estimate.tokens_per_step,
    etaSeconds: Math.max(0, (job.total_steps - job.step) / stepsPerSecond),
  };
}
