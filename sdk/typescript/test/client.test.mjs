// Against a real `harness serve` on a throwaway data directory:
//   node ../test-server.mjs -- npm test      (from sdk/typescript)
// Without HARNESS_SDK_TEST_DATA_DIR the tests are skipped.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { HarnessApiError, createHarnessClient, readServerSentEvents, streamEvents, unwrap } from '../dist/index.js';
import { connect, discover } from '../dist/node.js';

const dataDir = process.env.HARNESS_SDK_TEST_DATA_DIR;
const skip = dataDir ? false : 'HARNESS_SDK_TEST_DATA_DIR not set (run through sdk/test-server.mjs)';
const API = '/api/harness/v1';

/** The error a call throws through `unwrap`.
 * @param {Promise<any>} call
 * @returns {Promise<HarnessApiError>}
 */
async function failure(call) {
  try {
    await unwrap(call);
  } catch (e) {
    assert.ok(e instanceof HarnessApiError, String(e));
    return e;
  }
  assert.fail('the call succeeded');
}

test('discover reads instance.json and the session token', { skip }, () => {
  const found = discover(dataDir);
  assert.match(found.baseUrl, /^http:\/\/127\.0\.0\.1:\d+$/);
  assert.ok(found.token.length >= 32);
});

test('health is public, status needs a token', { skip }, async () => {
  const { baseUrl } = discover(dataDir);
  const anonymous = createHarnessClient({ baseUrl });
  const health = await unwrap(anonymous.GET(`${API}/health`));
  assert.equal(health.status, 'ok');
  assert.ok(health.version);

  const e = await failure(anonymous.GET(`${API}/status`));
  assert.equal(e.status, 401);
  assert.equal(e.code, 'api.unauthorized');

  const client = connect({ dataDir });
  const status = await unwrap(client.GET(`${API}/status`));
  assert.equal(status.version, health.version);
  assert.equal(status.servers.running, 0);
});

test('typed lists and a structured 404', { skip }, async () => {
  const client = connect({ dataDir });
  const library = await unwrap(client.GET(`${API}/models`));
  assert.deepEqual(library.entries, []);
  assert.ok(Array.isArray(await unwrap(client.GET(`${API}/definitions`))));
  const e = await failure(client.GET(`${API}/definitions/{id}`, { params: { path: { id: 'nothing-here' } } }));
  assert.equal(e.status, 404);
  assert.equal(e.messageKey, e.code);
});

test('query parameters reach the server', { skip }, async () => {
  const client = connect({ dataDir });
  const summary = await unwrap(client.GET(`${API}/analytics/summary`, { params: { query: { range: '24h' } } }));
  assert.ok(Array.isArray(summary.models));
  const e = await failure(client.GET(`${API}/analytics/summary`, { params: { query: { range: '12w' } } }));
  assert.equal(e.status, 400);
  assert.equal(e.code, 'api.invalid_range');
  const page = await unwrap(client.GET(`${API}/analytics/requests`, { params: { query: { limit: 1 } } }));
  assert.equal(page.total, 0);
});

test('named tokens: scopes are enforced, revoking ends access', { skip }, async () => {
  const admin = connect({ dataDir });
  const created = await unwrap(admin.POST(`${API}/tokens`, { body: { name: 'sdk-test-reader', scopes: ['read'] } }));
  assert.ok(created.token);
  const { baseUrl } = discover(dataDir);
  const reader = createHarnessClient({ baseUrl, token: created.token });
  await unwrap(reader.GET(`${API}/models`));
  const denied = await failure(reader.POST(`${API}/tokens`, { body: { name: 'nope', scopes: ['read'] } }));
  assert.equal(denied.status, 403);
  assert.equal(denied.code, 'api.insufficient_scope');

  await unwrap(admin.DELETE(`${API}/tokens/{id}`, { params: { path: { id: created.info.id } } }));
  const gone = await failure(reader.GET(`${API}/models`));
  assert.equal(gone.status, 401);
});

test('the event stream opens and stops on abort', { skip }, async () => {
  const client = connect({ dataDir });
  const controller = new AbortController();
  setTimeout(() => controller.abort(), 300);
  await assert.rejects(
    (async () => {
      for await (const _ of streamEvents(client, { types: ['server_status'], signal: controller.signal })) {
        // No server changes state here.
      }
    })(),
    (/** @type {any} */ e) => e.name === 'AbortError',
  );
});

test('readServerSentEvents splits events and multi-line data', async () => {
  const body = 'event: download\ndata: {"a":1}\n\n: keep-alive\n\nevent: lagged\ndata: 3\r\n\r\ndata: one\ndata: two\n\n';
  const events = [];
  for await (const e of readServerSentEvents(new Response(body))) events.push(e);
  assert.deepEqual(events, [
    { event: 'download', data: '{"a":1}' },
    { event: 'lagged', data: '3' },
    { event: 'message', data: 'one\ntwo' },
  ]);
});
