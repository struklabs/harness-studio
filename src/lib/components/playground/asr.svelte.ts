// The sherpa-onnx speech recognition models for the Speech tab and the chat dictation: what is
// installed, the recommended downloads and their progress.
import { toast } from 'svelte-sonner';
import { invoke } from '$lib/ipc';
import { t } from '$lib/i18n.svelte';
import { localizeError } from '$lib/network';
import type { RuntimeDownloadProgress } from '$lib/types';
import { sherpaNames, type AsrModelInfo, type AsrOverview } from './asr';

class AsrStore {
  overview = $state<AsrOverview | null>(null);
  /** Catalog id being downloaded, and its progress. */
  installingModel = $state<string | null>(null);
  modelProgress = $state<RuntimeDownloadProgress | null>(null);

  models = $derived<AsrModelInfo[]>(this.overview?.models ?? []);
  names = $derived(sherpaNames(this.models));

  async load() {
    try {
      this.overview = await invoke<AsrOverview>('get_asr_overview');
    } catch {
      this.overview = this.overview ?? { models: [], catalog: [], runtime_installed: false };
    }
  }

  /** Downloads a recommended model; failures are reported as a toast. Returns the model's name. */
  async installModel(id: string): Promise<string | null> {
    if (this.installingModel) return null;
    this.installingModel = id;
    const timer = setInterval(async () => {
      try {
        this.modelProgress = await invoke<RuntimeDownloadProgress | null>('get_asr_install_progress');
      } catch {
        /* best effort */
      }
    }, 500);
    try {
      const model = await invoke<AsrModelInfo>('install_asr_model', { id });
      toast.success(t('sasr.model_installed', { name: model.name }));
      await this.load();
      return model.name;
    } catch (e) {
      toast.error(t('sasr.install_failed', { error: localizeError(e) }));
      return null;
    } finally {
      clearInterval(timer);
      this.modelProgress = null;
      this.installingModel = null;
    }
  }
}

export const asr = new AsrStore();
