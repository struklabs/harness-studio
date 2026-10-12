// How far the installed runtime runs each Hugging Face task, from the backend
// (`get_task_support`: the task baseline lowered by the runtime's capability registry).
import { invoke } from '$lib/ipc';
import type { TaskSupport } from './tasks';

/** `harness_core::hub_tasks::TaskSupport`. */
export interface RuntimeTaskSupport {
  task: string;
  support: TaskSupport;
  /** Support without looking at the runtime. */
  baseline: TaskSupport;
  /** Capability-registry feature the task depends on (`embeddings`, `rerank`, `vision`). */
  feature: string | null;
  /** The runtime's `--help` was read. */
  runtime_checked: boolean;
  /** The runtime lacks what the task needs. */
  runtime_missing: boolean;
  /** A real run of this runtime answered the endpoint the task uses. */
  verified: boolean;
  /** Flag spelling found in the runtime (`--embeddings`). */
  flag: string | null;
  /** Another installed runtime family serves the task (`support` counts it). */
  runtime: string | null;
  /** The task runs on a runtime family that is not installed yet (`support` does not count it). */
  needs_runtime: string | null;
}

class TaskSupportStore {
  items = $state<Record<string, RuntimeTaskSupport>>({});
  loaded = $state(false);
  private loading: Promise<void> | null = null;

  /** Reads the table (once; `force` after a runtime change). */
  load(force = false): Promise<void> {
    if (this.loaded && !force) return Promise.resolve();
    this.loading ??= invoke<RuntimeTaskSupport[]>('get_task_support', { runtimeId: null })
      .then((list) => {
        this.items = Object.fromEntries(list.map((x) => [x.task, x]));
        this.loaded = true;
      })
      .catch(() => {
        // No backend (or no runtime yet): chips simply show no verdict.
      })
      .finally(() => (this.loading = null));
    return this.loading;
  }

  info(id: string | null | undefined): RuntimeTaskSupport | null {
    return id ? (this.items[id] ?? null) : null;
  }

  /** Support level of a task; null while unknown. */
  support(id: string | null | undefined): TaskSupport | null {
    return this.info(id)?.support ?? null;
  }
}

export const taskSupportStore = new TaskSupportStore();
