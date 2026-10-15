import type { Action } from 'svelte/action';

/**
 * Makes a horizontally or vertically scrollable region reachable by keyboard (Tab, then arrow
 * keys scroll it), but only while it actually overflows, so short code blocks add no tab stop.
 */
export const scrollFocusable: Action<HTMLElement> = (node) => {
  const update = () => {
    const scrollable = node.scrollWidth > node.clientWidth + 1 || node.scrollHeight > node.clientHeight + 1;
    if (scrollable) node.tabIndex = 0;
    else node.removeAttribute('tabindex');
  };
  const observer = new ResizeObserver(update);
  observer.observe(node);
  // Streaming appends text without resizing a fixed-width block.
  const mutations = new MutationObserver(update);
  mutations.observe(node, { childList: true, subtree: true, characterData: true });
  update();
  return {
    destroy() {
      observer.disconnect();
      mutations.disconnect();
    },
  };
};
