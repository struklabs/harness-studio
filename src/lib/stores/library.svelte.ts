import { invoke } from '#lib/ipc.js';
import type { LocalModel } from '#lib/types.js';
import { app } from '#lib/stores/app.svelte.js';

/** Path in a comparable form (forward slashes, lower case; Windows paths are case-insensitive). */
export function normPath(p: string): string {
  return p.replace(/\\/g, '/').toLowerCase();
}

/** Path of a model file inside its repo folder (`<library>/<owner>/<repo>/<this>`), or null. */
export function repoRelative(m: LocalModel, repoId: string): string | null {
  const p = normPath(m.path);
  const marker = `/${repoId.toLowerCase()}/`;
  const i = p.indexOf(marker);
  return i < 0 ? null : p.slice(i + marker.length);
}

/** Whether the model is a whole repo folder (`<library>/<owner>/<repo>`: an MLX or Python
 *  model), which `repoRelative` does not see: there is no file path inside the repo. */
export function isRepoFolder(m: LocalModel, repoId: string): boolean {
  return normPath(m.path).replace(/\/+$/, '').endsWith(`/${repoId.toLowerCase()}`);
}

/**
 * Local model library as the Hub sees it, plus small cross-view signals:
 * `version` bumps when the library changed on disk (a download finished) so views that show
 * local models can rescan; `pendingSelectId` asks the Dashboard to select a model.
 */
class LibraryState {
  models = $state<LocalModel[]>([]);
  loaded = $state(false);
  /** Bumped whenever the library on disk changed. */
  version = $state(0);
  /** Model the Dashboard should select next (set by the "open in Dashboard" toast action). */
  pendingSelectId = $state<string | null>(null);
  /** With `pendingSelectId`: open the model in a new server form (the selected server is running). */
  pendingNewServer = $state(false);
  /** Definition the Dashboard should open (My Models: "default load settings"). */
  pendingDefinitionId = $state<string | null>(null);
  /** Repos where a downloaded variant has a newer upstream version (filled lazily by the drawer). */
  updates = $state<Record<string, boolean>>({});
  /** Best-fitting quant per repo, once the drawer has loaded its variants. */
  recommended = $state<Record<string, string>>({});
  private inflight: Promise<LocalModel[]> | null = null;

  /** Rescans the library (concurrent calls share one scan). */
  refresh(): Promise<LocalModel[]> {
    if (this.inflight) return this.inflight;
    this.inflight = invoke<LocalModel[]>('scan_local_models')
      .then((list) => {
        this.models = list;
        this.loaded = true;
        return list;
      })
      .catch(() => this.models)
      .finally(() => {
        this.inflight = null;
      });
    return this.inflight;
  }

  /** Signals that files were added/removed; views showing local models rescan. */
  markChanged() {
    this.version++;
  }

  /** Local models that were downloaded from `repoId` (they live in `<library>/<repoId>/`). */
  forRepo(repoId: string): LocalModel[] {
    return this.models.filter((m) => repoRelative(m, repoId) !== null || isRepoFolder(m, repoId));
  }

  /** True when a starting/running server (not router mode) uses one of the model's files. */
  isRunning(m: LocalModel): boolean {
    const own = new Set([m.path, ...(m.shard_paths ?? [])].map(normPath));
    return app.servers.some(
      (s) =>
        (s.status === 'Running' || s.status === 'Starting') &&
        !!s.active &&
        !s.active.router_mode &&
        own.has(normPath(s.active.model_path))
    );
  }
}

export const library = new LibraryState();
