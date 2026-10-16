import type { Action } from 'svelte/action';

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

interface DialogOptions {
  /** Called on Escape. Omit to make the dialog non-dismissable via keyboard. */
  onEscape?: () => void;
}

/**
 * Modal dialog behaviour: moves focus into the node, keeps Tab/Shift+Tab inside it,
 * handles Escape and restores focus to the previously focused element once it is gone.
 */
export const dialog: Action<HTMLElement, DialogOptions | undefined> = (node, options) => {
  let opts = options ?? {};
  const previouslyFocused = document.activeElement as HTMLElement | null;

  const focusables = () =>
    Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
      (el) => el.offsetParent !== null || el === document.activeElement
    );

  queueMicrotask(() => {
    const autofocus = node.querySelector<HTMLElement>('[data-autofocus]');
    (autofocus ?? focusables()[0] ?? node).focus();
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && opts.onEscape) {
      e.stopPropagation();
      opts.onEscape();
      return;
    }
    if (e.key !== 'Tab') return;
    const items = focusables();
    if (items.length === 0) {
      e.preventDefault();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (e.shiftKey && (active === first || !node.contains(active))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (active === last || !node.contains(active))) {
      e.preventDefault();
      first.focus();
    }
  }

  node.addEventListener('keydown', onKeydown);
  if (!node.hasAttribute('tabindex')) node.setAttribute('tabindex', '-1');

  return {
    update(next) {
      opts = next ?? {};
    },
    destroy() {
      node.removeEventListener('keydown', onKeydown);
      // After the update that removes the dialog: the page behind it may still be `inert` while
      // this runs, and focus() on an inert element does nothing.
      queueMicrotask(() => {
        if (previouslyFocused && document.contains(previouslyFocused)) previouslyFocused.focus();
      });
    },
  };
};
