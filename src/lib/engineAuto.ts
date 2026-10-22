// Automatic engine (runtime) install. When a model needs an engine that is not installed, the
// flow that wanted the model installs the engine itself and then carries on: no confirm
// dialog, no "install it first" detour. Pure logic with injected dependencies; the UI wiring
// lives in `stores/engineInstall.svelte.ts` and the components.
/** The official llama.cpp build the chat models run on (managed, not a registry family). */
export const MANAGED_LLAMA = 'llama.cpp';

export interface EngineDeps {
  installed(family: string): boolean;
  /** The family has a one-click installer on this machine. */
  canInstall(family: string): boolean;
  /** The family exists on this OS / architecture. */
  offered(family: string): boolean;
  /** Installs the family; resolves true once it is installed. Failures are reported by the installer. */
  install(family: string): Promise<boolean>;
}

/** `ready` = installed now, `manual` = no one-click install exists, `failed` = the install failed. */
export type EnsureOutcome = 'ready' | 'manual' | 'failed';

/** Installs `family` when it is missing and can be installed with one click. */
export async function ensureEngine(family: string, deps: EngineDeps): Promise<EnsureOutcome> {
  if (deps.installed(family)) return 'ready';
  if (!deps.canInstall(family) || !deps.offered(family)) return 'manual';
  const ok = await deps.install(family);
  return ok ? 'ready' : 'failed';
}

/** The engine a raw failure says is missing: a registry family, or the official llama.cpp build. */
export function missingEngineFromError(message: string): string | null {
  if (/\bocr\.rasterizer_unavailable\b/.test(message)) return 'python-hf';
  // Same shape as `familyFromError` in engines.ts ("…: runtime.family_not_installed: whisper.cpp").
  const family = /runtime\.family_not_installed:\s*([\w.+-]+)/.exec(message)?.[1];
  if (family) return family;
  return /\bruntime\.not_installed\b/.test(message) ? MANAGED_LLAMA : null;
}

export class EngineInstallFailed extends Error {
  family: string;
  outcome: 'manual' | 'failed';
  original: unknown;
  constructor(family: string, outcome: 'manual' | 'failed', original: unknown) {
    super(`engine.install_${outcome}: ${family}`);
    this.family = family;
    this.outcome = outcome;
    this.original = original;
  }
}

export interface ChainDeps extends EngineDeps {
  /** The official llama.cpp install (null/false when it did not finish). */
  installManaged(): Promise<boolean>;
  /** Reads the raw text of a failure. */
  raw(error: unknown): string;
  /** The install of `family` begins (for the inline "Installing the model engine…" line). */
  onInstalling?(family: string): void;
  onDone?(family: string): void;
}

/**
 * Runs `action`; when it fails because an engine is missing, installs that engine and runs the
 * action again. Each engine is installed at most once per call (two different engines may be
 * needed one after another); any other failure, and an install that fails, is thrown.
 */
export async function runWithEngine<T>(action: () => Promise<T>, deps: ChainDeps): Promise<T> {
  const tried = new Set<string>();
  for (;;) {
    try {
      return await action();
    } catch (e) {
      const family = missingEngineFromError(deps.raw(e));
      if (!family || tried.has(family)) throw e;
      tried.add(family);
      deps.onInstalling?.(family);
      try {
        const outcome =
          family === MANAGED_LLAMA ? ((await deps.installManaged()) ? 'ready' : 'failed') : await ensureEngine(family, deps);
        if (outcome !== 'ready') throw new EngineInstallFailed(family, outcome, e);
      } finally {
        deps.onDone?.(family);
      }
    }
  }
}

/**
 * Whether the family a download needs can be installed together with it: it is missing,
 * installable and exists on this machine.
 */
export function autoInstallFamily(
  compatibility: { status: string; runtime?: string },
  deps: Pick<EngineDeps, 'installed' | 'canInstall' | 'offered'>,
): string | null {
  if (compatibility.status !== 'needs_runtime' || !compatibility.runtime) return null;
  const family = compatibility.runtime;
  return !deps.installed(family) && deps.canInstall(family) && deps.offered(family) ? family : null;
}
