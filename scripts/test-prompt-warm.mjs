import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isLoopbackUrl, warmBody, warmKey } from '../src/lib/components/playground/promptWarm.ts';
import { withToolGuidance } from '../src/lib/components/playground/toolGuidance.ts';

const base = { conversation: null, model: 'qwen', turns: 0, tools: 13, documents: 0, localModel: true, loaded: true, endpoint: 'http://127.0.0.1:11435/v1' };

test('only a new conversation on a loaded local model over loopback is warmed', () => {
  assert.equal(warmKey(base), 'new\u0000qwen');
  assert.equal(warmKey({ ...base, conversation: 'c1' }), 'c1\u0000qwen');
  for (const change of [{ turns: 1 }, { tools: 0 }, { documents: 1 }, { localModel: false }, { loaded: false }, { model: '' },
    { endpoint: 'https://api.example.com/v1' }, { endpoint: 'http://192.168.1.5:11435/v1' }]) {
    assert.equal(warmKey({ ...base, ...change }), null, JSON.stringify(change));
  }
  assert.ok(isLoopbackUrl('http://localhost:8080/v1') && isLoopbackUrl('http://[::1]:8080/v1'));
  assert.ok(!isLoopbackUrl('not a url'));
});

test('the warm-up starts with the same messages, tools and options as the first turn', () => {
  const tools = [{ type: 'function', function: { name: 'web_search', description: 'Search.', parameters: { type: 'object' } } }];
  const system = [{ role: 'system', content: 'Be brief.' }];
  const first = { messages: withToolGuidance([...system, { role: 'user', content: 'Hi' }], tools), tools, model: 'qwen', chat_template_kwargs: { enable_thinking: false } };
  const warm = warmBody(withToolGuidance(system, tools), tools, 'qwen', { chat_template_kwargs: { enable_thinking: false }, stream_options: { include_usage: true }, logprobs: true, max_tokens: 512 });
  // Every message before the user's is the same, byte for byte; so are tools and template options.
  assert.deepEqual(warm.messages.slice(0, -1), first.messages.slice(0, -1));
  assert.deepEqual(warm.messages.at(-1), { role: 'user', content: '' });
  assert.equal(JSON.stringify(warm.tools), JSON.stringify(first.tools));
  assert.deepEqual(warm.chat_template_kwargs, first.chat_template_kwargs);
  assert.equal(warm.max_tokens, 1);
  assert.equal(warm.stream, false);
  assert.ok(!('stream_options' in warm) && !('logprobs' in warm));
});
