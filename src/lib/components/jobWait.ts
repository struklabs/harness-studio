// Why a model's start is still queued (the job scheduler's `wait`), for the Jobs list and the
// studios' progress line.

export type WaitReason = 'queue' | 'memory' | 'alone';

export interface JobWait {
  reason: WaitReason;
  /** Memory free for it at the last check, after the reserve and loads in progress. */
  free_bytes: number;
  /** The budget for managed engines, after the reserve. */
  capacity_bytes: number;
}

export interface Job {
  id: number;
  label: string;
  priority: 'interactive' | 'agent' | 'background';
  status: 'queued' | 'loading' | 'running' | 'cancelling';
  memory_bytes: number | null;
  queue_wait_ms: number;
  load_ms: number | null;
  /** Absent on servers older than this field, and once the job has started. */
  wait?: JobWait | null;
}

/** The server id part a model alias gives (`orchestrator::server_id_for`): lower case, every
 *  other character a dash, at most 48 characters. */
export function aliasSlug(alias: string): string {
  const slug = alias.toLowerCase().replace(/[^a-z0-9]/g, '-').replace(/^-+|-+$/g, '').slice(0, 48);
  return slug || 'model';
}

/** The queued job that starts `alias`'s server (`m-<slug>`, `gw-<slug>`, or either with `-N`). */
export function queuedJobFor(jobs: Job[], alias: string): Job | undefined {
  const slug = aliasSlug(alias);
  return jobs.find(
    (job) =>
      job.status === 'queued' &&
      ['m-', 'gw-'].some((prefix) => job.label === prefix + slug || new RegExp(`^${prefix}${slug}-\\d+$`).test(job.label)),
  );
}

/** The locale key and byte values that say why `job` waits; null when it is not waiting. */
export function waitLine(job: Job): { key: 'jobs.wait_queue' | 'jobs.wait_memory' | 'jobs.wait_alone'; need: number | null; free: number; capacity: number } | null {
  if (job.status !== 'queued' || !job.wait) return null;
  const key = job.wait.reason === 'memory' ? 'jobs.wait_memory' : job.wait.reason === 'alone' ? 'jobs.wait_alone' : 'jobs.wait_queue';
  return { key, need: job.memory_bytes, free: job.wait.free_bytes, capacity: job.wait.capacity_bytes };
}
