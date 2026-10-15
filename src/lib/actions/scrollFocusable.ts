import type { Action } from 'svelte/action';

/**
 * Makes a horizontally or vertically scrollable region reachable by keyboard (Tab, then arrow
 * keys scroll it), but only while it actually overflows, so short code blocks add no tab stop.
 * While focusable it is a labelled region, so a screen reader names the tab stop.
 */
export const scrollFocusable: Action<HTMLElement, string> = (node, label) => {
  let name = label;
  let frame = 0;
  const update = () => {
    frame = 0;
    const scrollable = node.scrollWidth > node.clientWidth + 1 || node.scrollHeight > node.clientHeight + 1;
    if (scrollable) {
      node.tabIndex = 0;
      node.setAttribute('role', 'region');
      if (name) node.setAttribute('aria-label', name);
    } else {
      node.removeAttribute('tabindex');
      node.removeAttribute('role');
      node.removeAttribute('aria-label');
    }
  };
  // Streaming mutates on every token: measure at most once per frame.
  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(update);
  };
  const observer = new ResizeObserver(schedule);
  observer.observe(node);
  // Streaming appends text without resizing a fixed-width block.
  const mutations = new MutationObserver(schedule);
  mutations.observe(node, { childList: true, subtree: true, characterData: true });
  update();
  return {
    update(next) {
      name = next;
      schedule();
    },
    destroy() {
      if (frame) cancelAnimationFrame(frame);
      observer.disconnect();
      mutations.disconnect();
    },
  };
};
