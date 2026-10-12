// Which servers a Dashboard panel shows: "all" (the default) or a chosen few. Remembered per
// panel (localStorage, best effort). A choice that no longer matches any listed server falls
// back to all of them, so a stopped server never leaves the panel empty.

export class ServerFilter {
  /** Chosen server ids; empty = all. */
  chosen = $state<string[]>([]);
  private key: string;

  constructor(key: string) {
    this.key = key;
    try {
      const raw = localStorage.getItem(key);
      const parsed: unknown = raw ? JSON.parse(raw) : [];
      if (Array.isArray(parsed)) this.chosen = parsed.filter((x): x is string => typeof x === 'string').slice(0, 32);
    } catch {
      // storage unavailable or damaged: show all
    }
  }

  /** The ids to show out of `available`: the chosen ones among them, else all of them. */
  effective(available: string[]): string[] {
    const hit = available.filter((id) => this.chosen.includes(id));
    return hit.length > 0 ? hit : available;
  }

  /** Whether `id` is picked on purpose (not just shown because everything is). */
  isChosen(id: string, available: string[]): boolean {
    return this.chosen.length > 0 && this.effective(available).includes(id);
  }

  get isAll(): boolean {
    return this.chosen.length === 0;
  }

  all() {
    this.set([]);
  }

  /** From "all", a click picks that one; afterwards clicks add or remove. */
  toggle(id: string, available: string[]) {
    const current = this.chosen.length > 0 ? this.effective(available) : [];
    const next = current.includes(id) ? current.filter((x) => x !== id) : [...current, id];
    this.set(next.length === available.length ? [] : next);
  }

  /** Picks exactly one server. */
  only(id: string) {
    this.set([id]);
  }

  private set(next: string[]) {
    this.chosen = next;
    try {
      if (next.length === 0) localStorage.removeItem(this.key);
      else localStorage.setItem(this.key, JSON.stringify(next));
    } catch {
      // ignore
    }
  }
}

export const telemetryFilter = new ServerFilter('hs.dash.telemetry_servers');
export const logFilter = new ServerFilter('hs.dash.log_servers');
