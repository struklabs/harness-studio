// One-click installer for every engine (runtime) family a model can need: whisper.cpp,
// stable-diffusion.cpp and sherpa-onnx from their official GitHub releases (the same
// preview + install commands as Settings > Registry), ONNX Runtime and Lemonade through their
// own install commands. Keeps the installing state and progress per family, refreshes the
// stores that show "needs an engine" hints afterwards, and never accepts unverified archives.
import { toast } from 'svelte-sonner';
import { invoke } from '$lib/ipc';
import { t } from '$lib/i18n.svelte';
import { localizeError } from '$lib/network';
import { runtimeInstallErrorText } from '$lib/runtimeBackend';
import { engineSpec, familyInstalled, progressPercent } from '$lib/engines';
import { runtimesStore } from '$lib/stores/runtimes.svelte';
import { library } from '$lib/stores/library.svelte';
import { taskSupportStore } from '$lib/components/hub/taskSupport.svelte';
import { clearPageCache } from '$lib/components/hub/hubData';
import type {
  LemonadeStatus,
  OnnxRuntimeProgress,
  OnnxRuntimeVariant,
  RuntimeDownloadProgress,
  RuntimeEntry,
  RuntimeReleasePick,
} from '$lib/types';

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
  /** Bumped after each successful install, for views that re-read their data. */
  version = $state(0);
  onnxAvailable = $state(false);
  onnxInstalled = $state(false);
  lemonadeInstalled = $state(false);
  lemonadeSupported = $state(false);

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
      invoke<LemonadeStatus>('get_lemonade_status')
        .then((s) => {
          this.lemonadeSupported = s.supported;
          this.lemonadeInstalled = s.installs.length > 0;
        })
        .catch(() => {}),
    ]);
  }

  installed(family: string): boolean {
    const spec = engineSpec(family);
    if (spec?.method === 'onnx') return this.onnxInstalled;
    if (spec?.method === 'lemonade') return this.lemonadeInstalled || familyInstalled(family, runtimesStore.list);
    return familyInstalled(family, runtimesStore.list);
  }

  /** The family exists on this machine (ONNX Runtime and Lemonade are platform-bound). */
  offered(family: string): boolean {
    const spec = engineSpec(family);
    if (spec?.method === 'onnx') return this.onnxAvailable;
    if (spec?.method === 'lemonade') return this.lemonadeSupported;
    return true;
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
  async install(family: string): Promise<boolean> {
    const spec = engineSpec(family);
    if (this.anyBusy) return false;
    if (!spec || spec.method === 'manual') {
      this.states[family] = { busy: false, pct: null, problem: 'manual', error: null };
      toast.error(t('eng.manual', { runtime: family }));
      return false;
    }
    this.states[family] = { busy: true, pct: null, problem: null, error: null };
    try {
      if (spec.method === 'github') {
        if (!(await this.installGithub(family, spec.repo ?? ''))) return false;
      } else if (spec.method === 'onnx') {
        await this.installOnnx();
      } else {
        await this.installLemonade();
      }
      this.states[family] = IDLE;
      toast.success(t('eng.installed_ok', { runtime: family }));
      await this.afterInstall();
      return true;
    } catch (e) {
      const error = spec.method === 'github' ? runtimeInstallErrorText(e) : localizeError(e);
      this.states[family] = { busy: false, pct: null, problem: 'failed', error };
      toast.error(t('eng.failed', { runtime: family, error }));
      return false;
    }
  }

  /** Preview, then install; an archive without a SHA-256 digest is never accepted here. */
  private async installGithub(family: string, repo: string): Promise<boolean> {
    const pick = await invoke<RuntimeReleasePick>('preview_runtime_release', {
      query: { family, repo, tag: null, backend: null, asset: null },
    });
    if (pick.unverified.length > 0) {
      this.states[family] = { busy: false, pct: null, problem: 'unverified', error: null };
      toast.error(t('eng.unverified', { runtime: family }));
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
          name: family,
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

  /** The hints that said "needs an engine" re-read their data, so they disappear at once. */
  private async afterInstall() {
    this.version++;
    clearPageCache();
    await this.refresh();
    await Promise.all([taskSupportStore.load(true), library.refresh().catch(() => [])]);
  }
}

export const engineInstall = new EngineInstaller();
