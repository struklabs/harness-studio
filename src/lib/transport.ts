/** Same-origin Control API transport. Credentials remain in the HttpOnly cookie. */
export const CONTROL_BASE = '/api/harness/v1';
export function isDesktop(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
let csrf: string | null = null;
export interface BrowserSession { setup_required: boolean; authenticated: boolean; csrf: string | null }
export class ControlError extends Error {
  status: number;
  code: string;
  retryAfter: number;
  params: Record<string, string | number>;
  constructor(status: number, code: string, retryAfter: number, params: Record<string, string | number> = {}) {
    super(code); this.status = status; this.code = code; this.retryAfter = retryAfter; this.params = params;
  }
}
export async function control<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (csrf && !['GET', 'HEAD'].includes(method)) headers['X-Harness-CSRF'] = csrf;
  const response = await fetch(`${CONTROL_BASE}${path}`, { method, headers, credentials: 'same-origin', cache: 'no-store', body: body === undefined ? undefined : JSON.stringify(body) });
  if (!response.ok) {
    const detail = await response.json().catch(() => ({}));
    if (response.status === 401 && path !== '/browser/login') {
      csrf = null;
      window.dispatchEvent(new Event('harness-session-expired'));
    }
    throw new ControlError(response.status, detail.error?.code ?? 'browser.request_failed', Number(response.headers.get('Retry-After') ?? 0), detail.error?.params ?? {});
  }
  if (response.status === 204) return undefined as T;
  return response.json();
}
export async function browserSession(): Promise<BrowserSession> {
  const session = await control<BrowserSession>('/browser/session');
  csrf = session.csrf;
  return session;
}
export async function browserLogin(password: string, setup_code?: string): Promise<void> {
  const session = await control<BrowserSession>('/browser/login', 'POST', { password, setup_code });
  csrf = session.csrf;
}
export async function browserLogout(all = false): Promise<void> {
  await control(all ? '/browser/sessions' : '/browser/logout', all ? 'DELETE' : 'POST');
  csrf = null;
}
/** EventSource reconnects with the cookie. A reconnect also refreshes state after missed events. */
export function controlEvents(refresh: () => void, disconnected: () => void): () => void {
  const source = new EventSource(`${CONTROL_BASE}/events`, { withCredentials: true });
  source.onopen = refresh;
  for (const name of ['server_status', 'download', 'download_removed', 'definitions_changed', 'server_failed', 'server_restart', 'lagged']) source.addEventListener(name, refresh);
  source.onerror = disconnected;
  return () => source.close();
}
