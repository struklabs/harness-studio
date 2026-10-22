// One-click installer for every engine (runtime) family a model can need: whisper.cpp,
// stable-diffusion.cpp and sherpa-onnx from their official GitHub releases (the same
// preview + install commands as Settings > Registry), ONNX Runtime and Lemonade through their
// own install commands. Keeps the installing state and progress per family, refreshes the
// stores that show "needs an engine" hints afterwards, and never accepts unverified archives.
import { toast } from 'svelte-sonner';
import { invoke } from '#lib/ipc.js';
import { t } from '#lib/i18n.svelte.js';
import { localizeError } from '#lib/network.js';
import { runtimeInstallErrorText } from '#lib/runtimeBackend.js';
import { installManagedRuntime } from '#lib/runtimeInstall.js';
import { rawError } from '#lib/errors.js';
import { EngineInstallFailed, MANAGED_LLAMA, runWithEngine, type ChainDeps } from '#lib/engineAuto.js';
import { engineLabel, engineSpec, familyInstalled, progressPercent } from '#lib/engines.js';
import { runtimesStore } from '#lib/stores/runtimes.svelte.js';
import { library } from '#lib/stores/library.svelte.js';
import { taskSupportStore } from '#lib/components/hub/taskSupport.svelte.js';
import { clearPageCache } from '#lib/components/hub/hubData.js';
import type {
  LemonadeStatus,
  OnnxRuntimeProgress,
  OnnxRuntimeSupport,
  OnnxRuntimeVariant,
  PythonEnvStatus,
  RuntimeDownloadProgress,
  RuntimeEntry,
  RuntimeReleasePick,
} from '#lib/types.js';

/** Why a family could not be installed with one click. */
export type EngineProblem = 'unverified' | 'manual' | 'failed';

export interface EngineState {
  busy: boolean;
  /** 0-100 while downloading, null while the size is unknown. */
  pct: number | null;
  problem: EngineProblem | null;
  /** Localized failure text for `failed`. */
  error: string | null;
}

const IDLE: EngineState = { busy: false, pct: null, problem: null, error: null };

class EngineInstaller {
  states = $state<Record<string, EngineState>>({});
  /** Families whose running install began as an update (the old environment goes away meanwhile). */
  private updating = $state<Record<string, boolean>>({});
  /** Bumped after each successful install, for views that re-read their data. */
  version = $state(0);
  onnxAvailable = $state(false);
  onnxInstalled = $state(false);
  /** Whether this OS / architecture has any ONNX Runtime build (none on Intel Macs). */
  onnxSupport = $state<OnnxRuntimeSupport | null>(null);
  lemonadeInstalled = $state(false);
  lemonadeSupported = $state(false);
  /** The family an automatic install (started by a model flow) is installing right now. */
  autoFamily = $state<string | null>(null);
  /** Managed Python environments of the Python engines, by name. */
  pyEnvs = $state<Record<string, PythonEnvStatus>>({});

  state(family: string): EngineState {
    return this.states[family] ?? IDLE;
  }

  busy(family: string): boolean {
    return this.state(family).busy;
  }

  get anyBusy(): boolean {
    return Object.values(this.states).some((s) => s.busy);
  }

  /** Reads the install state of every family (registry list, ONNX Runtime, Lemonade). */
  async refresh(): Promise<void> {
    await Promise.all([
      runtimesStore.load(true),
      invoke<OnnxRuntimeVariant[]>('list_onnx_runtime')
        .then((v) => {
          this.onnxAvailable = v.length > 0;
          this.onnxInstalled = v.some((x) => x.installed);
        })
        .catch(() => {}),
      invoke<OnnxRuntimeSupport>('get_onnx_runtime_support')
        .then((s) => (this.onnxSupport = s))
        .catch(() => {}),
      invoke<LemonadeStatus>('get_lemonade_status')
        .then((s) => {
          this.lemonadeSupported = s.supported;
          this.lemonadeInstalled = s.installs.length > 0;
        })
        .catch(() => {}),
      this.loadPyEnvs(),
    ]);
  }

  private async loadPyEnvs(): Promise<void> {
    try {
      const list = await invoke<PythonEnvStatus[]>('list_python_envs');
      this.pyEnvs = Object.fromEntries(list.map((e) => [e.name, e]));
    } catch {
      /* best effort */
    }
  }

  /** The managed Python environment of a `pyenv` family. */
  pyEnv(family: string): PythonEnvStatus | undefined {
    const env = engineSpec(family)?.env;
    if (!env) return undefined;
    // A GPU build of the env (`hf-cuda` of `hf-cpu`): the ready one serves (GPU first), else
    // the installed one, else the one this machine should install.
    const pair = [...Object.values(this.pyEnvs).filter((e) => e.gpu_build_of === env), this.pyEnvs[env]].filter(
      (e): e is PythonEnvStatus => !!e,
    );
    return (
      pair.find((e) => e.ready) ??
      pair.find((e) => e.installed) ??
      pair.find((e) => e.preferred && e.supported) ??
      this.pyEnvs[env]
    );
  }

  /**
   * The GPU build a `pyenv` family can move to (`hf-cuda` on an NVIDIA machine whose engine
   * runs on `hf-cpu`), else undefined.
   */
  pyGpuUpgrade(family: string): PythonEnvStatus | undefined {
    const current = this.pyEnv(family);
    if (!current || current.gpu_build_of || !current.installed) return undefined;
    return Object.values(this.pyEnvs).find(
      (e) => e.gpu_build_of === current.name && e.preferred && e.supported && !e.ready,
    );
  }

  /** Why this machine can't have a `pyenv` family's environment (an error to localize), else null. */
  pyRefusal(family: string): string | null {
    return this.pyEnv(family)?.refusal ?? null;
  }

  installed(family: string): boolean {
    const spec = engineSpec(family);
    if (spec?.method === 'onnx') return this.onnxInstalled;
    if (spec?.method === 'lemonade') return this.lemonadeInstalled || familyInstalled(family, runtimesStore.list);
    if (spec?.method === 'pyenv') return !!this.pyEnv(family)?.ready;
    return familyInstalled(family, runtimesStore.list);
  }

  /** A `pyenv` family whose environment exists but was built from an older lock: it needs an update. */
  outdated(family: string): boolean {
    if (engineSpec(family)?.method !== 'pyenv') return false;
    if (this.updating[family]) return true;
    const env = this.pyEnv(family);
    return !!env?.installed && !env.ready;
  }

  /** The family exists on this machine (ONNX Runtime and Lemonade are platform-bound). */
  offered(family: string): boolean {
    const spec = engineSpec(family);
    if (spec?.method === 'onnx') return this.onnxAvailable;
    if (spec?.method === 'lemonade') return this.lemonadeSupported;
    if (spec?.method === 'pyenv') return !!this.pyEnv(family)?.supported;
    return true;
  }

  /** The platform when the family has no build for it at all (ONNX Runtime on an Intel Mac), else null. */
  unsupported(family: string): OnnxRuntimeSupport | null {
    const s = this.onnxSupport;
    return engineSpec(family)?.method === 'onnx' && s && !s.supported ? s : null;
  }

  /** Whether the family has a one-click installer. */
  canInstall(family: string): boolean {
    const spec = engineSpec(family);
    return !!spec && spec.method !== 'manual';
  }

  private set(family: string, patch: Partial<EngineState>) {
    this.states[family] = { ...this.state(family), ...patch };
  }

  /** Resolves true once the family is installed. Failures are reported as a toast and in `state(family)`. */
  async install(family: string, env?: string): Promise<boolean> {
    const spec = engineSpec(family);
    if (this.anyBusy) return false;
    if (!spec || spec.method === 'manual') {
      this.states[family] = { busy: false, pct: null, problem: 'manual', error: null };
      toast.error(t('eng.manual', { runtime: engineLabel(family) }));
      return false;
    }
    const update = this.outdated(family);
    this.states[family] = { busy: true, pct: null, problem: null, error: null };
    this.updating[family] = update;
    try {
      if (spec.method === 'github') {
        if (!(await this.installGithub(family, spec.repo ?? ''))) return false;
      } else if (spec.method === 'onnx') {
        await this.installOnnx();
      } else if (spec.method === 'pyenv') {
        await this.installPyEnv(family, env ?? this.pyEnv(family)?.name ?? spec.env ?? '');
      } else {
        await this.installLemonade();
      }
      // Refresh first, so the notice does not flash its old state between busy and installed.
      // The install itself succeeded, so a failed refresh is not reported as an install failure.
      await this.refreshAvailability().catch(() => {});
      this.states[family] = IDLE;
      toast.success(t(update ? 'eng.updated_ok' : 'eng.installed_ok', { runtime: engineLabel(family) }));
      return true;
    } catch (e) {
      const error = spec.method === 'github' ? runtimeInstallErrorText(e) : localizeError(e);
      this.states[family] = { busy: false, pct: null, problem: 'failed', error };
      toast.error(t('eng.failed', { runtime: engineLabel(family), error }));
      return false;
    } finally {
      this.updating[family] = false;
    }
  }

  /** Preview, then install; an archive without a SHA-256 digest is never accepted here. */
  private async installGithub(family: string, repo: string): Promise<boolean> {
    const pick = await invoke<RuntimeReleasePick>('preview_runtime_release', {
      query: { family, repo, tag: null, backend: null, asset: null },
    });
    if (pick.unverified.length > 0) {
      this.states[family] = { busy: false, pct: null, problem: 'unverified', error: null };
      toast.error(t('eng.unverified', { runtime: engineLabel(family) }));
      return false;
    }
    const timer = setInterval(async () => {
      try {
        const p = await invoke<RuntimeDownloadProgress | null>('get_runtime_install_progress');
        if (p) this.set(family, { pct: progressPercent(p.downloaded_bytes, p.total_bytes) });
      } catch {
        /* best effort */
      }
    }, 500);
    try {
      await invoke<RuntimeEntry>('install_runtime_github', {
        request: {
          family: pick.family ?? family,
          repo: pick.repo,
          tag: pick.tag,
          // The previewed tag and backend pick the same archives again (a CUDA build's
          // `cudart-*` runtime included), as the Registry page does.
          asset: null,
          backend: pick.backend || null,
          name: engineLabel(family),
          id: null,
          allow_unverified: false,
        },
      });
    } finally {
      clearInterval(timer);
    }
    return true;
  }

  private async installOnnx(): Promise<void> {
    const variants = await invoke<OnnxRuntimeVariant[]>('list_onnx_runtime');
    const variant = (variants.find((v) => v.default) ?? variants[0])?.variant;
    if (!variant) throw new Error('onnx.runtime_missing');
    const timer = setInterval(async () => {
      try {
        const p = await invoke<OnnxRuntimeProgress | null>('get_onnx_runtime_progress');
        if (p) this.set('onnxruntime', { pct: progressPercent(p.downloaded_bytes, p.total_bytes) });
      } catch {
        /* best effort */
      }
    }, 500);
    try {
      await invoke('install_onnx_runtime', { variant });
    } finally {
      clearInterval(timer);
    }
  }

  /** Builds the Python environment; `packages` (uv's download) has no byte count, so the bar is indeterminate there. */
  private async installPyEnv(family: string, env: string): Promise<void> {
    const timer = setInterval(async () => {
      await this.loadPyEnvs();
      const p = this.pyEnvs[env]?.progress;
      if (p) this.set(family, { pct: progressPercent(p.downloaded_bytes, p.total_bytes) });
    }, 700);
    try {
      await invoke<PythonEnvStatus>('install_python_env', { name: env });
    } finally {
      clearInterval(timer);
    }
  }

  /** Stops a Python environment build (the partial files are removed). */
  async cancel(family: string): Promise<void> {
    // The build running now (a GPU build can be on its way while the CPU one serves).
    const env = Object.values(this.pyEnvs).find((e) => e.progress)?.name ?? this.pyEnv(family)?.name;
    if (env) await invoke('cancel_python_env_install', { name: env }).catch(() => {});
  }

  /** Deletes the Python environment of a `pyenv` family. */
  async remove(family: string): Promise<void> {
    const env = this.pyEnv(family)?.name;
    if (!env) return;
    try {
      await invoke('remove_python_env', { name: env });
      toast.success(t('eng.removed', { runtime: engineLabel(family) }));
    } catch (e) {
      toast.error(localizeError(e));
    }
    await this.refreshAvailability();
  }

  private async installLemonade(): Promise<void> {
    const timer = setInterval(async () => {
      try {
        const s = await invoke<LemonadeStatus>('get_lemonade_status');
        const p = s.install_progress;
        if (p) this.set('lemonade', { pct: progressPercent(p.downloaded_bytes, p.total_bytes) });
      } catch {
        /* best effort */
      }
    }, 500);
    try {
      await invoke<RuntimeEntry>('install_lemonade', { tag: null });
    } finally {
      clearInterval(timer);
    }
  }

  /** The official llama.cpp build, installed as part of a flow (verified, staged, activated). */
  private async installManagedLlama(): Promise<boolean> {
    this.states[MANAGED_LLAMA] = { busy: true, pct: null, problem: null, error: null };
    try {
      const result = await installManagedRuntime(null, {
        onProgress: (p) => {
          if (p && p.total_bytes > 0) this.set(MANAGED_LLAMA, { pct: progressPercent(p.downloaded_bytes, p.total_bytes) });
        },
      });
      if (result === null) {
        // An archive without a digest is never accepted silently.
        this.states[MANAGED_LLAMA] = { busy: false, pct: null, problem: 'unverified', error: null };
        toast.error(t('eng.unverified', { runtime: MANAGED_LLAMA }));
        return false;
      }
      this.states[MANAGED_LLAMA] = IDLE;
      toast.success(t('eng.installed_ok', { runtime: MANAGED_LLAMA }));
      await this.refreshAvailability();
      return true;
    } catch (e) {
      const error = runtimeInstallErrorText(e);
      this.states[MANAGED_LLAMA] = { busy: false, pct: null, problem: 'failed', error };
      toast.error(t('eng.failed', { runtime: MANAGED_LLAMA, error }));
      return false;
    }
  }

  /**
   * Runs a model flow (start a server, load a model, run a task). When it fails because its
   * engine is missing, the engine is installed (progress in `autoFamily` / `state(family)`) and
   * the flow runs again. No confirm dialog; a failed install throws `EngineInstallFailed`
   * (see `autoFailureText`), so the caller shows the error and the user can retry.
   */
  runWithEngine<T>(action: () => Promise<T>): Promise<T> {
    const deps: ChainDeps = {
      installed: (f) => this.installed(f),
      canInstall: (f) => this.canInstall(f),
      offered: (f) => this.offered(f),
      install: (f) => this.install(f),
      installManaged: () => this.installManagedLlama(),
      raw: (e) => rawError(e),
      onInstalling: (f) => (this.autoFamily = f),
      onDone: () => (this.autoFamily = null),
    };
    return runWithEngine(action, deps);
  }

  /** Localized text for an automatic install that failed, else null (any other error). */
  autoFailureText(e: unknown): string | null {
    if (!(e instanceof EngineInstallFailed)) return null;
    if (e.outcome === 'manual') return t('eng.manual', { runtime: engineLabel(e.family) });
    return t('eng.auto_failed', { runtime: engineLabel(e.family), error: this.state(e.family).error ?? '' });
  }

  /** The hints that said "needs an engine" re-read their data, so they disappear at once. */
  async refreshAvailability() {
    this.version++;
    clearPageCache();
    await this.refresh();
    await Promise.all([taskSupportStore.load(true), library.refresh().catch(() => [])]);
  }
}

export const engineInstall = new EngineInstaller();
