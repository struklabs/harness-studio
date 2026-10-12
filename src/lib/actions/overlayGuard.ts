// Safety net for overlay leftovers. bits-ui modal layers (dialogs, sheets, dropdown menus, selects)
// lock the page with `pointer-events: none` on <body> and restore it on a timer after they close.
// When two layers overlap (a popover with a nested menu, a sheet closed with Escape while a tooltip
// unmounts) the restore can be skipped, and the next click lands on <html>: the sidebar item only
// gets its hover style and the page does not change until a second click. This guard clears the
// lock whenever no open overlay exists, so it never outlives the layer that set it.

const OPEN_OVERLAY =
  '[data-state="open"][role="dialog"], [data-state="open"][role="alertdialog"], [data-state="open"][role="menu"], [data-state="open"][role="listbox"], [data-dialog-content][data-state="open"]';

function hasOpenOverlay(): boolean {
  return document.querySelector(OPEN_OVERLAY) !== null;
}

function release(): void {
  const body = document.body;
  if (body.style.pointerEvents === 'none' && !hasOpenOverlay()) {
    body.style.removeProperty('pointer-events');
  }
}

/** Installs the guard; returns a cleanup function. */
export function installOverlayGuard(): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  // Debounced so bits-ui's own delayed restore (and exit animations) run first.
  const schedule = () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(release, 150);
  };
  const observer = new MutationObserver(schedule);
  observer.observe(document.body, { attributes: true, attributeFilter: ['style'] });
  // A press on the bare document while the lock is stale releases it right away.
  const onPointerDown = (e: PointerEvent) => {
    if (e.target === document.documentElement) release();
  };
  document.addEventListener('pointerdown', onPointerDown, true);
  schedule();
  return () => {
    observer.disconnect();
    document.removeEventListener('pointerdown', onPointerDown, true);
    if (timer) clearTimeout(timer);
  };
}
