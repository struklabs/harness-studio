// The mobile client's pure parts: SSE framing, provider rules (shared with the desktop's
// harness-core::cloud), placement headers and scopes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { SseParser } from '../src/lib/sse.ts';
import { chatRequest, cost, isLocalHost, mergeUsage, parseModels, parseQuota, statusError, streamDelta, validBaseUrl } from '../src/lib/providers.ts';
import { accountFor, can, placementFromHeaders, serverState, type HarnessConnection } from '../src/lib/harness.ts';

test('SSE events survive chunks split anywhere', () => {
  const p = new SseParser();
  const text = 'data: {"a":1}\r\n\r\nevent: message_stop\ndata: {}\n\n: ping\n\ndata: [DONE]';
  const events = [];
  for (const ch of text) events.push(...p.push(ch));
  events.push(...p.flush());
  assert.deepEqual(events, [
    { event: null, data: '{"a":1}' },
    { event: 'message_stop', data: '{}' },
    { event: null, data: '[DONE]' },
  ]);
});

test('OpenAI, Gemini and Anthropic model lists keep chat models only', () => {
  const openai = parseModels('openai', { data: [{ id: 'gpt-5' }, { id: 'text-embedding-3' }, { id: 'whisper-1' }, { id: 'gpt-5' }] });
  assert.deepEqual(openai.map((m) => m.id), ['gpt-5']);
  const gemini = parseModels('gemini', { data: [{ id: 'models/gemini-3-flash' }, { id: 'models/aqa' }] });
  assert.deepEqual(gemini.map((m) => m.id), ['gemini-3-flash']);
  const anthropic = parseModels('anthropic', { data: [{ id: 'claude-x', display_name: 'Claude X', max_input_tokens: 200000 }] });
  assert.equal(anthropic[0].displayName, 'Claude X');
  assert.equal(anthropic[0].contextLength, 200000);
  assert.throws(() => parseModels('openai', { nope: [] }));
});

test('OpenRouter models carry prices and capabilities, and credits parse', () => {
  const [m] = parseModels('openrouter', {
    data: [{ id: 'x/y', name: 'Y', context_length: 1000, pricing: { prompt: '0.000001', completion: '0.000002' }, supported_parameters: ['tools'], architecture: { input_modalities: ['text', 'image'] } }],
  });
  assert.deepEqual([m.inputPrice, m.outputPrice, m.tools, m.vision, m.displayName], [1, 2, true, true, 'Y']);
  assert.equal(cost(m, { promptTokens: 1000, completionTokens: 500 }), 0.002);
  assert.deepEqual(parseQuota({ data: { limit: 10, usage: 2.5, limit_remaining: 7.5 } }), { limit: 10, usage: 2.5, remaining: 7.5 });
  assert.equal(parseQuota({ data: {} }), null);
});

test('chat requests and stream deltas per provider', () => {
  const a = chatRequest('anthropic', 'claude-x', [{ role: 'system', content: 'Be brief.' }, { role: 'user', content: 'Hi' }]);
  assert.equal(a.path, '/messages');
  assert.deepEqual(JSON.parse(a.body), { model: 'claude-x', max_tokens: 2048, stream: true, messages: [{ role: 'user', content: 'Hi' }], system: 'Be brief.' });
  const o = chatRequest('openai', 'gpt', [{ role: 'user', content: 'Hi' }]);
  assert.equal(o.path, '/chat/completions');
  assert.equal(JSON.parse(o.body).stream, true);

  assert.equal(streamDelta('harness', null, '{"choices":[{"delta":{"content":"Hel"}}]}').text, 'Hel');
  assert.deepEqual(streamDelta('openai', null, '{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4}}').usage, { promptTokens: 3, completionTokens: 4 });
  assert.equal(streamDelta('openai', null, '[DONE]').done, true);
  assert.equal(streamDelta('anthropic', 'content_block_delta', '{"delta":{"type":"text_delta","text":"lo"}}').text, 'lo');
  const start = streamDelta('anthropic', 'message_start', '{"message":{"usage":{"input_tokens":9,"output_tokens":1}}}').usage;
  const end = streamDelta('anthropic', 'message_delta', '{"usage":{"output_tokens":12}}').usage;
  assert.deepEqual(mergeUsage(start, end), { promptTokens: 9, completionTokens: 12 });
  assert.equal(streamDelta('openai', null, '{"error":{"message":"quota exceeded"}}').error, 'quota exceeded');
  assert.equal(statusError(429), 'mobile.err.rate_limited');
  assert.equal(statusError(401), 'mobile.err.unauthorized');
});

test('plain http only to local addresses', () => {
  for (const ok of ['http://192.168.1.5:11435', 'http://10.0.0.2', 'http://localhost:12440', 'http://mac.local:11435', 'https://api.openai.com/v1/']) {
    assert.ok(validBaseUrl(ok), ok);
  }
  assert.equal(validBaseUrl('https://api.openai.com/v1/'), 'https://api.openai.com/v1');
  for (const bad of ['http://example.com', 'http://8.8.8.8', 'ftp://x', 'https://u:p@x.com', 'https://x.com/?k=1', 'nonsense']) {
    assert.equal(validBaseUrl(bad), null, bad);
  }
  assert.ok(isLocalHost('172.20.1.1'));
  assert.ok(!isLocalHost('172.32.1.1'));
  assert.ok(!isLocalHost('fdomain.com'));
});

test('placement from gateway headers, scopes and token accounts', () => {
  const c: HarnessConnection = { id: 'h', name: 'Home Mac', gatewayUrl: 'http://192.168.1.5:11435', apiUrl: 'http://192.168.1.5:11500', account: 'harness:h' };
  const p = placementFromHeaders(c, [['X-Harness-Model', 'qwen'], ['x-harness-server', 'srv-1'], ['x-harness-queue-ms', '12'], ['x-harness-load-ms', '']]);
  assert.deepEqual(p, { where: 'harness', provider: 'Home Mac', model: 'qwen', server: 'srv-1', device: null, backend: null, queueMs: 12, loadMs: null });
  assert.equal(accountFor(c, 'http://192.168.1.5:11500/api/harness/v1/jobs'), 'harness:h.api');
  assert.equal(accountFor(c, 'http://192.168.1.5:11435/v1/models'), 'harness:h');
  assert.ok(can({ tokenName: 't', scopes: ['admin'], version: null }, 'servers:control'));
  assert.ok(!can({ tokenName: 't', scopes: ['read', 'inference'], version: null }, 'admin'));
  assert.ok(!can(null, 'read'));
  assert.equal(serverState('Running'), 'running');
  assert.equal(serverState({ Failed: { reason: 'x' } }), 'failed');
});
