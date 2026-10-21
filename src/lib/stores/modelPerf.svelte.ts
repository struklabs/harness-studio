// Measured performance of models on this device (`list_model_perf`), shared by Model Hub,
// Library and Performance > Model Scores.
import { invoke } from '#lib/ipc.js';
import { perfFor, type ModelPerfRecord } from '#lib/modelScores.js';

class ModelPerfStore {
  records = $state<ModelPerfRecord[]>([]);
  loaded = $state(false);
  failed = $state(false);
  private loading: Promise<void> | null = null;

  /** Reads the records (once; `force` after a measurement). */
  load(force = false): Promise<void> {
    if (this.loading) return force ? this.loading.then(() => this.load(true)) : this.loading;
    if (this.loaded && !force) return Promise.resolve();
    this.loading = invoke<ModelPerfRecord[]>('list_model_perf')
      .then((list) => {
        this.records = list;
        this.loaded = true;
        this.failed = false;
      })
      .catch(() => {
        this.failed = true;
      })
      .finally(() => (this.loading = null));
    return this.loading;
  }

  /** Newest record of a model file or name on any device (the records are this machine's). */
  for(name: string): ModelPerfRecord | null {
    return perfFor(this.records, name);
  }
}

export const modelPerf = new ModelPerfStore();
