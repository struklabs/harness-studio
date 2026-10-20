import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { toolIndex, selectedTools, routeTools, redactDetail } from '../src/lib/components/playground/toolRouting.ts';

const tools = JSON.parse(readFileSync(new URL('../crates/harness-core/src/tools/builtin/llama_tools.json', import.meta.url))).map(t => t.definition);
const signal = () => new AbortController().signal;
const messages = [{ role: 'user', content: 'Read README.md using read_file' }];
const body = { model: 'test', messages, tools, stream: true };
const reply = names => Response.json({ choices: [{ message: { content: JSON.stringify(names) } }] });
const source = readFileSync(new URL('../src/lib/components/playground/chatRequest.ts', import.meta.url), 'utf8')
  .replaceAll("'./toolRouting'", JSON.stringify(new URL('../src/lib/components/playground/toolRouting.ts', import.meta.url).href))
  .replaceAll("'./toolHistory'", JSON.stringify(new URL('../src/lib/components/playground/toolHistory.ts', import.meta.url).href));
const { chatRequest } = await import('data:text/javascript;base64,' + Buffer.from(stripTypeScriptTypes(source)).toString('base64'));

test('discovery sends only the compact index and loads exactly selected schemas', async () => {
  let request;
  const result = await routeTools('http://local/v1', body, {}, signal(), async (_, init) => {
    request = JSON.parse(init.body);
    return reply(['read_file']);
  });
  assert.equal(request.tools, undefined);
  assert.ok(request.messages[0].content.includes(toolIndex(tools)));
  assert.ok(!request.messages[0].content.includes('parameters'));
  assert.deepEqual(result.body.tools, tools.filter(t => t.function.name === 'read_file'));
  assert.equal(result.fallback, false);
  assert.equal(body.tools, tools);
});

test('empty discovery permits a schema-free answer when tool_choice is not forced', async () => {
  const result = await routeTools('local', { ...body, tool_choice: 'auto' }, {}, signal(), async () => reply([]));
  assert.equal(result.body.tools, undefined);
  assert.equal(result.body.tool_choice, undefined);
});

test('empty discovery keeps the full tool list and tool_choice required', async () => {
  const result = await routeTools('local', { ...body, tool_choice: 'required' }, {}, signal(), async () => reply([]));
  assert.deepEqual(result.body.tools, tools);
  assert.equal(result.body.tool_choice, 'required');
  assert.equal(result.fallback, false);
});

test('redactDetail strips credentials and truncates', () => {
  const key = 'sk-abcdefghijklmnop1234';
  const text = `401 Authorization: Bearer ${key} api_key=abc123secret {"token":"zzz999"} https://x/y?key=q1w2e3 ${'A'.repeat(40)}`;
  const out = redactDetail(text);
  for (const leak of [key, 'abc123secret', 'zzz999', 'q1w2e3', 'AAAAAAAAAAAAAAAA']) assert.ok(!out.includes(leak), leak);
  assert.ok(out.includes('401'));
  assert.ok(redactDetail('x '.repeat(5000), 100).length <= 101);
});

test('a forced tool_choice function stays available even when discovery omits it', async () => {
  const tool_choice = { type: 'function', function: { name: 'read_file' } };
  const result = await routeTools('local', { ...body, tool_choice }, {}, signal(), async () => reply([]));
  assert.deepEqual(result.body.tools.map(t => t.function.name), ['read_file']);
  assert.deepEqual(result.body.tool_choice, tool_choice);
});

test('unknown names, malformed responses and failed discovery preserve enabled schemas', async () => {
  assert.throws(() => selectedTools('["invented"]', tools));
  assert.throws(() => selectedTools('{}', tools));
  for (const send of [async () => reply(['invented']), async () => reply({}), async () => new Response('', { status: 500 }), async () => { throw Error('offline'); }]) {
    const result = await routeTools('local', body, {}, signal(), send);
    assert.equal(result.fallback, true);
    assert.deepEqual(result.body.tools, tools);
    assert.ok(result.error);
  }
});

test('malformed discovery preserves enabled schemas for a Turkish request in the actual chat', async () => {
  const sent = [];
  const result = await chatRequest('local', { ...body, messages: [{ role: 'user', content: 'README.md dosyasını oku' }], tool_choice: 'auto' }, {}, signal(), async (_, init) => {
    sent.push(JSON.parse(init.body));
    return sent.length === 1
      ? Response.json({ choices: [{ message: { content: 'not JSON' } }] })
      : Response.json({ choices: [{ message: { content: 'done' } }] });
  });
  assert.equal(sent.length, 2);
  assert.deepEqual(sent[1].tools, tools);
  assert.equal(sent[1].tool_choice, 'auto');
  assert.equal(result.routingFallback, true);
});

test('confirmation follow-ups retain the request and assistant context before any tool call', async () => {
  const contexts = [];
  for (const [request, confirmation, name] of [
    ['Read README.md', 'Shall I read the file?', 'read_file'],
    ['Run the test suite', 'Shall I run the tests?', 'exec_shell_command'],
  ]) {
    assert.ok(tools.some(t => t.function.name === name));
    const history = [{ role: 'user', content: request }, { role: 'assistant', content: confirmation }, { role: 'user', content: 'Yes, go ahead' }];
    const result = await routeTools('local', { ...body, messages: history }, {}, signal(), async (_, init) => {
      const context = JSON.parse(init.body).messages[1].content;
      contexts.push(context);
      assert.equal(context, `user: ${request}\nassistant: ${confirmation}\nuser: Yes, go ahead`);
      return reply([context.includes('Read README.md') ? 'read_file' : 'exec_shell_command']);
    });
    assert.deepEqual(result.body.tools.map(t => t.function.name), [name]);
  }
  assert.notEqual(contexts[0], contexts[1]);
});

test('previously called contracts remain available for subsequent rounds', async () => {
  const history = [...messages, { role: 'assistant', tool_calls: [{ function: { name: 'read_file' } }] }, { role: 'tool', content: 'done' }];
  const result = await routeTools('local', { ...body, messages: history }, {}, signal(), async () => reply([]));
  assert.deepEqual(result.body.tools.map(t => t.function.name), ['read_file']);
});

test('discovery excludes attachment payloads and truncates oversized results', async () => {
  const history = [...messages, { role: 'tool', content: 'x'.repeat(10000) },
    { role: 'user', content: [{ type: 'text', text: 'Read README.md' }, { type: 'image_url', image_url: { url: 'data:image/png;base64,SECRET' } }] }];
  await routeTools('local', { ...body, messages: history }, {}, signal(), async (_, init) => {
    const request = JSON.parse(init.body);
    assert.deepEqual(request.messages.map(m => m.role), ['system', 'user']);
    assert.ok(request.messages[1].content.includes('Read README.md'));
    assert.ok(request.messages[1].content.length < 2200);
    assert.ok(!request.messages[1].content.includes('SECRET'));
    return reply(['read_file']);
  });
});

test('discovery bounds recent conversation and tool results in chronological order', async () => {
  const history = [{ role: 'user', content: 'OLD_REQUEST' },
    ...Array.from({ length: 8 }, (_, i) => ({ role: i % 2 ? 'assistant' : 'user', content: `${i}:` + 'x'.repeat(10000) })),
    ...Array.from({ length: 4 }, (_, i) => ({ role: 'tool', content: `result-${i}` })),
    { role: 'user', content: 'Yes, go ahead' }];
  let context;
  await routeTools('local', { ...body, messages: history }, {}, signal(), async (_, init) => {
    context = JSON.parse(init.body).messages[1].content;
    return reply([]);
  });
  assert.ok(context.length <= 8000);
  assert.ok(!context.includes('OLD_REQUEST'));
  assert.ok(!context.includes('result-0'));
  assert.ok(!context.includes('result-1'));
  assert.ok(context.includes('result-2'));
  assert.ok(context.indexOf('result-2') < context.indexOf('result-3'));
  assert.ok(context.endsWith('user: Yes, go ahead'));
});

test('discovery retains the current request after several tool results', async () => {
  const history = [...messages, ...Array.from({ length: 4 }, () => ({ role: 'tool', content: 'done' }))];
  await routeTools('local', { ...body, messages: history }, {}, signal(), async (_, init) => {
    const request = JSON.parse(init.body);
    assert.ok(request.messages[1].content.includes('Read README.md using read_file'));
    return reply([]);
  });
});

test('cancellation never falls through to a second request', async () => {
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(routeTools('local', body, {}, controller.signal, async () => { throw Error('cancelled'); }));
});

test('models rejecting tool schemas recover with text chat and a visible routing error', async () => {
  const sent = [];
  const result = await chatRequest('local', body, {}, signal(), async (_, init) => {
    sent.push(JSON.parse(init.body));
    if (sent.length === 1) return reply(['read_file']);
    if (sent.length === 2) return new Response('Tools are not supported', { status: 400 });
    return Response.json({ choices: [{ message: { content: 'Text fallback' } }] });
  });
  assert.equal(sent.length, 3);
  assert.equal(sent[0].tools, undefined);
  assert.deepEqual(sent[1].tools.map(t => t.function.name), ['read_file']);
  assert.equal(sent[2].tools, undefined);
  assert.equal(result.response.ok, true);
  assert.equal(result.routingFallback, true);
  assert.match(result.routingError, /not supported/);
});

test('a request without tools never pays discovery cost', async () => {
  let count = 0;
  await chatRequest('local', { messages }, {}, signal(), async () => { count++; return Response.json({}); });
  assert.equal(count, 1);
});
