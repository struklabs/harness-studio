// Installs the whisper.cpp runtime from the Hub (the same preview + install commands as
// Settings > Runtime > "Add from GitHub"), for repos whose compatibility says it is needed.
import { toast } from 'svelte-sonner';
import { invoke } from '$lib/ipc';
import { t } from '$lib/i18n.svelte';
import { localizeError } from '$lib/network';
import { runtimesStore } from '$lib/stores/runtimes.svelte';
import type { RuntimeEntry, RuntimeReleasePick } from '$lib/types';

const FAMILY = 'whisper.cpp';
const REPO = 'ggml-org/whisper.cpp';

class WhisperRuntimeInstaller {
  installing = $state(false);

  /** Resolves true once a whisper.cpp runtime is installed; failures are reported as a toast. */
  async install(): Promise<boolean> {
    if (this.installing) return false;
    this.installing = true;
    try {
      const pick = await invoke<RuntimeReleasePick>('preview_runtime_release', {
        query: { family: FAMILY, repo: REPO, tag: null, backend: null, asset: null },
      });
      if (pick.unverified.length > 0) {
        // Unverified files need the explicit confirmation Settings > Runtime asks for.
        toast.error(t('hub.compat_install_unverified'));
        return false;
      }
      const entry = await invoke<RuntimeEntry>('install_runtime_github', {
        request: {
          family: pick.family ?? FAMILY,
          repo: pick.repo,
          tag: pick.tag,
          asset: pick.assets[0]?.name ?? null,
          backend: null,
          name: 'whisper.cpp',
          id: null,
          allow_unverified: false,
        },
      });
      toast.success(t('rt.added', { name: entry.name }));
      await runtimesStore.load(true);
      return true;
    } catch (e) {
      toast.error(t('hub.compat_install_failed', { error: localizeError(e) }));
      return false;
    } finally {
      this.installing = false;
    }
  }
}

export const whisperRuntime = new WhisperRuntimeInstaller();
