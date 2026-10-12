import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '$lib/ipc';
import { isIpc } from '$lib/ipcUrl';

/**
 * Whether the window can be seen. WebView2 keeps `document.hidden === false` while the window is
 * minimized or hidden to the tray, so the backend reports that (`window-visible` event) and this
 * module combines it with the page's own visibility. Background work skips while it is false.
 */
let shown = true;
const waiting = new Set<() => void>();
const changed = new Set<(active: boolean) => void>();

export function windowActive(): boolean {
  return shown && !document.hidden;
}

/** Calls `cb` whenever the window becomes visible again (to refresh what was skipped). */
export function onWindowActive(cb: () => void): () => void {
  const onDoc = () => !document.hidden && shown && cb();
  document.addEventListener('visibilitychange', onDoc);
  waiting.add(cb);
  return () => {
    document.removeEventListener('visibilitychange', onDoc);
    waiting.delete(cb);
  };
}

/** Calls `cb` with the new state whenever the window becomes visible or not. */
export function onWindowActivity(cb: (active: boolean) => void): () => void {
  const onDoc = () => cb(windowActive());
  document.addEventListener('visibilitychange', onDoc);
  changed.add(cb);
  return () => {
    document.removeEventListener('visibilitychange', onDoc);
    changed.delete(cb);
  };
}

function set(v: boolean) {
  const was = shown;
  shown = v;
  if (v === was) return;
  if (v) {
    clearTimeout(releaseTimer);
    waiting.forEach((cb) => cb());
  }
  changed.forEach((cb) => cb(windowActive()));
}

// ---- Releasing the web view while hidden to the tray ----
//
// Hidden to the tray, the page is closed after a while (backend `release_window`, unless the
// user keeps it loaded in Settings) and built again from the tray: the tray then costs only
// the app process. Never while a request of this page is in flight (a reply streaming in the
// Playground, a tool call, a long command), and not before it has been quiet for a while.

/** Hidden this long before the page is released (quick tray round trips stay instant). */
const RELEASE_AFTER_MS = 30_000;
/** Quiet this long (no request of the page in flight or finished) before release. */
const QUIET_MS = 10_000;
let inflight = 0;
let lastActivity = Date.now();
let releaseTimer: ReturnType<typeof setTimeout> | undefined;
let beforeRelease: (() => void) | null = null;

/** Checks that keep the page loaded (unsaved text or form edits would be lost with it). */
const holds = new Map<string, () => boolean>();

/** Keeps the page from being released while `has()` is true (e.g. an unsent message).
 *  Returns the cleanup. */
export function holdReleaseWhile(key: string, has: () => boolean): () => void {
  holds.set(key, has);
  return () => {
    if (holds.get(key) === has) holds.delete(key);
  };
}

function held(): boolean {
  for (const has of holds.values()) {
    try {
      if (has()) return true;
    } catch {
      // a broken check never blocks
    }
  }
  return false;
}

/** Called right before the page is released (to remember what to show when rebuilt). */
export function onBeforeRelease(cb: () => void) {
  beforeRelease = cb;
}

let tracking = false;

function trackRequests() {
  if (tracking) return;
  tracking = true;
  const original = window.fetch.bind(window);
  window.fetch = (...args: Parameters<typeof fetch>) => {
    // Tauri commands also travel as fetches (`ipc.localhost` / `ipc://`). A long one keeps the
    // page while it runs, but the short ones views repeat every few seconds while a server runs
    // (its effective values, telemetry) must not count as activity, or the page never goes quiet.
    const ipc = isIpc(args[0]);
    inflight++;
    if (!ipc) lastActivity = Date.now();
    const settle = () => {
      inflight = Math.max(0, inflight - 1);
      if (!ipc) lastActivity = Date.now();
    };
    // Counted until the response arrives; a reply that keeps streaming after that is seen by
    // the backend (its server slot is busy), which then declines the release.
    return original(...args).finally(settle);
  };
}

function scheduleRelease() {
  clearTimeout(releaseTimer);
  if (shown) return;
  releaseTimer = setTimeout(tryRelease, RELEASE_AFTER_MS);
}

function tryRelease() {
  if (shown) return;
  const quietFor = Date.now() - lastActivity;
  if (inflight > 0 || quietFor < QUIET_MS) {
    releaseTimer = setTimeout(tryRelease, Math.max(QUIET_MS - quietFor, 5_000));
    return;
  }
  // Unsaved edits stay in memory: look again later (they may be sent or saved from elsewhere).
  if (held()) {
    releaseTimer = setTimeout(tryRelease, RELEASE_AFTER_MS);
    return;
  }
  try {
    beforeRelease?.();
  } catch {
    // best effort
  }
  invoke<'released' | 'busy' | 'kept'>('release_window')
    .then((result) => {
      // A server is still working on a request (a reply streaming in): look again later.
      if (result === 'busy' && !shown) releaseTimer = setTimeout(tryRelease, QUIET_MS);
    })
    .catch(() => {});
}

/** Starts listening for the backend's visibility events; returns the cleanup. */
export async function initWindowActivity(): Promise<() => void> {
  trackRequests();
  const un = await listen<boolean>('window-visible', (e) => set(e.payload));
  // Hidden to the tray (not just minimized): the page may be released after a while.
  const unTray = await listen('window-in-tray', () => scheduleRelease());
  try {
    const w = getCurrentWindow();
    const visible = await w.isVisible();
    set(visible && !(await w.isMinimized()));
    // Started hidden in the tray ("start minimized", or built ahead by the tray icon and never
    // shown): no `window-in-tray` comes for that, so the release is scheduled here.
    if (!visible) scheduleRelease();
  } catch {
    // Not running inside Tauri (tests): stay "visible".
  }
  return () => {
    un();
    unTray();
    clearTimeout(releaseTimer);
  };
}
