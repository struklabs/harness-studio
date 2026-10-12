import { onMount } from 'svelte';
import { initialPage } from '$lib/uiLevel';
import { uiLevel } from './uiLevel.svelte';
import type { SectionGroup } from '$lib/components/common/sections';

const requested = $state<Record<string, string | undefined>>({});
export function requestSection(screen: string, id: string) { requested[screen] = id; }

/** Local page selection; content stays mounted independently of navigation. */
export class SectionSelection {
  active = $state('');
  #selected = false;
  constructor(readonly screen: string, readonly ids: string[], fallback = ids[0], readonly normalize = (id: string) => id, readonly groups?: SectionGroup[]) {
    this.active = fallback;
    $effect(() => { const id = requested[screen]; if (id) { this.select(id); delete requested[screen]; } });
    onMount(() => {
      // Restoration is not a navigation action: preserve the saved hidden page.
      try {
        const saved = normalize(localStorage.getItem(`harness_${screen}_page`) ?? fallback);
        const restored = groups ? initialPage(groups, saved, uiLevel.level) : saved;
        if (!this.#selected && ids.includes(restored)) this.active = restored;
      } catch { /* unavailable */ }
      const fromUrl = () => {
        const url = new URL(location.href);
        const hash = url.hash.match(/^#([^/]+)\/(.+)$/);
        const page = hash?.[1] === screen ? hash[2] : (url.searchParams.get('screen') ?? url.searchParams.get('tab')) === screen ? url.searchParams.get('section') : null;
        if (page) this.select(page);
      };
      fromUrl();
      window.addEventListener('hashchange', fromUrl);
      window.addEventListener('popstate', fromUrl);
      return () => { window.removeEventListener('hashchange', fromUrl); window.removeEventListener('popstate', fromUrl); };
    });
  }
  restore = (id: string) => {
    if (this.#selected) return;
    id = this.normalize(id);
    const restored = this.groups ? initialPage(this.groups, id, uiLevel.level) : id;
    if (this.ids.includes(restored)) this.active = restored;
  };
  select = (id: string) => {
    id = this.normalize(id);
    if (!this.ids.includes(id)) return;
    this.#selected = true;
    this.active = id;
    try { localStorage.setItem(`harness_${this.screen}_page`, id); } catch { /* unavailable */ }
  };
}
