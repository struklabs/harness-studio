import test from 'node:test';
import assert from 'node:assert/strict';
import { browserSession, browserLogin, browserLogout, control, ControlError, isDesktop } from '../src/lib/transport.ts';

test('browser transport keeps credentials in cookies and sends CSRF only on writes', async () => {
  const calls = [];
  const expired = [];
  const originalFetch = globalThis.fetch;
  const originalWindow = globalThis.window;
  globalThis.window = { dispatchEvent: event => expired.push(event.type) };
  globalThis.fetch = async (url, options) => {
    calls.push({ url, ...options });
    if (url.endsWith('/browser/session')) return Response.json({ setup_required: false, authenticated: true, csrf: 'csrf-value' });
    if (url.endsWith('/browser/login')) return Response.json({ csrf: 'rotated-csrf' });
    if (url.endsWith('/expired')) return Response.json({ error: { code: 'api.unauthorized' } }, { status: 401 });
    if (url.endsWith('/limited')) return Response.json({ error: { code: 'browser.rate_limit' } }, { status: 429, headers: { 'Retry-After': '8' } });
    return new Response(null, { status: 204 });
  };
  try {
    assert.equal(isDesktop(), false);
    window.__TAURI_INTERNALS__ = {};
    assert.equal(isDesktop(), true);
    delete window.__TAURI_INTERNALS__;
    await browserSession();
    await control('/servers');
    assert.equal(calls.at(-1).headers['X-Harness-CSRF'], undefined);
    await control('/servers/a/start', 'POST', {});
    assert.equal(calls.at(-1).headers['X-Harness-CSRF'], 'csrf-value');
    assert.equal(calls.at(-1).headers.Authorization, undefined);
    assert.equal(calls.at(-1).credentials, 'same-origin');
    assert.equal(calls.at(-1).cache, 'no-store');
    await browserLogin('password');
    await control('/tokens', 'POST', { name: 'name', scopes: ['read'] });
    assert.equal(calls.at(-1).headers['X-Harness-CSRF'], 'rotated-csrf');
    await assert.rejects(control('/limited'), e => e instanceof ControlError && e.retryAfter === 8);
    await assert.rejects(control('/expired'), e => e.status === 401);
    assert.deepEqual(expired, ['harness-session-expired']);
    await browserSession();
    await browserLogout(true);
    assert.equal(calls.at(-1).url, '/api/harness/v1/browser/sessions');
    assert.equal(calls.at(-1).method, 'DELETE');
  } finally {
    globalThis.fetch = originalFetch;
    globalThis.window = originalWindow;
  }
});
