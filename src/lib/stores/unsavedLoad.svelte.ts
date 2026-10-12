// Manual load settings that were used for a run but never saved to the model's definition.
// A screen registers them after a start / load that used them (`register`); leaving the screen,
// switching model, stopping the server or closing the app asks once whether they become the
// model's default (`flush`). Settings the automatic default-load rule chose are never part of it.

export interface PendingLoad {
  /** One pending entry per key (the Dashboard form, or a Playground definition). */
  key: string;
  /** Model / definition name for the dialog. */
  alias: string;
  /** Writes the used values to the model's definition as explicit user values. */
  save: () => Promise<void>;
}

class UnsavedLoad {
  pending = $state<PendingLoad | null>(null);
  /** The prompt is on screen for this entry. */
  asking = $state<PendingLoad | null>(null);
  /** Mirrors `AppSettings.no_save_load_prompt`, set by the app shell. */
  disabled = false;
  private waiters: Array<() => void> = [];

  register(p: PendingLoad) {
    if (this.disabled) return;
    this.pending = p;
  }

  /** Drops the entry (saved elsewhere, or its values are no longer in use). */
  clear(key?: string) {
    if (!key || this.pending?.key === key) this.pending = null;
  }

  /** Asks about the pending entry, if any; resolves once it is answered. */
  flush(): Promise<void> {
    const p = this.pending;
    if (!p || this.disabled) {
      this.pending = null;
      return Promise.resolve();
    }
    this.pending = null;
    return new Promise((resolve) => {
      this.waiters.push(resolve);
      if (!this.asking) this.asking = p;
    });
  }

  /** The dialog was answered: the next queued question is shown, waiters are released. */
  done() {
    this.asking = null;
    const w = this.waiters.splice(0);
    for (const r of w) r();
  }
}

export const unsavedLoad = new UnsavedLoad();
