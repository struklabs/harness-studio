// The project list and the Playground's active project (kept in state.db's app_state).
import { invoke } from '#lib/ipc.js';
import { projectApi, type Project } from '#lib/projects.js';

const ACTIVE_KEY = 'active_project';

class ProjectsStore {
  list = $state<Project[]>([]);
  /** The Playground's project: new chats and outputs are filed under it; `null` = none. */
  activeId = $state<string | null>(null);
  loaded = $state(false);
  /** A chat the Projects page asked the Playground to open. */
  pendingConversation = $state<string | null>(null);
  error = $state('');

  active = $derived(this.list.find((p) => p.id === this.activeId) ?? null);

  async load() {
    try {
      const [list, saved] = await Promise.all([
        projectApi.list(false),
        this.loaded ? Promise.resolve(this.activeId) : invoke<string | null>('get_playground_state', { key: ACTIVE_KEY }).catch(() => null),
      ]);
      this.list = list;
      this.activeId = saved && list.some((p) => p.id === saved) ? saved : null;
      this.error = '';
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.loaded = true;
    }
  }

  async setActive(id: string | null) {
    this.activeId = id;
    await invoke('set_playground_state', { key: ACTIVE_KEY, value: id ?? '' }).catch(() => {});
  }
}

export const projects = new ProjectsStore();
