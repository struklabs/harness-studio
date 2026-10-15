/**
 * Keeps bottom-right toasts above an element pinned to the window's bottom edge (the chat
 * composer): its height from the bottom goes into `--toast-clearance`, which the Toaster's
 * bottom offset adds. A hidden element (another tab or page) clears it.
 */
export function toastClearance(node: HTMLElement) {
  const root = document.documentElement;
  const update = () => {
    const r = node.getBoundingClientRect();
    root.style.setProperty('--toast-clearance', r.height > 0 ? `${Math.max(0, Math.round(innerHeight - r.top))}px` : '0px');
  };
  const observer = new ResizeObserver(update);
  observer.observe(node);
  addEventListener('resize', update);
  update();
  return () => {
    observer.disconnect();
    removeEventListener('resize', update);
    root.style.removeProperty('--toast-clearance');
  };
}
