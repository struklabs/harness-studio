// Live log lines of several servers at once. Each server's ring buffer is read with its own
// cursor (only lines newer than the last one we have); every line gets a number from one
// counter in the order it reached the UI. The backend stamps every line with the time it was
// written, and a merged view orders by that time (ties by arrival), so lines of different
// servers that reach the UI in the same push (up to ~250 ms) still come in their real order.

import { invoke } from '#lib/ipc.js';
import type { LogEntry } from '#lib/components/dashboard/logFilter.js';

/** A line as the backend buffers it: text and the moment it was written (ms since the epoch). */
interface TimedLine {
  at_ms: number;
  text: string;
}

/** Lines kept per server. */
export const MAX_LOG_LINES = 1000;

class ServerLogs {
  /** Lines per server id, oldest first. */
  buffers = $state.raw<Record<string, LogEntry[]>>({});
  private cursors = new Map<string, number>();
  private stamp = 0;
  private wanted = new Set<string>();
  private inFlight = new Map<string, Promise<void>>();
  private again = new Set<string>();

  /** Sets which servers are read; the others are dropped (memory), new ones are loaded. */
  want(ids: string[]) {
    this.wanted = new Set(ids);
    const drop = Object.keys(this.buffers).filter((id) => !this.wanted.has(id));
    if (drop.length > 0) {
      this.buffers = Object.fromEntries(Object.entries(this.buffers).filter(([id]) => this.wanted.has(id)));
      for (const id of drop) this.cursors.delete(id);
    }
    void this.pull(ids.filter((id) => !(id in this.buffers)));
  }

  /** Reads the new lines of `ids` (those not wanted are ignored). */
  pull(ids: string[]): Promise<void> {
    return Promise.all(ids.filter((id) => this.wanted.has(id)).map((id) => this.fetch(id))).then(() => {});
  }

  /** Every wanted server (a refresh after the window was hidden). */
  pullAll(): Promise<void> {
    return this.pull([...this.wanted]);
  }

  private fetch(id: string): Promise<void> {
    const running = this.inFlight.get(id);
    if (running) {
      this.again.add(id); // one more pass after the one in flight
      return running;
    }
    const p = this.read(id).finally(() => {
      this.inFlight.delete(id);
      if (this.again.delete(id)) void this.fetch(id);
    });
    this.inFlight.set(id, p);
    return p;
  }

  private async read(id: string) {
    try {
      const res = await invoke<[number, TimedLine[]]>('get_server_logs_since', { serverId: id, since: this.cursors.get(id) ?? 0 });
      if (!this.wanted.has(id) || !Array.isArray(res) || res.length !== 2 || !Array.isArray(res[1])) return;
      const [next, lines] = res;
      if (lines.length > 0) {
        const fresh = lines.map((l) => ({ seq: ++this.stamp, text: l.text, server: id, at: l.at_ms }));
        const merged = (this.buffers[id] ?? []).concat(fresh);
        this.buffers = { ...this.buffers, [id]: merged.length > MAX_LOG_LINES ? merged.slice(merged.length - MAX_LOG_LINES) : merged };
      } else if (!(id in this.buffers)) {
        this.buffers = { ...this.buffers, [id]: [] };
      }
      if (typeof next === 'number') this.cursors.set(id, next);
    } catch {
      // A failed background read is retried with the next push.
    }
  }

  /** Empties the shown buffers and the servers' own ones. */
  async clear(ids: string[]) {
    this.buffers = { ...this.buffers, ...Object.fromEntries(ids.map((id) => [id, [] as LogEntry[]])) };
    for (const id of ids) this.cursors.set(id, 0);
    await Promise.all(ids.map((id) => invoke('clear_server_logs', { serverId: id })));
  }

  /** The lines of `ids` in the order they arrived. */
  merged(ids: string[]): LogEntry[] {
    const lists = ids.map((id) => this.buffers[id] ?? []);
    if (lists.length === 1) return lists[0];
    return lists.flat().sort((a, b) => (a.at ?? 0) - (b.at ?? 0) || a.seq - b.seq);
  }
}

export const serverLogs = new ServerLogs();
