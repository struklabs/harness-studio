import { invoke } from '$lib/ipc';
import { listen } from '@tauri-apps/api/event';
import type { DefinitionState, InferencePreset, ModelDefinition } from '$lib/types';

/**
 * Model definitions, inference presets and the definitions' run state, shared by the
 * Dashboard, the Playground, the preset manager and the command palette.
 */
class DomainState {
  definitions = $state<ModelDefinition[]>([]);
  presets = $state<InferencePreset[]>([]);
  states = $state<DefinitionState[]>([]);
  definitionsLoaded = $state(false);
  presetsLoaded = $state(false);

  /** Preset manager dialog (opened from the Playground and the command palette). */
  presetManagerOpen = $state(false);
  /** Preset to select when the manager opens. */
  presetManagerSelect = $state<string | null>(null);
  /** Command palette asked for a new Playground chat with this preset (consumed there). */
  pendingNewChatPreset = $state<string | null>(null);
  /** "Load and Chat" asked the Playground for a new chat on this server / model alias (consumed there). */
  pendingChat = $state<{ serverId: string; alias: string } | null>(null);
  /** Command palette asked the Playground to open its model picker. */
  pendingModelPicker = $state(false);

  private started = false;
  private pushRefresh: Promise<void> | null = null;
  private pushAgain = false;

  /**
   * Subscribes to `definitions-changed` (once, from the page): a definition made outside the
   * window (CLI `harness load`, Control API, another window) shows up without a rescan. Returns
   * the cleanup. Pushes arriving during a refresh are merged into one more refresh.
   */
  start(): () => void {
    if (this.started) return () => {};
    this.started = true;
    let disposed = false;
    let off: (() => void) | undefined;
    listen('definitions-changed', () => void this.refreshFromPush())
      .then((fn) => (disposed ? fn() : (off = fn)))
      .catch(() => {});
    return () => {
      disposed = true;
      this.started = false;
      off?.();
    };
  }

  private refreshFromPush(): Promise<void> {
    if (this.pushRefresh) {
      this.pushAgain = true;
      return this.pushRefresh;
    }
    this.pushRefresh = (async () => {
      try {
        do {
          this.pushAgain = false;
          await this.refreshDefinitions();
        } while (this.pushAgain);
      } finally {
        this.pushRefresh = null;
      }
    })();
    return this.pushRefresh;
  }

  async refreshDefinitions(): Promise<ModelDefinition[]> {
    try {
      this.definitions = await invoke<ModelDefinition[]>('list_model_definitions');
      this.definitionsLoaded = true;
    } catch {
      // keep the last known list
    }
    return this.definitions;
  }

  async refreshPresets(): Promise<InferencePreset[]> {
    try {
      this.presets = await invoke<InferencePreset[]>('list_inference_presets');
      this.presetsLoaded = true;
    } catch {
      // keep the last known list
    }
    return this.presets;
  }

  async refreshStates(): Promise<DefinitionState[]> {
    try {
      this.states = await invoke<DefinitionState[]>('list_definition_states');
    } catch {
      // keep the last known states
    }
    return this.states;
  }

  definition(id: string | null | undefined): ModelDefinition | undefined {
    return id ? this.definitions.find((d) => d.id === id) : undefined;
  }

  preset(id: string | null | undefined): InferencePreset | undefined {
    return id ? this.presets.find((p) => p.id === id) : undefined;
  }

  state(definitionId: string): DefinitionState | undefined {
    return this.states.find((s) => s.definition_id === definitionId);
  }

  openPresetManager(select: string | null = null) {
    this.presetManagerSelect = select;
    this.presetManagerOpen = true;
  }
}

export const domain = new DomainState();

/** Dashboard definition menu actions. */
export type DefinitionAction = 'new' | 'duplicate' | 'edit' | 'rename' | 'export' | 'import' | 'import_modelfile' | 'import_ollama' | 'delete';

/** File name of a path (either separator). */
export function baseName(path: string): string {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

/** Local file of a definition, if any. */
export function definitionFile(d: ModelDefinition): string | null {
  return d.base.kind === 'local' ? d.base.path : null;
}

/** Same file path (Windows paths compare case-insensitively, both separators). */
export function samePath(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  const n = (p: string) => p.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
  return n(a) === n(b);
}
