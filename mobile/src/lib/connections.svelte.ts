// The phone's connections: Harness instances and BYOK providers. Only their names and URLs are
// kept in the WebView's storage; tokens and keys go to the keychain (native.ts) and are never
// read back.
import { deleteSecret, hasSecret, http, saveSecret } from './native.ts';
import { HarnessError, whoami, type HarnessConnection, type Identity, type Pairing } from './harness.ts';
import {
  defaultBaseUrl,
  errorMessage,
  keyRequired,
  modelsUrl,
  parseModels,
  parseQuota,
  providerHeaders,
  statusError,
  validBaseUrl,
  type ProviderKind,
  type ProviderModel,
  type Quota,
} from './providers.ts';

export interface ProviderConnection {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  account: string;
}

export type ProviderState = 'online' | 'offline' | 'unauthorized' | 'key_missing' | 'checking';

export interface ProviderStatus {
  state: ProviderState;
  models: ProviderModel[];
  quota: Quota | null;
  error: string | null;
  detail: string | null;
}

const STORAGE = 'harness_mobile_connections';

interface Saved {
  harness: HarnessConnection[];
  providers: ProviderConnection[];
}

function load(): Saved {
  try {
    const v = JSON.parse(localStorage.getItem(STORAGE) ?? '{}');
    return { harness: Array.isArray(v.harness) ? v.harness : [], providers: Array.isArray(v.providers) ? v.providers : [] };
  } catch {
    return { harness: [], providers: [] };
  }
}

function newId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
}

/** A URL a user typed: scheme added when missing, no trailing slash or `/v1`. */
export function normalizeUrl(raw: string, strip: RegExp = /\/v1$/): string | null {
  let text = raw.trim();
  if (!text) return null;
  if (!/^[a-z]+:\/\//i.test(text)) text = `http://${text}`;
  const url = validBaseUrl(text);
  return url ? url.replace(strip, '') : null;
}

/** Pairing with a named token the user created on the computer (`harness api-token create`). */
export const tokenPairing: Pairing = {
  method: 'token',
  async pair(connection, secret) {
    await saveSecret(connection.account, connection.gatewayUrl, secret);
    if (connection.apiUrl && new URL(connection.apiUrl).origin !== new URL(connection.gatewayUrl).origin) {
      // The same token on the Control API's origin, under its own keychain entry.
      await saveSecret(`${connection.account}.api`, connection.apiUrl, secret);
    }
    return whoami(connection);
  },
};

class Connections {
  harness = $state<HarnessConnection[]>([]);
  providers = $state<ProviderConnection[]>([]);
  identities = $state<Record<string, Identity | null>>({});
  identityErrors = $state<Record<string, string | null>>({});
  status = $state<Record<string, ProviderStatus>>({});

  constructor() {
    const saved = typeof localStorage === 'undefined' ? { harness: [], providers: [] } : load();
    this.harness = saved.harness;
    this.providers = saved.providers;
  }

  private persist() {
    try {
      localStorage.setItem(STORAGE, JSON.stringify({ harness: this.harness, providers: this.providers }));
    } catch {
      // storage unavailable: the connections last for this session
    }
  }

  async addHarness(name: string, gateway: string, api: string, token: string, pairing: Pairing = tokenPairing): Promise<HarnessConnection> {
    const gatewayUrl = normalizeUrl(gateway);
    const apiUrl = api.trim() ? normalizeUrl(api, /\/api\/harness\/v1$/) : null;
    if (!gatewayUrl || (api.trim() && !apiUrl)) throw new HarnessError('mobile.err.url');
    const id = newId('harness');
    const connection: HarnessConnection = { id, name: name.trim() || new URL(gatewayUrl).hostname, gatewayUrl, apiUrl, account: `harness:${id}` };
    try {
      const identity = await pairing.pair(connection, token);
      this.identities[id] = identity;
    } catch (e) {
      await this.forgetSecrets(connection.account);
      throw e;
    }
    this.harness = [...this.harness, connection];
    this.persist();
    return connection;
  }

  async refreshIdentity(c: HarnessConnection) {
    try {
      this.identities[c.id] = await whoami(c);
      this.identityErrors[c.id] = null;
    } catch (e) {
      this.identities[c.id] = null;
      this.identityErrors[c.id] = e instanceof HarnessError ? e.key : 'mobile.transport';
    }
  }

  private async forgetSecrets(account: string) {
    await deleteSecret(account).catch(() => {});
    await deleteSecret(`${account}.api`).catch(() => {});
  }

  async removeHarness(id: string) {
    const c = this.harness.find((h) => h.id === id);
    if (c) await this.forgetSecrets(c.account);
    this.harness = this.harness.filter((h) => h.id !== id);
    delete this.identities[id];
    this.persist();
  }

  async addProvider(kind: ProviderKind, name: string, baseUrl: string, key: string): Promise<ProviderConnection> {
    const base = validBaseUrl(baseUrl.trim() || defaultBaseUrl(kind) || '');
    if (!base) throw new HarnessError('mobile.err.url');
    if (keyRequired(kind) && !key.trim()) throw new HarnessError('mobile.err.key_required');
    const id = newId(kind);
    const provider: ProviderConnection = { id, name: name.trim() || (kind === 'custom' ? new URL(base).hostname : kind), kind, baseUrl: base, account: `provider:${id}` };
    if (key.trim()) await saveSecret(provider.account, base, key);
    this.providers = [...this.providers, provider];
    this.persist();
    await this.refreshProvider(provider);
    return provider;
  }

  async removeProvider(id: string) {
    const p = this.providers.find((x) => x.id === id);
    if (p) await deleteSecret(p.account).catch(() => {});
    this.providers = this.providers.filter((x) => x.id !== id);
    delete this.status[id];
    this.persist();
  }

  /** Model discovery, credits (OpenRouter) and the key's state. */
  async refreshProvider(p: ProviderConnection) {
    this.status[p.id] = { state: 'checking', models: this.status[p.id]?.models ?? [], quota: null, error: null, detail: null };
    const keyed = await hasSecret(p.account).catch(() => false);
    if (keyRequired(p.kind) && !keyed) {
      this.status[p.id] = { state: 'key_missing', models: [], quota: null, error: 'mobile.err.key_required', detail: null };
      return;
    }
    const secret = keyed ? { account: p.account, scheme: p.kind === 'anthropic' ? ('x_api_key' as const) : ('bearer' as const) } : undefined;
    try {
      const res = await http({ method: 'GET', url: modelsUrl(p.kind, p.baseUrl), headers: providerHeaders(p.kind), secret, timeoutMs: 20000 });
      if (res.status < 200 || res.status >= 300) {
        this.status[p.id] = {
          state: res.status === 401 || res.status === 403 ? 'unauthorized' : 'offline',
          models: [],
          quota: null,
          error: statusError(res.status),
          detail: errorMessage(res.body),
        };
        return;
      }
      const models = parseModels(p.kind, JSON.parse(res.body));
      let quota: Quota | null = null;
      if (p.kind === 'openrouter' && secret) {
        const q = await http({ method: 'GET', url: `${p.baseUrl}/key`, secret, timeoutMs: 10000 }).catch(() => null);
        if (q && q.status === 200) quota = parseQuota(JSON.parse(q.body));
      }
      this.status[p.id] = { state: 'online', models, quota, error: null, detail: null };
    } catch (e) {
      this.status[p.id] = { state: 'offline', models: [], quota: null, error: typeof e === 'string' ? e : e instanceof Error ? e.message : 'mobile.transport', detail: null };
    }
  }
}

export const connections = new Connections();
