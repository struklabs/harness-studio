// Pure decisions behind "Load and Chat" / "Load automatically when a download finishes" and the
// "Update Available" badge. No stores or IPC here so they can be unit-tested.
import type { DownloadProgress, LoadTarget, LocalModel } from './types';

/** Queued or running (same meaning as `isPending` in the download queue). */
function pending(status: DownloadProgress['status']): boolean {
  return status === 'Queued' || status === 'Downloading' || status === 'Verifying';
}

/** A finished file that a chat server can load: GGUF, an LLM (not embedding / reranker / whisper / projector / adapter), all shards present. */
export function isChatLoadable(model: LocalModel): boolean {
  if (!model.is_ready) return false;
  if (model.kind !== 'llm') return false;
  if (model.gguf?.is_mmproj) return false;
  if (!/\.gguf$/i.test(model.filename)) return false;
  if (model.is_split && (model.present_shards ?? 0) < (model.total_shards ?? 0)) return false;
  return true;
}

/**
 * Whether a download that just finished is loaded automatically: the setting is on, the file is a
 * chat-capable GGUF, and no other job of the same repo is still queued or running (a repo's
 * projector or second quant may still be on its way).
 */
export function shouldAutoLoad(
  enabled: boolean,
  job: Pick<DownloadProgress, 'id' | 'repo_id'>,
  model: LocalModel | undefined,
  jobs: readonly Pick<DownloadProgress, 'id' | 'repo_id' | 'status'>[],
): boolean {
  if (!enabled || !model || !isChatLoadable(model)) return false;
  return !repoJobsPending(job, jobs);
}

/** Another job of the same repo is still queued or running: its projector or a second quant. */
export function repoJobsPending(
  job: Pick<DownloadProgress, 'id' | 'repo_id'>,
  jobs: readonly Pick<DownloadProgress, 'id' | 'repo_id' | 'status'>[],
): boolean {
  return jobs.some((j) => j.id !== job.id && j.repo_id === job.repo_id && pending(j.status));
}

/**
 * Where "Load and Chat" and the automatic load run a model: the selected server, like the
 * Playground model picker's "load here". A new server instead when the selection is a saved
 * profile (it runs its own definition only) or is busy/running (loading here would stop what the
 * user is using).
 */
export function defaultLoadTarget(
  selectedServerId: string,
  defaultServerId: string,
  profileIds: readonly string[],
  occupied: boolean,
): LoadTarget {
  const isProfile = selectedServerId !== defaultServerId && profileIds.includes(selectedServerId);
  return isProfile || occupied ? { kind: 'new_server' } : { kind: 'server', server_id: selectedServerId };
}

/**
 * "Update Available" on a repo card. `updates` holds the repos whose downloaded variants the
 * drawer already compared against Hugging Face (LFS oid); it is not fetched per card. The badge
 * needs the repo to be downloaded locally, so a stale flag of a deleted model never shows.
 */
export function showUpdateBadge(
  updates: Readonly<Record<string, boolean>>,
  repoId: string | null | undefined,
  hasLocalFiles: boolean,
): boolean {
  return !!repoId && hasLocalFiles && updates[repoId] === true;
}
