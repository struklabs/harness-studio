import type { AppSettings, CloseAction, CustomBinaryConfig, RuntimeMode } from '#lib/types.js';

/** Fields the Settings page edits and saves with the "Save" button. Theme, language and ui_level apply immediately. */
export interface DraftShape {
  runtime_mode: RuntimeMode;
  auto_update: boolean;
  custom_binary: CustomBinaryConfig;
  port: number;
  /** Default bind address (`server.host`). */
  host: string;
  /** `network.cors_origins`. */
  cors_origins: string[];
  default_ngl: number;
  default_ctx: number;
  load_timeout_secs: number;
  auto_restart: boolean;
  restart_max_attempts: number;
  auto_benchmark: boolean;
  /** `auto_load_after_download`. */
  auto_load_after_download: boolean;
  close_action: CloseAction;
  start_minimized: boolean;
  restore_last_session: boolean;
  keep_window_in_tray: boolean;
  crash_reports: boolean;
  app_errors: boolean;
  llama_errors: boolean;
  hardware_info: boolean;
  performance_metrics: boolean;
  anonymous_analytics: boolean;
}
export type DraftKey = keyof DraftShape;

type Accessor<K extends DraftKey> = {
  read: (s: AppSettings) => DraftShape[K];
  write: (s: AppSettings, v: DraftShape[K]) => void;
};

function network(s: AppSettings) {
  // Settings of older builds have no network block.
  s.network ??= { cors_origins: [], exposure_acks: {} };
  return s.network;
}

function startup(s: AppSettings) {
  // Older settings objects may predate the startup block.
  s.startup ??= { close_to_tray: false, start_minimized: false, restore_last_session: false };
  return s.startup;
}

const ACCESSORS: { [K in DraftKey]: Accessor<K> } = {
  runtime_mode: { read: (s) => s.runtime_mode, write: (s, v) => (s.runtime_mode = v) },
  auto_update: {
    read: (s) => s.managed_runtime.auto_update,
    write: (s, v) => (s.managed_runtime.auto_update = v),
  },
  custom_binary: { read: (s) => s.custom_binary, write: (s, v) => (s.custom_binary = v) },
  port: { read: (s) => s.server.port, write: (s, v) => (s.server.port = v) },
  host: { read: (s) => s.server.host || '127.0.0.1', write: (s, v) => (s.server.host = v) },
  cors_origins: {
    read: (s) => network(s).cors_origins,
    write: (s, v) => (network(s).cors_origins = v),
  },
  default_ngl: { read: (s) => s.server.default_ngl, write: (s, v) => (s.server.default_ngl = v) },
  default_ctx: { read: (s) => s.server.default_ctx, write: (s, v) => (s.server.default_ctx = v) },
  load_timeout_secs: {
    read: (s) => s.server.load_timeout_secs ?? 300,
    write: (s, v) => (s.server.load_timeout_secs = v),
  },
  auto_restart: { read: (s) => s.server.auto_restart ?? false, write: (s, v) => (s.server.auto_restart = v) },
  restart_max_attempts: {
    read: (s) => s.server.restart_max_attempts ?? 3,
    write: (s, v) => (s.server.restart_max_attempts = v),
  },
  auto_benchmark: { read: (s) => s.server.auto_benchmark ?? true, write: (s, v) => (s.server.auto_benchmark = v) },
  auto_load_after_download: {
    read: (s) => s.auto_load_after_download ?? false,
    write: (s, v) => (s.auto_load_after_download = v),
  },
  close_action: {
    read: (s) => startup(s).close_action ?? (startup(s).close_to_tray ? 'tray' : 'ask'),
    write: (s, v) => {
      startup(s).close_action = v;
      startup(s).close_to_tray = v === 'tray';
    },
  },
  start_minimized: { read: (s) => startup(s).start_minimized, write: (s, v) => (startup(s).start_minimized = v) },
  restore_last_session: {
    read: (s) => startup(s).restore_last_session,
    write: (s, v) => (startup(s).restore_last_session = v),
  },
  keep_window_in_tray: {
    read: (s) => startup(s).keep_window_in_tray ?? false,
    write: (s, v) => (startup(s).keep_window_in_tray = v),
  },
  crash_reports: { read: (s) => s.privacy.crash_reports, write: (s, v) => (s.privacy.crash_reports = v) },
  app_errors: { read: (s) => s.privacy.app_errors, write: (s, v) => (s.privacy.app_errors = v) },
  llama_errors: { read: (s) => s.privacy.llama_errors, write: (s, v) => (s.privacy.llama_errors = v) },
  hardware_info: { read: (s) => s.privacy.hardware_info, write: (s, v) => (s.privacy.hardware_info = v) },
  performance_metrics: {
    read: (s) => s.privacy.performance_metrics,
    write: (s, v) => (s.privacy.performance_metrics = v),
  },
  anonymous_analytics: {
    read: (s) => s.privacy.anonymous_analytics,
    write: (s, v) => (s.privacy.anonymous_analytics = v),
  },
};

const clone = <T>(v: T): T => structuredClone($state.snapshot(v) as T);
const same = (a: unknown, b: unknown) => JSON.stringify($state.snapshot(a)) === JSON.stringify($state.snapshot(b));

/**
 * Tracks only the fields the user changed on top of the latest saved settings. Unedited fields always
 * follow the `settings` prop, so background updates (runtime-updated, storage or language changes)
 * are never overwritten, and saving merges the edits into the current settings.
 */
export class SettingsDraft {
  #source: () => AppSettings;
  #patch = $state<Record<string, unknown>>({});

  constructor(source: () => AppSettings) {
    this.#source = source;
  }

  get<K extends DraftKey>(key: K): DraftShape[K] {
    if (key in this.#patch) return this.#patch[key] as DraftShape[K];
    return (ACCESSORS[key] as Accessor<K>).read(this.#source());
  }

  set<K extends DraftKey>(key: K, value: DraftShape[K]) {
    const base = (ACCESSORS[key] as Accessor<K>).read(this.#source());
    if (same(base, value)) delete this.#patch[key];
    else this.#patch[key] = clone(value);
  }

  isDirty(key: DraftKey): boolean {
    return key in this.#patch;
  }

  /** Forget edits for these keys (their saved value changed elsewhere). */
  discard(...keys: DraftKey[]) {
    for (const k of keys) delete this.#patch[k];
  }

  get dirtyCount(): number {
    return Object.keys(this.#patch).length;
  }

  reset() {
    for (const k of Object.keys(this.#patch)) delete this.#patch[k];
  }

  /** A detached copy of the latest saved settings with the user's edits applied. */
  merged(): AppSettings {
    const next = clone(this.#source());
    for (const key of Object.keys(this.#patch) as DraftKey[]) {
      (ACCESSORS[key] as Accessor<typeof key>).write(next, clone(this.#patch[key]) as never);
    }
    return next;
  }
}
