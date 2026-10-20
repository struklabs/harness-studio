export type ModuleStatus = 'not_installed' | 'installing' | 'installed' | 'disabled' | 'failed';
export interface LocaleKeys { en: string; tr: string }
export interface Component { id: string; version: string; sources: string[]; download_bytes: number; disk_bytes: number }
export interface ModuleView {
  manifest: {
    id: string; version: string; manifest_version: number; name: LocaleKeys; description: LocaleKeys;
    functions: LocaleKeys[]; platforms: string[]; hardware: LocaleKeys; components: Component[];
    permissions: LocaleKeys[]; shared_component_ids: string[]; min_app_version: string; planned: boolean;
  };
  state: { status: ModuleStatus; installed_version: string | null; error: string | null; components: string[] };
  disk_bytes: number; shared_disk_bytes: number; update_available: boolean;
  data_choices: { id: string; disk_bytes: number; path: string }[];
  incompatibility: string | null;
  progress: { step: string; detail: string; downloaded_bytes: number; total_bytes: number } | null;
  components: Record<string, { version: string; owners: string[]; disk_bytes: number }>;
}
/** Keep planned/incompatible entries inspectable while preventing backend actions. */
export function moduleActions(module: Pick<ModuleView, 'manifest' | 'state' | 'incompatibility' | 'progress'>, busy = false) {
  const planned = module.manifest.planned;
  const installing = module.state.status === 'installing' || module.progress !== null;
  const installed = module.state.status === 'installed' || module.state.status === 'disabled';
  const available = !planned && !module.incompatibility && !installing && !busy;
  return {
    install: !planned && !module.incompatibility && !installing && !installed && !busy,
    open: available && module.state.status === 'installed',
    enable: available && module.state.status === 'disabled',
    disable: available && module.state.status === 'installed',
    repair: available && (installed || module.state.status === 'failed'),
    remove: available && (installed || module.state.status === 'failed'),
    cancel: !planned && installing,
  };
}
export function progressPercent(downloaded: number, total: number): number | undefined {
  if (!Number.isFinite(downloaded) || !Number.isFinite(total) || total <= 0) return undefined;
  return Math.max(0, Math.min(100, downloaded / total * 100));
}
