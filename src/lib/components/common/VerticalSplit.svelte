<script lang="ts">
  // Two stacked areas with a draggable horizontal divider between them (WAI-ARIA window
  // splitter): pointer drag, arrows = resize, Home/End = smallest/largest, double-click = the
  // top area's natural height. The chosen height is remembered per `id` (localStorage).
  import type { Snippet } from 'svelte';
  import { cn } from '$lib/utils';

  interface Props {
    /** Storage key for the remembered height. */
    id: string;
    /** Accessible name of the divider. */
    label: string;
    /** Smallest height of the top and bottom areas (px). */
    minTop?: number;
    minBottom?: number;
    top: Snippet;
    bottom: Snippet;
  }

  let { id, label, minTop = 96, minBottom = 120, top, bottom }: Props = $props();

  const STEP = 16;
  const BIG_STEP = 64;
  const storageKey = $derived(`hs.split.${id}`);

  let container = $state<HTMLElement>();
  /** Height of the top area; null = its natural height. */
  let height = $state<number | null>(null);
  let dragging = $state(false);
  let startY = 0;
  let startHeight = 0;

  $effect(() => {
    try {
      const v = Number(localStorage.getItem(storageKey));
      height = Number.isFinite(v) && v > 0 ? v : null;
    } catch {
      height = null;
    }
  });

  function maxTop(): number {
    const total = container?.clientHeight ?? 0;
    return Math.max(minTop, total - minBottom);
  }

  function clamp(v: number): number {
    return Math.round(Math.min(maxTop(), Math.max(minTop, v)));
  }

  function save(v: number | null) {
    height = v;
    try {
      if (v === null) localStorage.removeItem(storageKey);
      else localStorage.setItem(storageKey, String(v));
    } catch {
      // not persisted
    }
  }

  function topEl(): HTMLElement | null {
    return (container?.firstElementChild as HTMLElement | null) ?? null;
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    startY = e.clientY;
    startHeight = topEl()?.getBoundingClientRect().height ?? height ?? minTop;
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    height = clamp(startHeight + e.clientY - startY);
  }

  function onPointerUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    save(height);
  }

  function onKeydown(e: KeyboardEvent) {
    const current = height ?? topEl()?.getBoundingClientRect().height ?? minTop;
    const step = e.shiftKey ? BIG_STEP : STEP;
    let next: number | null;
    if (e.key === 'ArrowUp') next = clamp(current - step);
    else if (e.key === 'ArrowDown') next = clamp(current + step);
    else if (e.key === 'Home') next = minTop;
    else if (e.key === 'End') next = maxTop();
    else if (e.key === 'Enter') next = null;
    else return;
    e.preventDefault();
    save(next);
  }

  // The stored height may no longer fit after the window got smaller.
  let shownHeight = $derived(height === null ? null : height);
</script>

<div bind:this={container} class="flex min-h-0 flex-1 flex-col overflow-hidden">
  <div
    class={cn('shrink-0', shownHeight !== null && 'overflow-y-auto')}
    style:height={shownHeight !== null ? `min(${shownHeight}px, calc(100% - ${minBottom}px))` : undefined}
  >
    {@render top()}
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    role="separator"
    aria-orientation="horizontal"
    aria-label={label}
    aria-valuenow={shownHeight ?? undefined}
    aria-valuemin={minTop}
    tabindex="0"
    class={cn(
      'group relative z-10 -my-1 flex h-2 shrink-0 cursor-row-resize items-center justify-center outline-none',
      'focus-visible:ring-2 focus-visible:ring-ring',
    )}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    ondblclick={() => save(null)}
    onkeydown={onKeydown}
  >
    <span
      class={cn(
        'h-0.5 w-10 rounded-full bg-border transition-colors group-hover:bg-primary/60',
        dragging && 'bg-primary',
      )}
    ></span>
  </div>
  <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
    {@render bottom()}
  </div>
</div>
