// A Harness instance at home or on a server: its gateway (chat, OpenAI-compatible) and its
// Control API (jobs, models, modules, servers, devices, permissions), both with one scoped
// named token. What the token may do decides which screens offer which actions.
import { http, header, type HttpResponse } from './native.ts';
import { errorMessage, statusError } from './providers.ts';

export interface HarnessConnection {
  id: string;
  name: string;
  /** `http://192.168.1.5:11435` (no `/v1`). */
  gatewayUrl: string;
  /** `http://192.168.1.5:11500` (no `/api/harness/v1`); null when the Control API is off. */
  apiUrl: string | null;
  /** Keychain account of the token. */
  account: string;
}

/** Turns a pairing into a saved token. Today: a named token the user typed; the pairing flow
 * (a one-time code exchanged for a scoped token, `feat/remote-pairing-devices`) implements the
 * same interface. */
export interface Pairing {
  readonly method: 'token' | 'code';
  /** Stores the token for both URLs' origins and returns what it may do. */
  pair(connection: HarnessConnection, secret: string): Promise<Identity>;
}

export interface Identity {
  tokenName: string | null;
  scopes: string[];
  version: string | null;
}

export class HarnessError extends Error {
  key: string;
  status: number | null;
  detail: string | null;

  constructor(key: string, status: number | null = null, detail: string | null = null) {
    super(detail ?? key);
    this.key = key;
    this.status = status;
    this.detail = detail;
  }
}

export function apiBase(c: HarnessConnection): string | null {
  return c.apiUrl ? `${c.apiUrl}/api/harness/v1` : null;
}

/** The keychain entry for a URL: a token is bound to one origin, so a Control API on another
 * origin than the gateway has its own copy (`<account>.api`). */
export function accountFor(c: HarnessConnection, url: string): string {
  const origin = new URL(url).origin;
  return c.apiUrl && origin !== new URL(c.gatewayUrl).origin && origin === new URL(c.apiUrl).origin ? `${c.account}.api` : c.account;
}

async function call(c: HarnessConnection, url: string, method: 'GET' | 'POST' | 'PUT' = 'GET', body?: unknown): Promise<HttpResponse> {
  const res = await http({
    method,
    url,
    headers: body === undefined ? [] : [['content-type', 'application/json']],
    body: body === undefined ? undefined : JSON.stringify(body),
    secret: { account: accountFor(c, url), scheme: 'bearer' },
  }).catch((e: unknown) => {
    throw new HarnessError(typeof e === 'string' ? e : 'mobile.transport');
  });
  if (res.status < 200 || res.status >= 300) {
    throw new HarnessError(statusError(res.status), res.status, errorMessage(res.body));
  }
  return res;
}

async function json<T>(c: HarnessConnection, url: string, method: 'GET' | 'POST' | 'PUT' = 'GET', body?: unknown): Promise<T> {
  const res = await call(c, url, method, body);
  return (res.body ? JSON.parse(res.body) : null) as T;
}

function api(c: HarnessConnection, path: string): string {
  const base = apiBase(c);
  if (!base) throw new HarnessError('mobile.err.no_api');
  return `${base}${path}`;
}

/** The token's name and scopes, from the gateway. */
export async function whoami(c: HarnessConnection): Promise<Identity> {
  const v = await json<{ token_name: string | null; scopes: string[]; harness_version: string }>(c, `${c.gatewayUrl}/v1/harness/whoami`);
  return { tokenName: v.token_name, scopes: v.scopes ?? [], version: v.harness_version ?? null };
}

/** May the token do `scope`? `admin` may do everything. */
export function can(identity: Identity | null, scope: string): boolean {
  return !!identity && (identity.scopes.includes(scope) || identity.scopes.includes('admin'));
}

export interface GatewayModel {
  id: string;
  owned_by?: string;
}

export async function gatewayModels(c: HarnessConnection): Promise<GatewayModel[]> {
  const v = await json<{ data: GatewayModel[] }>(c, `${c.gatewayUrl}/v1/models`);
  return v.data ?? [];
}

export interface Job {
  id: number;
  label: string;
  priority: 'interactive' | 'agent' | 'background';
  status: 'queued' | 'loading' | 'running' | 'cancelling';
  memory_bytes: number | null;
  queue_wait_ms: number;
  load_ms: number | null;
}

export const jobs = (c: HarnessConnection) => json<Job[]>(c, api(c, '/jobs'));

export interface ServerView {
  id: string;
  name: string;
  status: unknown;
  endpoint: string | null;
  served_alias: string | null;
  definition_id: string | null;
  /** The running process: its engine and launch arguments. */
  active?: { engine_family?: string | null; serve_mode?: string | null; [key: string]: unknown } | null;
  profile?: { config?: Record<string, unknown> } | null;
}

export const servers = (c: HarnessConnection) => json<ServerView[]>(c, api(c, '/servers'));
export const server = (c: HarnessConnection, id: string) => json<ServerView>(c, api(c, `/servers/${encodeURIComponent(id)}`));
/** Loads a model by its served alias (a new server on a free port) or stops its servers. */
export const loadModel = (c: HarnessConnection, alias: string) => call(c, api(c, '/load'), 'POST', { alias });
export const unloadModel = (c: HarnessConnection, alias: string) => call(c, api(c, '/unload'), 'POST', { alias });
export const startServer = (c: HarnessConnection, id: string) => call(c, api(c, `/servers/${encodeURIComponent(id)}/start`), 'POST', {});
export const stopServer = (c: HarnessConnection, id: string) => call(c, api(c, `/servers/${encodeURIComponent(id)}/stop`), 'POST', {});

/** `running`, `stopped`, `starting`, … or `failed` (the API's status is a string or `{failed: …}`). */
export function serverState(status: unknown): string {
  if (typeof status === 'string') return status.toLowerCase();
  if (status && typeof status === 'object') return Object.keys(status)[0]?.toLowerCase() ?? 'unknown';
  return 'unknown';
}

export interface Device {
  index: number;
  name: string;
  backend: string | null;
  memory_total_bytes: number | null;
  memory_free_bytes: number | null;
}

/** The accelerators llama.cpp sees (`/hardware/devices`: Metal, CUDA, Vulkan, …) and the main
 * one. Host-side helpers without memory (BLAS / Accelerate) are left out. */
export async function devices(c: HarnessConnection): Promise<{ devices: Device[]; main: number | null }> {
  const v = await json<{ devices?: Record<string, any>[]; main_device?: number | null }>(c, api(c, '/hardware/devices'));
  const list = (v.devices ?? [])
    .filter((d) => (d.total_bytes ?? 0) > 0)
    .map((d, i) => ({
      index: typeof d.index === 'number' ? d.index : i,
      name: String(d.name ?? d.id ?? `#${i}`),
      backend: typeof d.backend === 'string' ? d.backend : null,
      memory_total_bytes: d.total_bytes ?? null,
      memory_free_bytes: d.available_bytes ?? d.free_bytes ?? null,
    }));
  return { devices: list, main: v.main_device ?? null };
}

export interface ModuleView {
  manifest: { id: string; version: string; name: Record<string, string> | string; planned?: boolean };
  state: { status: string; installed_version: string | null; error: string | null };
  update_available: boolean;
}

export const modules = (c: HarnessConnection) => json<ModuleView[]>(c, api(c, '/modules'));
export const moduleAction = (c: HarnessConnection, id: string, action: 'install' | 'update' | 'repair' | 'enable' | 'disable') =>
  call(c, api(c, `/modules/${encodeURIComponent(id)}/action`), 'POST', { action });

export interface Subject {
  kind: string;
  id: string;
  label: string;
  available: boolean;
  trust: string | null;
  inherent: string;
}

/** MCP servers, agent runtimes and the training backend that ask to be trusted. */
export const permissions = (c: HarnessConnection) => json<Subject[]>(c, api(c, '/permissions'));
export const setTrust = (c: HarnessConnection, s: Subject, trust: string | null) =>
  call(c, api(c, `/permissions/${encodeURIComponent(s.kind)}/${encodeURIComponent(s.id)}/trust`), 'PUT', { trust });

export interface ModelStats {
  alias: string;
  requests: number;
  errors: number;
  avg_gen_tps: number | null;
  ttft_p50_ms: number | null;
  latency_p50_ms: number | null;
  vram_peak_bytes: number | null;
}

export const analytics = (c: HarnessConnection) => json<{ totals: ModelStats; models: ModelStats[] }>(c, api(c, '/analytics/summary'));

/** Where a gateway answer ran: the headers of the answer, joined with the server and the
 * device it uses (when the token may read the Control API). */
export interface Placement {
  /** A Harness, a provider's cloud, or a custom endpoint the user named. */
  where: 'harness' | 'provider' | 'endpoint';
  provider: string;
  model: string | null;
  server: string | null;
  device: string | null;
  backend: string | null;
  queueMs: number | null;
  loadMs: number | null;
}

export function placementFromHeaders(connection: HarnessConnection, headers: [string, string][]): Placement {
  const num = (name: string) => {
    const v = header(headers, name);
    return v === null || v === '' || Number.isNaN(Number(v)) ? null : Number(v);
  };
  return {
    where: 'harness',
    provider: connection.name,
    model: header(headers, 'x-harness-model'),
    server: header(headers, 'x-harness-server'),
    device: null,
    backend: null,
    queueMs: num('x-harness-queue-ms'),
    loadMs: num('x-harness-load-ms'),
  };
}

/** Fills in the device: the server's engine and the GPU it uses. A server launched with no GPU
 * layers runs on the CPU; otherwise llama.cpp's main device. Best effort; null when the token
 * may not read the Control API. */
export async function resolveDevice(c: HarnessConnection, p: Placement): Promise<Placement> {
  if (!p.server || !c.apiUrl) return p;
  try {
    const [view, hw] = await Promise.all([server(c, p.server), devices(c)]);
    const config = (view.profile?.config ?? {}) as Record<string, unknown>;
    const ngl = Number(config.n_gpu_layers ?? config.ngl ?? NaN);
    const main = Number(config.main_gpu ?? hw.main ?? 0);
    const device = ngl === 0 ? null : (hw.devices.find((d) => d.index === main) ?? hw.devices[0] ?? null);
    const engine = view.active?.engine_family ?? null;
    return {
      ...p,
      device: ngl === 0 ? 'CPU' : (device?.name ?? null),
      backend: [engine, device?.backend].filter(Boolean).join(' · ') || null,
    };
  } catch {
    return p;
  }
}
