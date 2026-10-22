// Coding area: project-scoped code work with a model through the Gateway (core `coding`).
import { invoke } from '#lib/ipc.js';

export type Backend = 'git' | 'snapshot';
export type ChangeStatus = 'added' | 'modified' | 'deleted';
export type CallStatus = 'running' | 'done' | 'error' | 'denied';
export type Decision = 'run' | 'ask' | 'refuse';

export interface Checkpoint {
  id: string;
  project_id: string;
  folder: string;
  backend: Backend;
  label: string;
  created_at: number;
  files: number;
  undone: boolean;
}

export interface FileChange {
  path: string;
  status: ChangeStatus;
  additions: number;
  deletions: number;
  binary: boolean;
  diff: string;
}

export interface ToolPermission {
  name: string;
  class: 'safe' | 'restricted' | 'dangerous';
  decision: Decision;
}

export interface WorkspacePermissions {
  folder: string | null;
  extra_folders: string[];
  folder_missing: boolean;
  backend: Backend;
  policy: 'off' | 'ask_every_time' | 'allow_trusted' | 'allow_all';
  tools: ToolPermission[];
  protected: string[];
}

export type CodingEvent =
  | { type: 'started'; run_id: string; checkpoint: Checkpoint }
  | { type: 'round'; run_id: string; round: number }
  | { type: 'call'; run_id: string; call_id: string; tool: string; summary: string; status: CallStatus }
  | { type: 'approval'; run_id: string; approval_id: string; tool: string; summary: string; warnings: string[] }
  | { type: 'finished'; run_id: string; stop: string; text: string; error: string | null; changes: number };

export interface Applied {
  checkpoint: Checkpoint;
  files: string[];
}

export const codingApi = {
  permissions: (projectId: string) => invoke<WorkspacePermissions>('coding_permissions', { projectId }),
  run: (projectId: string, baseUrl: string, apiKey: string | null, model: string, prompt: string) =>
    invoke<string>('coding_run', { input: { project_id: projectId, base_url: baseUrl, api_key: apiKey, model, prompt } }),
  cancel: (runId: string) => invoke<boolean>('coding_cancel', { runId }),
  approve: (approvalId: string, allow: boolean) => invoke<boolean>('coding_approve', { approvalId, allow }),
  checkpoints: (projectId: string) => invoke<Checkpoint[]>('coding_checkpoints', { projectId }),
  changes: (projectId: string, checkpointId: string) => invoke<FileChange[]>('coding_changes', { projectId, checkpointId }),
  undo: (projectId: string, checkpointId: string, files: string[] | null) =>
    invoke<string[]>('coding_undo', { projectId, checkpointId, files }),
  createCheckpoint: (projectId: string, label: string) => invoke<Checkpoint>('coding_checkpoint_create', { projectId, label }),
  deleteCheckpoint: (projectId: string, checkpointId: string) => invoke<void>('coding_checkpoint_delete', { projectId, checkpointId }),
  applyPatch: (projectId: string, patch: string) => invoke<Applied>('coding_apply_patch', { projectId, patch }),
};

export type DiffLineKind = 'add' | 'del' | 'ctx' | 'hunk' | 'meta';

export interface DiffLine {
  kind: DiffLineKind;
  text: string;
}

/** A unified diff as lines to render: headers, hunk markers, added, removed and context. */
export function diffLines(diff: string): DiffLine[] {
  const out: DiffLine[] = [];
  let inHunk = false;
  for (const line of diff.split('\n')) {
    if (line === '' && out.length) continue;
    if (line.startsWith('@@')) { inHunk = true; out.push({ kind: 'hunk', text: line }); }
    else if (!inHunk) out.push({ kind: 'meta', text: line });
    else if (line.startsWith('\\')) out.push({ kind: 'meta', text: line });
    else if (line.startsWith('+')) out.push({ kind: 'add', text: line });
    else if (line.startsWith('-')) out.push({ kind: 'del', text: line });
    else out.push({ kind: 'ctx', text: line });
  }
  return out;
}

export function totals(changes: FileChange[]): { files: number; additions: number; deletions: number } {
  return changes.reduce(
    (t, c) => ({ files: t.files + 1, additions: t.additions + c.additions, deletions: t.deletions + c.deletions }),
    { files: 0, additions: 0, deletions: 0 },
  );
}

/** Tasks the quick-start buttons put in the prompt box (locale keys). */
export const starters = [
  { id: 'write', prompt: 'coding.starter_write_prompt', label: 'coding.starter_write' },
  { id: 'fix', prompt: 'coding.starter_fix_prompt', label: 'coding.starter_fix' },
  { id: 'refactor', prompt: 'coding.starter_refactor_prompt', label: 'coding.starter_refactor' },
  { id: 'test', prompt: 'coding.starter_test_prompt', label: 'coding.starter_test' },
] as const;

/** The locale key of a command warning mark from the backend (`delete`, `discard`, ...). */
export function warningKey(mark: string): string {
  return ({
    delete: 'coding.warn_delete',
    discard: 'coding.warn_discard',
    install: 'coding.warn_install',
    network: 'coding.warn_network',
    push: 'coding.warn_push',
  } as Record<string, string>)[mark] ?? 'coding.warn_unknown';
}

/** Applies one run event to the progress list; a call's status updates its row in place. */
export interface Step {
  key: string;
  tool: string;
  summary: string;
  status: CallStatus;
}

export function applyEvent(steps: Step[], event: CodingEvent): Step[] {
  if (event.type !== 'call') return steps;
  const key = event.call_id || `denied-${steps.length}`;
  const at = steps.findIndex((s) => s.key === key);
  const step: Step = { key, tool: event.tool, summary: event.summary, status: event.status };
  if (at < 0) return [...steps, step];
  const next = steps.slice();
  next[at] = step;
  return next;
}
