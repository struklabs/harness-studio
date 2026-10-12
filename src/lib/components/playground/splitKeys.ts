// Split view pane shortcuts, kept import-free so a plain node test can load it.

type PaneKeyEvent = Pick<KeyboardEvent, 'key' | 'code' | 'altKey' | 'ctrlKey' | 'metaKey'>;

/**
 * Alt+1 / Alt+2 (⌥1 / ⌥2 on macOS) → pane 'a' / 'b'. On macOS Option rewrites `e.key`
 * ('¡', '™'), so the physical `e.code` (Digit / Numpad) is matched as well as `e.key`.
 */
export function paneShortcut(e: PaneKeyEvent): 'a' | 'b' | null {
  if (!e.altKey || e.ctrlKey || e.metaKey) return null;
  if (e.code === 'Digit1' || e.code === 'Numpad1' || e.key === '1') return 'a';
  if (e.code === 'Digit2' || e.code === 'Numpad2' || e.key === '2') return 'b';
  return null;
}
