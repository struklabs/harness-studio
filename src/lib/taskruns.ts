// Task runs of a project (core `taskflow`): agent tasks and analysis sessions that can pause,
// step back and resume, here or on another device.
import { invoke } from '#lib/ipc.js';

export type RunKind = 'agent_task' | 'analysis';
export type RunStatus = 'draft' | 'running' | 'paused' | 'awaiting_approval' | 'completed' | 'failed' | 'interrupted';
export type StepStatus = 'pending' | 'running' | 'done' | 'skipped' | 'failed';

export interface RunPolicy {
  models: string[];
  min_context: number | null;
  require_tools: boolean;
  max_retries: number;
  backoff_ms: number;
  timeout_secs: number;
  max_tokens: number;
  temperature: number;
  price: { input_per_m: number; output_per_m: number } | null;
}

export interface FileRef { path: string; size: number; sha256: string }

export interface TaskRun {
  id: string;
  project_id: string | null;
  kind: RunKind;
  title: string;
  goal: string;
  status: RunStatus;
  mode: 'auto' | 'step';
  policy: RunPolicy;
  file_refs: FileRef[];
  tokens_in: number;
  tokens_out: number;
  cost_micro: number;
  last_model: string | null;
  error: string | null;
  imported_from: string | null;
  created_at: number;
  updated_at: number;
}

export interface TaskRunRow extends TaskRun { steps_total: number; steps_done: number; active: boolean }

export interface TaskStep {
  idx: number;
  title: string;
  prompt: string;
  gate: boolean;
  approval: 'none' | 'pending' | 'granted' | 'denied';
  status: StepStatus;
  output: string;
  model: string | null;
  fallback_reason: string | null;
  attempts: number;
  tokens_in: number;
  tokens_out: number;
  cost_micro: number;
  error: string | null;
}

export interface TaskEvent { step_idx: number | null; at: number; kind: string; message: string }
export interface TaskCheckpoint { id: string; step_idx: number; label: string; model: string | null; created_at: number }
export interface FileRefState { path: string; state: 'present' | 'changed' | 'missing' }

export interface TaskRunDetail {
  run: TaskRun;
  steps: TaskStep[];
  checkpoints: TaskCheckpoint[];
  events: TaskEvent[];
  files: FileRefState[];
  active: boolean;
}

export interface ModelInfo { model: string; context: number | null; tools: boolean | null; provider: string }
export interface ModelCheck { model: string; ok: boolean; reason: string | null; info: ModelInfo | null }
export interface CompatNote { level: 'info' | 'warn'; message: string }

export interface Endpoint { base_url: string; headers: Record<string, string>; model: string | null }

export interface TaskRunDraft {
  project_id: string | null;
  kind: RunKind;
  title: string;
  goal: string;
  mode: 'auto' | 'step';
  policy: RunPolicy;
  files: string[];
  steps: { title: string; prompt: string; gate: boolean }[];
}

export const taskRunApi = {
  list: (projectId: string | null) => invoke<TaskRunRow[]>('taskrun_list', { projectId }),
  get: (id: string) => invoke<TaskRunDetail>('taskrun_get', { id }),
  create: (draft: TaskRunDraft) => invoke<TaskRun>('taskrun_create', { draft }),
  update: (id: string, draft: TaskRunDraft) => invoke<TaskRun>('taskrun_update', { id, draft }),
  remove: (id: string) => invoke<void>('taskrun_delete', { id }),
  start: (id: string, endpoint: Endpoint, step: boolean) => invoke<void>('taskrun_start', { id, endpoint, step }),
  stop: (id: string) => invoke<boolean>('taskrun_stop', { id }),
  rewind: (id: string, idx: number) => invoke<TaskRun>('taskrun_rewind', { id, idx }),
  approve: (id: string, idx: number, grant: boolean) => invoke<TaskRun>('taskrun_approve', { id, idx, grant }),
  skip: (id: string, idx: number) => invoke<void>('taskrun_skip', { id, idx }),
  preflight: (id: string, endpoint: Endpoint) => invoke<ModelCheck[]>('taskrun_preflight', { id, endpoint }),
  compat: (id: string, endpoint: Endpoint, model: string | null) => invoke<CompatNote[]>('taskrun_compat', { id, endpoint, model }),
  exportBundle: (id: string, embedFiles: boolean) =>
    invoke<{ path: string; steps: number; embedded_files: number } | null>('taskrun_export', { id, embedFiles }),
  importBundle: (projectId: string | null) =>
    invoke<{ run: TaskRun; files: FileRefState[]; restored_files: number } | null>('taskrun_import', { projectId }),
};

export function defaultPolicy(): RunPolicy {
  return { models: [], min_context: null, require_tools: false, max_retries: 2, backoff_ms: 500, timeout_secs: 180, max_tokens: 768, temperature: 0.2, price: null };
}

export function emptyDraft(projectId: string | null, kind: RunKind): TaskRunDraft {
  return { project_id: projectId, kind, title: '', goal: '', mode: 'auto', policy: defaultPolicy(), files: [], steps: [] };
}

/** One step per line; a leading `! ` marks an approval gate. Blank lines are dropped. */
export function parseSteps(text: string): TaskRunDraft['steps'] {
  return text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean).map((line) => {
    const gate = line.startsWith('! ');
    const prompt = gate ? line.slice(2).trim() : line;
    // The list numbers the steps itself; a number in the title showed twice ("1. 1. ...").
    return { title: prompt.slice(0, 40), prompt, gate };
  });
}

export function stepsText(steps: { prompt: string; gate: boolean }[]): string {
  return steps.map((s) => (s.gate ? '! ' : '') + s.prompt).join('\n');
}

export function lines(text: string): string[] {
  return text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
}

/** Micro-USD as a short dollar amount. */
export function formatCost(micro: number): string {
  if (micro <= 0) return '';
  const usd = micro / 1_000_000;
  return usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(2)}`;
}

export const RESUMABLE: RunStatus[] = ['draft', 'paused', 'interrupted', 'failed', 'awaiting_approval'];
