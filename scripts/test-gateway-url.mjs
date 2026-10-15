import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runningGatewayEndpoint } from '../src/lib/gateway-url.ts';

const endpoint = (host, port) => ({
  kind: 'managed',
  host_url: `http://${host}:${port}`,
  base_url: `http://${host}:${port}/v1`,
  requires_token: false,
});
const overview = (running, live) => ({
  settings: { enabled: true, host: '127.0.0.1', port: 11435 },
  status: { running, endpoint: live },
});

test('unknown, disabled and failed gateways have no request address, even with a stale endpoint', () => {
  assert.equal(runningGatewayEndpoint(null), null);
  assert.equal(runningGatewayEndpoint(overview(false, null)), null);
  assert.equal(runningGatewayEndpoint(overview(true, null)), null);
  const disabled = overview(false, endpoint('127.0.0.1', 11436));
  disabled.settings.enabled = false;
  assert.equal(runningGatewayEndpoint(disabled), null);
  assert.equal(runningGatewayEndpoint({ ...disabled, settings: { ...disabled.settings, enabled: true } }), null);
});

test('the bound address wins over configured host and port', () => {
  for (const host of ['127.0.0.1', '[::1]', '192.168.1.20']) {
    const live = endpoint(host, 11436);
    assert.deepEqual(runningGatewayEndpoint(overview(true, live)), live);
  }
});

test('status changes replace the address and stopping removes it', () => {
  const gateway = overview(true, endpoint('127.0.0.1', 11436));
  assert.equal(runningGatewayEndpoint(gateway).base_url, 'http://127.0.0.1:11436/v1');
  gateway.status.endpoint = endpoint('[::1]', 12001);
  assert.equal(runningGatewayEndpoint(gateway).base_url, 'http://[::1]:12001/v1');
  gateway.status.running = false;
  assert.equal(runningGatewayEndpoint(gateway), null);
});
