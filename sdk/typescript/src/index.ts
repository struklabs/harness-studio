// Client of the Harness Studio Control API. `schema.ts` is generated from the API's OpenAPI
// document (`node sdk/generate.mjs`); requests go through openapi-fetch, so every path, query,
// body and response is typed from it.
import createFetchClient, { type Client, type ClientOptions } from 'openapi-fetch';
import type { components, paths } from './schema.js';

export type * from './schema.js';

/** Prefix of every Control API path. */
export const BASE_PATH = '/api/harness/v1';
/** Where `harness serve` and the app listen by default. */
export const DEFAULT_BASE_URL = 'http://127.0.0.1:11500';

export type HarnessClient = Client<paths>;

export interface HarnessClientOptions extends Omit<ClientOptions, 'baseUrl'> {
  /** Origin of the Control API (default `http://127.0.0.1:11500`), without `/api/harness/v1`. */
  baseUrl?: string;
  /** Session token (`<data dir>/api-token`) or a named token. Sent as `Authorization: Bearer`. */
  token?: string;
}

/**
 * A typed client: `client.GET('/api/harness/v1/models')` returns `{ data, error, response }`.
 * Wrap a call in {@link unwrap} to get the data or a thrown {@link HarnessApiError}.
 */
export function createHarnessClient(options: HarnessClientOptions = {}): HarnessClient {
  const { baseUrl = DEFAULT_BASE_URL, token, ...rest } = options;
  const client = createFetchClient<paths>({ ...rest, baseUrl: baseUrl.replace(/\/+$/, '') });
  if (token) {
    client.use({
      onRequest({ request }) {
        request.headers.set('Authorization', `Bearer ${token}`);
        return request;
      },
    });
  }
  return client;
}

type ErrorDetail = components['schemas']['ErrorDetail'];

/**
 * An error answer of the API: `code` is stable (`api.unauthorized`, `api.not_found`...),
 * `params` fills the placeholders of the localized message `message_key`.
 */
export class HarnessApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly messageKey: string;
  readonly params: Record<string, unknown>;

  constructor(status: number, detail: Partial<ErrorDetail> | undefined, fallback?: string) {
    const code = detail?.code ?? `http.${status}`;
    const params = (detail?.params ?? {}) as Record<string, unknown>;
    const extra = Object.entries(params)
      .map(([k, v]) => `${k}=${typeof v === 'string' ? v : JSON.stringify(v)}`)
      .join(', ');
    super(`${status} ${code}${extra ? ` (${extra})` : ''}${!detail && fallback ? `: ${fallback}` : ''}`);
    this.name = 'HarnessApiError';
    this.status = status;
    this.code = code;
    this.messageKey = detail?.message_key ?? code;
    this.params = params;
  }
}

interface FetchResult<T> {
  data?: T;
  error?: unknown;
  response: Response;
}

/** The data of a successful call; a {@link HarnessApiError} otherwise. */
export async function unwrap<T>(call: Promise<FetchResult<T>> | FetchResult<T>): Promise<T> {
  const { data, error, response } = await call;
  if (response.ok) return data as T;
  const detail =
    error && typeof error === 'object' && 'error' in error
      ? ((error as { error?: Partial<ErrorDetail> }).error ?? undefined)
      : undefined;
  throw new HarnessApiError(response.status, detail, typeof error === 'string' ? error : undefined);
}

/** One server-sent event: its `event:` name and `data:` text. */
export interface ServerSentEvent {
  event: string;
  data: string;
}

/** Reads a `text/event-stream` body (`/events`, `/servers/{id}/logs/stream`) event by event. */
export async function* readServerSentEvents(response: Response): AsyncGenerator<ServerSentEvent> {
  if (!response.body) return;
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = '';
  let event = 'message';
  let data: string[] = [];
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      buffer += value;
      let nl: number;
      while ((nl = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, nl).replace(/\r$/, '');
        buffer = buffer.slice(nl + 1);
        if (line === '') {
          if (data.length) yield { event, data: data.join('\n') };
          event = 'message';
          data = [];
        } else if (line.startsWith('event:')) {
          event = line.slice(6).trim();
        } else if (line.startsWith('data:')) {
          data.push(line.slice(5).replace(/^ /, ''));
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

export type CoreEvent = components['schemas']['CoreEvent'];

/**
 * The core event bus (`GET /events`): server status, download and benchmark progress, as
 * `{ type, event }`. `type` is `lagged` (with the number of skipped events) when the client
 * fell behind. Stop it with `signal`.
 */
export async function* streamEvents(
  client: HarnessClient,
  options: { types?: string[]; signal?: AbortSignal } = {},
): AsyncGenerator<{ type: string; event: CoreEvent | number }> {
  const { response } = await client.GET('/api/harness/v1/events', {
    params: { query: options.types?.length ? { types: options.types.join(',') } : {} },
    parseAs: 'stream',
    signal: options.signal,
  });
  if (!response.ok) await unwrap({ response, error: await response.json().catch(() => undefined) });
  for await (const e of readServerSentEvents(response)) {
    yield { type: e.event, event: JSON.parse(e.data) as CoreEvent | number };
  }
}
