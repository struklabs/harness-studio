// The one boundary to the native side (mobile/src-tauri): HTTP and the keychain. Everything
// else in the UI is plain TypeScript, so it can move to another shell (Capacitor) by replacing
// this file.
import { Channel, invoke } from '@tauri-apps/api/core';

export type Scheme = 'bearer' | 'x_api_key';

export interface HttpRequest {
  method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  url: string;
  headers?: [string, string][];
  body?: string;
  /** A keychain account whose key authenticates the request (never visible to the page). */
  secret?: { account: string; scheme: Scheme };
  timeoutMs?: number;
}

export interface HttpResponse {
  status: number;
  headers: [string, string][];
  body: string;
}

export type StreamEvent =
  | { event: 'head'; status: number; headers: [string, string][] }
  | { event: 'data'; text: string }
  | { event: 'end' }
  | { event: 'error'; message: string };

function wire(req: HttpRequest) {
  return {
    method: req.method,
    url: req.url,
    headers: req.headers ?? [],
    body: req.body ?? null,
    secret: req.secret ?? null,
    timeout_ms: req.timeoutMs ?? null,
  };
}

export function http(req: HttpRequest): Promise<HttpResponse> {
  return invoke<HttpResponse>('http_request', { request: wire(req) });
}

let nextStream = 1;

/** Streams a request's answer to `onEvent`; the returned function cancels it. */
export async function httpStream(req: HttpRequest, onEvent: (e: StreamEvent) => void): Promise<() => void> {
  const id = nextStream++;
  const channel = new Channel<StreamEvent>();
  channel.onmessage = onEvent;
  await invoke('http_stream', { id, request: wire(req), onEvent: channel });
  return () => void invoke('http_cancel', { id });
}

/** Saves `key` in the keychain for requests to `url`'s origin only. */
export function saveSecret(account: string, url: string, key: string): Promise<void> {
  return invoke('secret_set', { account, url, key });
}

export function hasSecret(account: string): Promise<boolean> {
  return invoke<boolean>('secret_has', { account });
}

export function deleteSecret(account: string): Promise<void> {
  return invoke('secret_delete', { account });
}

export function header(headers: [string, string][], name: string): string | null {
  const lower = name.toLowerCase();
  return headers.find(([k]) => k.toLowerCase() === lower)?.[1] ?? null;
}
