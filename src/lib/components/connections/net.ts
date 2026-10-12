import { t } from '$lib/i18n.svelte';

/** Localized fetch failures; request URLs never show credentials, queries or fragments. */
export function fetchErrorText(e: unknown, url: string): string {
  const message = e instanceof Error ? e.message : String(e);
  const timeout = e instanceof Error && e.name === 'TimeoutError';
  const network = e instanceof TypeError && /Failed to fetch|NetworkError|Load failed/i.test(message);
  // WebView2/Chromium rejects a body read with "network error" when the server dies mid-response.
  const closed = e instanceof TypeError && /^network error$/i.test(message.trim());
  if (!timeout && !network && !closed) return message;
  let shown = url.split(/[?#]/)[0];
  let local = false;
  try {
    const parsed = new URL(url);
    const host = parsed.hostname.toLowerCase();
    local = host === 'localhost' || host.endsWith('.localhost') || host === '[::1]' || /^127\.\d+\.\d+\.\d+$/.test(host);
    parsed.username = '';
    parsed.password = '';
    parsed.search = '';
    parsed.hash = '';
    shown = parsed.toString();
  } catch { /* keep the address without query or fragment */ }
  const key = timeout
    ? (local ? 'http.timeout_local' : 'http.timeout')
    : closed
      ? 'http.connection_closed'
      : (local ? 'http.connect_failed_local' : 'http.connect_failed');
  return t(key, { url: shown });
}

/** A failed chat request: a recognized transport failure gets its own localized sentence;
 *  anything else keeps the caller's wrapper (e.g. "Sunucuya ulaşılamadı: …"). */
export function requestErrorText(e: unknown, url: string, wrap: (message: string) => string): string {
  const raw = e instanceof Error ? e.message : String(e);
  const text = fetchErrorText(e, url);
  return text === raw ? wrap(raw) : text;
}

/** fetch with a timeout; also honours an external AbortSignal (its listener is always removed). */
export async function fetchWithTimeout(url: string, init: RequestInit = {}, timeoutMs = 10000): Promise<Response> {
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; controller.abort(); }, timeoutMs);
  const external = init.signal ?? null;
  const onAbort = () => controller.abort();
  if (external?.aborted) controller.abort();
  else external?.addEventListener('abort', onAbort, { once: true });
  try {
    return await fetch(url, { ...init, signal: controller.signal });
  } catch (e) {
    if (timedOut && !external?.aborted && e instanceof Error && e.name === 'AbortError') {
      throw new DOMException('The request timed out', 'TimeoutError');
    }
    throw e;
  } finally {
    clearTimeout(timer);
    external?.removeEventListener('abort', onAbort);
  }
}
