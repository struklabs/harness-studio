<script lang="ts">
  // A side panel with a drag handle on its inner edge (see stores/panes.svelte.ts for the modes).
  // Handle: pointer drag, double-click = default width, arrows = resize, Enter = hide/show
  // (WAI-ARIA window splitter pattern).
  import { onDestroy, type Snippet } from 'svelte';
  import { ChevronLeft, ChevronRight, PanelLeftOpen, PanelRightOpen } from '@lucide/svelte';
  import { t } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import type { PaneState } from '$lib/stores/panes.svelte';

  interface Props {
    pane: PaneState;
    /** Which side of the content the panel sits on; the handle is on the opposite edge. */
    side: 'left' | 'right';
    /** Accessible name of the panel ("Conversations"). */
    label: string;
    /** Classes for the panel surface (background, border colour). */
    class?: string;
    /** When collapsed without a rail, show a slim strip that re-opens the panel. */
    reopenStrip?: boolean;
    children: Snippet;
  }

  let { pane, side, label, class: className = '', reopenStrip = false, children }: Props = $props();

  const STEP = 16;
  const BIG_STEP = 64;

  let panelEl = $state<HTMLElement>();
  let restoreFocus: HTMLElement | null = null;
  let startX = 0;
  let startWidth = 0;

  // Leaving the narrow range drops the floating state, so narrowing again starts closed.
  $effect(() => {
    if (!pane.narrow) pane.floatingOpen = false;
  });

  // A floating panel closes on Escape and on a click outside it (its toggle buttons excepted,
  // they carry aria-controls and toggle it themselves).
  $effect(() => {
    if (!pane.floating) return;
    restoreFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    panelEl?.querySelector<HTMLElement>('button')?.focus();
    const close = () => {
      const hadFocus = panelEl?.contains(document.activeElement);
      pane.close();
      if (hadFocus) (restoreFocus?.isConnected ? restoreFocus : panelEl?.querySelector<HTMLElement>(`button[aria-controls="${pane.elementId}"]`))?.focus();
    };
    const onPointer = (e: PointerEvent) => {
      const target = e.target as Element | null;
      if (!target || panelEl?.contains(target)) return;
      if (target.closest(`[aria-controls="${pane.elementId}"]`)) return;
      // The adjacent main sidebar is an independent control, not the drawer's backdrop.
      if ((pane.isSection || pane.id === 'pg-settings') && target.closest('#pane-nav')) return;
      // Menus and popovers opened from inside the panel are portalled to <body>.
      if (target.closest('[data-bits-floating-content], [role="menu"], [role="listbox"], [role="dialog"]')) return;
      close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.defaultPrevented) { e.preventDefault(); close(); }
    };
    document.addEventListener('pointerdown', onPointer, true);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', onPointer, true);
      document.removeEventListener('keydown', onKey);
    };
  });

  let showsContent = $derived(pane.expanded || pane.rail > 0);
  let panelWidth = $derived(pane.expanded ? pane.width : pane.rail);
  // Section drawers and the main icon rail keep their edge free for adjacent controls.
  let hasHandle = $derived(showsContent && !(pane.narrow && !pane.floating) &&
    !(pane.isSection && pane.floating) && !(pane.id === 'nav' && !pane.expanded));
  let showStrip = $derived(reopenStrip && !showsContent);
  const STRIP = 20;

  function sign(): 1 | -1 {
    return side === 'left' ? 1 : -1;
  }

  // Move/up are tracked on the window: dragging a panel shut unmounts the handle mid-drag.
  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    startX = e.clientX;
    startWidth = panelWidth;
    pane.dragging = true;
    // Chromium starts a text selection on the mousedown that follows this pointerdown, and a
    // selection that already exists is not cleared by `user-select: none`: block both, so the
    // log panel next to the handle is not left with a grey selection block.
    clearSelection();
    document.documentElement.classList.add('pane-resizing');
    document.addEventListener('selectstart', blockSelection, true);
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', endDrag);
    window.addEventListener('pointercancel', endDrag);
    window.addEventListener('blur', endDrag);
  }

  function clearSelection() {
    const sel = window.getSelection();
    if (sel && sel.rangeCount > 0 && !sel.isCollapsed) sel.removeAllRanges();
  }

  function blockSelection(e: Event) {
    e.preventDefault();
  }

  function onPointerMove(e: PointerEvent) {
    if (!pane.dragging) return;
    clearSelection();
    pane.dragTo(startWidth + (e.clientX - startX) * sign());
  }

  function endDrag() {
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', endDrag);
    window.removeEventListener('pointercancel', endDrag);
    window.removeEventListener('blur', endDrag);
    document.removeEventListener('selectstart', blockSelection, true);
    document.documentElement.classList.remove('pane-resizing');
    if (!pane.dragging) return;
    pane.dragging = false;
    clearSelection();
    pane.save();
  }

  onDestroy(endDrag);

  function onKeyDown(e: KeyboardEvent) {
    const step = e.shiftKey ? BIG_STEP : STEP;
    switch (e.key) {
      case 'ArrowLeft':
        pane.resizeBy(-step * sign());
        break;
      case 'ArrowRight':
        pane.resizeBy(step * sign());
        break;
      case 'Home':
        pane.setSize(pane.min);
        break;
      case 'End':
        pane.setSize(pane.maxNow);
        break;
      case 'Enter':
        pane.toggle();
        break;
      default:
        return;
    }
    e.preventDefault();
  }
</script>

<div
  class={cn('relative h-full shrink-0', !pane.dragging && 'transition-[width] duration-150 ease-out motion-reduce:transition-none')}
  style:width="{showStrip ? STRIP : pane.dockedWidth}px"
>
  <div
    bind:this={panelEl}
    id={pane.elementId}
    role="region"
    aria-label={label}
    hidden={!showsContent}
    class={cn(
      'group/pane flex h-full min-w-0 flex-col',
      pane.floating ? 'absolute inset-y-0 z-40 shadow-2xl' : 'relative',
      pane.floating && (side === 'left' ? 'left-0' : 'right-0'),
      side === 'left' ? 'border-r' : 'border-l',
      'border-border',
      className
    )}
    style:width={pane.floating ? `${panelWidth}px` : '100%'}
  >
    {@render children()}

    {#if reopenStrip && pane.expanded && !pane.narrow}
      {@const Icon = side === 'left' ? ChevronLeft : ChevronRight}
      <button
        type="button"
        class={cn(
          'absolute top-2.5 z-40 flex size-5 items-center justify-center rounded-full border border-border bg-background text-muted-foreground opacity-0 shadow-sm outline-none transition-opacity group-hover/pane:opacity-100 hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring/50',
          side === 'left' ? '-right-2.5' : '-left-2.5'
        )}
        aria-controls={pane.elementId}
        aria-expanded="true"
        aria-label={t('pane.hide', { name: label })}
        title={t('pane.hide', { name: label })}
        onclick={() => pane.close()}
      >
        <Icon class="size-3" />
      </button>
    {/if}

    {#if hasHandle}
      <!-- A focusable separator is the ARIA window splitter pattern (a widget, not static content). -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div
        role="separator"
        tabindex="0"
        aria-orientation="vertical"
        aria-controls={pane.elementId}
        aria-label={t('pane.resize', { name: label })}
        aria-valuenow={pane.expanded ? pane.width : pane.rail}
        aria-valuemin={pane.rail || pane.min}
        aria-valuemax={pane.maxNow}
        title={t('pane.resize_hint')}
        class={cn(
          'group/handle absolute inset-y-0 z-30 w-2 cursor-col-resize touch-none outline-none select-none',
          side === 'left' ? '-right-1' : '-left-1'
        )}
        onpointerdown={onPointerDown}
        onmousedown={(e) => e.preventDefault()}
        ondblclick={() => pane.reset()}
        onkeydown={onKeyDown}
      >
        <span
          aria-hidden="true"
          class={cn(
            'pointer-events-none absolute inset-y-0 left-1/2 w-0.5 -translate-x-1/2 bg-transparent transition-colors delay-100',
            'group-hover/handle:bg-primary/50 group-focus-visible/handle:bg-ring',
            pane.dragging && 'bg-primary/70'
          )}
        ></span>
      </div>
    {/if}
  </div>

  {#if showStrip}
    {@const Icon = side === 'left' ? PanelLeftOpen : PanelRightOpen}
    <button
      type="button"
      class={cn(
        'flex h-full w-full items-start justify-center border-border bg-card/60 pt-3 text-muted-foreground outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset',
        side === 'left' ? 'border-r' : 'border-l'
      )}
      aria-controls={pane.elementId}
      aria-expanded="false"
      aria-label={t('pane.show', { name: label })}
      title={t('pane.show', { name: label })}
      onclick={() => pane.open()}
    >
      <Icon class="size-3.5" />
    </button>
  {/if}
</div>
