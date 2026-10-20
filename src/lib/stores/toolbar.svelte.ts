import { parseHidden, type ToolbarId } from '#lib/toolbarLayout.js';

const KEY = 'harness_titlebar_hidden';

/** Title bar toolbar buttons the user turned off (Settings > Appearance); kept per device. */
class ToolbarPrefs {
  hidden = $state<ToolbarId[]>([]);

  constructor() {
    try {
      this.hidden = parseHidden(localStorage.getItem(KEY));
    } catch {
      // Storage unavailable: all buttons stay shown.
    }
  }

  isShown(id: ToolbarId): boolean {
    return !this.hidden.includes(id);
  }

  set(id: ToolbarId, shown: boolean): void {
    const rest = this.hidden.filter((h) => h !== id);
    this.hidden = shown ? rest : [...rest, id];
    try {
      localStorage.setItem(KEY, JSON.stringify(this.hidden));
    } catch {
      // Not remembered.
    }
  }
}

export const toolbarPrefs = new ToolbarPrefs();
