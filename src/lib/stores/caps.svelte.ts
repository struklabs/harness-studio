import { invoke } from '$lib/ipc';
import type { FeatureStatus, FlagInfo, ParamCatalog, ParamSpec, RuntimeCapabilities } from '$lib/types';

/**
 * Runtime capabilities of the selected server's llama-server binary, the parameter catalog
 * and per-feature status. While nothing is known (probe failed or still loading) every
 * setting counts as supported, so the Dashboard behaves as before.
 */
class CapsState {
  caps = $state<RuntimeCapabilities | null>(null);
  catalog = $state<ParamCatalog | null>(null);
  features = $state<Record<string, FeatureStatus>>({});
  loading = $state(false);
  error = $state<string | null>(null);

  /** Flag spelling -> flag info (all aliases). */
  private index = $derived.by(() => {
    const map = new Map<string, FlagInfo>();
    for (const info of Object.values(this.caps?.flags ?? {})) {
      for (const alias of info.aliases) map.set(alias, info);
    }
    return map;
  });

  /** The runtime's help was read, so missing flags really are missing. */
  known = $derived(!!this.caps?.help_parsed);

  private loadedFor: string | null = null;
  private featureToken = 0;
  /** Runtime the form selects (`null` = the server's own / Settings > Runtime). */
  runtimeId = $state<string | null>(null);

  /**
   * Loads the capabilities of the binary `serverId` would use; `runtimeId` (the form's runtime
   * selector) wins over the server's saved runtime, so hidden parameters follow the selection.
   */
  async load(serverId: string, force = false, runtimeId: string | null = this.runtimeId) {
    const key = `${serverId}|${runtimeId ?? ''}`;
    if (!force && this.loadedFor === key && this.caps) return;
    this.loadedFor = key;
    this.runtimeId = runtimeId;
    this.loading = true;
    try {
      const [caps, catalog] = await Promise.all([
        invoke<RuntimeCapabilities>('get_runtime_capabilities', { serverId, runtimeId }),
        this.catalog ? Promise.resolve(this.catalog) : invoke<ParamCatalog>('get_param_catalog'),
      ]);
      if (this.loadedFor !== key) return;
      this.caps = caps;
      this.catalog = catalog;
      this.error = null;
    } catch (e) {
      if (this.loadedFor !== key) return;
      this.caps = null;
      this.error = String(e);
      if (!this.catalog) {
        try {
          this.catalog = await invoke<ParamCatalog>('get_param_catalog');
        } catch {
          // catalog is embedded; this only fails outside Tauri
        }
      }
    } finally {
      this.loading = false;
    }
  }

  async loadFeatures(serverId: string, modelPath: string | null, hasMmproj: boolean) {
    const token = ++this.featureToken;
    try {
      const list = await invoke<FeatureStatus[]>('get_feature_status', {
        serverId,
        modelPath,
        hasMmproj,
        runtimeId: this.runtimeId,
      });
      if (token !== this.featureToken) return;
      this.features = Object.fromEntries(list.map((f) => [f.id, f]));
    } catch {
      if (token === this.featureToken) this.features = {};
    }
  }

  flag(spelling: string): FlagInfo | undefined {
    const info = this.index.get(spelling);
    return info && !info.removed ? info : undefined;
  }

  /** The runtime accepts one of the spellings (true while unknown). */
  hasAny(spellings: string[]): boolean {
    if (!this.known) return true;
    return spellings.some((s) => !!this.flag(s));
  }

  spec(id: string): ParamSpec | undefined {
    return this.catalog?.param.find((p) => p.id === id);
  }

  /** A catalog parameter (typed field or not) is usable with this runtime. */
  supports(id: string): boolean {
    const spec = this.spec(id);
    return spec ? this.hasAny(spec.flags) : true;
  }

  /** Allowed values for an enum parameter: from --help, plus the catalog's own. */
  allowedValues(spec: ParamSpec): string[] {
    const out: string[] = [];
    if (spec.values_from_help) {
      const info = spec.flags.map((f) => this.flag(f)).find(Boolean);
      out.push(...(info?.enum_values ?? []));
    }
    for (const v of spec.values) if (!out.includes(v)) out.push(v);
    return out;
  }

  /** Catalog parameters rendered from `params` (not typed fields) of a group. */
  groupSpecs(group: string): ParamSpec[] {
    return (this.catalog?.param ?? []).filter((p) => !p.legacy_field && p.group === group);
  }
}

export const capsStore = new CapsState();
