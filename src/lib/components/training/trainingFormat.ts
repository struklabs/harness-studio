import type { TrainingJob, TrainingStatus } from '#lib/training-types.js';

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

export function fmtLoss(v: number | null | undefined): string {
  return v === null || v === undefined ? '-' : v.toFixed(3);
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
