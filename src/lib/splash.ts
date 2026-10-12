/**
 * The start screen in src/app.html (`#hs-splash`): plain HTML/CSS that paints before any script,
 * at app start and when the window is rebuilt from the tray. Removed once the first real screen
 * (or the error screen) is up.
 */
let hidden = false;

export function hideSplash(): void {
  if (hidden || typeof document === 'undefined') return;
  hidden = true;
  const el = document.getElementById('hs-splash');
  if (!el) return;
  const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
  if (reduced) {
    el.remove();
    return;
  }
  el.classList.add('hs-out');
  // Removed after the fade; the timer also covers a window that is not drawn (a page built
  // hidden in the background runs no transitions).
  el.addEventListener('transitionend', () => el.remove(), { once: true });
  setTimeout(() => el.remove(), 250);
}
