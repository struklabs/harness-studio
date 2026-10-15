<script lang="ts">
  import { flushSync, onMount, tick, untrack, type Snippet } from 'svelte';
  import type { LibraryEntry } from '#lib/types.js';
  import { anchoredOffset, columnCount, groupRows, renderedRows, type WindowRow } from './windowing';

  let { entries, layout, viewport, resetKey, children }: {
    entries: LibraryEntry[];
    layout: 'grid' | 'list';
    viewport: HTMLDivElement;
    resetKey: string;
    children: Snippet<[LibraryEntry]>;
  } = $props();

  let grid: HTMLDivElement;
  let width = $state(0);
  let viewportHeight = $state(0);
  let scrollTop = $state(0);
  let gap = $state(8);
  let focusedId = $state<string | null>(null);
  let revision = $state(0);
  let rows = $state<WindowRow[]>([]);
  const heights = new Map<string, number>();
  const nodes = new Map<string, HTMLElement>();
  let previousIds: string[] = [];
  let previousWidth = 0;
  let previousLayout: 'grid' | 'list' | undefined;
  let previousResetKey: string | undefined;
  let generation = 0;
  let observer: ResizeObserver | undefined;
  let frame = 0;
  const portalSelector = '[role="dialog"], [role="alertdialog"], [role="menu"], [data-slot="dropdown-menu-content"]';

  let virtual = $derived(entries.length >= 60);
  let columns = $derived(columnCount(width, layout, gap));
  let mounted = $derived(renderedRows(rows, scrollTop, viewportHeight, entries.findIndex(e => e.model.id === focusedId)));
  let items = $derived(entries.flatMap((entry, index) => {
    const row = Math.floor(index / columns);
    return !virtual || mounted.has(row) ? [{ entry, index, row }] : [];
  }));
  // Empty tracks stand in for offscreen rows; mounted tracks size naturally, including expanded details.
  let tracks = $derived(virtual ? rows.map((r, i) => mounted.has(i) ? 'auto' : `${r.height}px`).join(' ') : undefined);

  $effect.pre(() => {
    const ids = entries.map(e => e.model.id);
    const currentWidth = width;
    const currentLayout = layout;
    const count = columns;
    const spacing = gap;
    const search = resetKey;
    void revision;
    untrack(() => {
      // Width/layout-specific measurements must not leak into differently wrapped cards.
      if (currentWidth !== previousWidth || currentLayout !== previousLayout) heights.clear();
      const next = groupRows(ids, count, heights, 220, spacing);
      const top = scrollTop;
      const target = previousResetKey !== search ? 0 : anchoredOffset(rows, previousIds, next, ids, top);
      rows = next;
      scrollTop = target;
      previousIds = ids;
      previousWidth = currentWidth;
      previousLayout = currentLayout;
      previousResetKey = search;
      const token = ++generation;
      void tick().then(() => {
        if (token !== generation || !viewport || viewport.clientHeight === 0) return;
        viewport.scrollTop = target;
        scrollTop = viewport.scrollTop;
        measure();
      });
    });
  });

  function measureNow() {
    let changed = false;
    for (const [id, node] of nodes) {
      const height = node.getBoundingClientRect().height;
      if (height > 0 && Math.abs((heights.get(id) ?? 0) - height) > 0.5) {
        heights.set(id, height);
        changed = true;
      }
    }
    if (changed) revision++;
  }

  function measure() {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(measureNow);
  }

  function card(node: HTMLElement, id: string) {
    nodes.set(id, node);
    observer?.observe(node);
    measure();
    return { destroy() { observer?.unobserve(node); nodes.delete(id); } };
  }

  onMount(() => {
    observer = new ResizeObserver(measureNow);
    for (const node of nodes.values()) observer.observe(node);
    let hidden = false;
    const size = new ResizeObserver(() => {
      // A hidden, still-mounted page reports zero. Retain its geometry and scroll window.
      if (!viewport.clientHeight || !grid.clientWidth) {
        hidden = true;
        return;
      }
      if (hidden) {
        viewport.scrollTop = scrollTop;
        hidden = false;
      }
      width = grid.getBoundingClientRect().width;
      viewportHeight = viewport.clientHeight;
      gap = parseFloat(getComputedStyle(grid).rowGap) || 8;
      measure();
    });
    size.observe(viewport);
    size.observe(grid);
    const scroll = () => { if (viewport.clientHeight) scrollTop = viewport.scrollTop; };
    const focus = (event: FocusEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const item = target.closest<HTMLElement>('[data-model-id]');
      if (item && grid.contains(item)) focusedId = item.dataset.modelId ?? null;
      else if (!target.closest(portalSelector) && target !== document.body) focusedId = null;
    };
    const keydown = (event: KeyboardEvent) => {
      const target = event.target;
      if (event.key !== 'Tab' || !virtual || !viewport.clientHeight || !(target instanceof Element) || grid.contains(target) || target.closest(portalSelector)) return;
      const position = target.compareDocumentPosition(grid);
      const entering = event.shiftKey
        ? position & Node.DOCUMENT_POSITION_PRECEDING
        : position & Node.DOCUMENT_POSITION_FOLLOWING;
      if (entering) {
        // Mount the boundary before native Tab navigation chooses its next target.
        flushSync(() => { focusedId = entries[event.shiftKey ? entries.length - 1 : 0].model.id; });
      }
    };
    viewport.addEventListener('scroll', scroll, { passive: true });
    document.addEventListener('focusin', focus);
    document.addEventListener('keydown', keydown, true);
    return () => {
      generation++;
      cancelAnimationFrame(frame);
      observer?.disconnect();
      size.disconnect();
      viewport.removeEventListener('scroll', scroll);
      document.removeEventListener('focusin', focus);
      document.removeEventListener('keydown', keydown, true);
    };
  });
</script>

<div
  bind:this={grid!}
  role="list"
  class="grid gap-2"
  style:overflow-anchor={virtual ? 'none' : undefined}
  style:grid-template-columns={layout === 'grid' ? 'repeat(auto-fill,minmax(min(100%,340px),1fr))' : 'minmax(0,1fr)'}
  style:grid-template-rows={tracks}
>
  {#each items as { entry, index, row } (entry.model.id)}
    <div
      use:card={entry.model.id}
      data-model-id={entry.model.id}
      role="listitem"
      aria-setsize={entries.length}
      aria-posinset={index + 1}
      class="grid min-w-0"
      style:grid-row={virtual ? row + 1 : undefined}
      style:grid-column={virtual ? index % columns + 1 : undefined}
    >
      {@render children(entry)}
    </div>
  {/each}
</div>
