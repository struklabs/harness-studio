import { invoke } from '#lib/ipc.js';
import { toast } from 'svelte-sonner';
import { t } from '#lib/i18n.svelte.js';
import { app } from './app.svelte';
import type { UiLevel } from '#lib/uiLevel.js';

class UiLevelState {
  #pending = $state<UiLevel | null>(null);
  busy = $state(false);
  get level(): UiLevel { return this.#pending ?? app.settings?.ui_level ?? 'advanced'; }
  get advanced(): boolean { return this.level === 'advanced'; }

  async set(level: UiLevel) {
    if (!app.settings || this.busy || level === this.level) return;
    this.busy = true;
    this.#pending = level;
    const next = { ...$state.snapshot(app.settings), ui_level: level };
    try {
      await invoke('save_app_settings', { settings: next });
      app.setSettings(next);
    } catch (error) {
      toast.error(t('uiLevel.save_failed', { error: String(error) }));
    } finally {
      this.#pending = null;
      this.busy = false;
    }
  }
}

export const uiLevel = new UiLevelState();
