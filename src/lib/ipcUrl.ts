/** A Tauri command call: `http://ipc.localhost/<cmd>` on Windows/Linux, `ipc://localhost/<cmd>` on macOS. */
export function isIpc(input: RequestInfo | URL): boolean {
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
  return /^(?:https?:\/\/ipc\.localhost|ipc:\/\/localhost)(?:[/?#]|$)/i.test(url);
}
