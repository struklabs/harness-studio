import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { toolIndex, selectedTools, routeTools } from '../src/lib/components/playground/toolRouting.ts';

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

test('empty discovery permits a schema-free answer and removes tool_choice', async () => {
  const result = await routeTools('local', { ...body, tool_choice: 'required' }, {}, signal(), async () => reply([]));
  assert.equal(result.body.tools, undefined);
  assert.equal(result.body.tool_choice, undefined);
});

test('unknown names, malformed responses and failed discovery use request matches without execution', async () => {
  assert.throws(() => selectedTools('["invented"]', tools));
  assert.throws(() => selectedTools('{}', tools));
  for (const send of [async () => reply(['invented']), async () => new Response('', { status: 500 }), async () => { throw Error('offline'); }]) {
    const result = await routeTools('local', body, {}, signal(), send);
    assert.equal(result.fallback, true);
    assert.ok(result.body.tools.some(t => t.function.name === 'read_file'));
    assert.ok(result.body.tools.length < tools.length);
  }
});

test('previously called contracts remain available for subsequent rounds', async () => {
  const history = [...messages, { role: 'assistant', tool_calls: [{ function: { name: 'read_file' } }] }, { role: 'tool', content: 'done' }];
  const result = await routeTools('local', { ...body, messages: history }, {}, signal(), async () => reply([]));
  assert.deepEqual(result.body.tools.map(t => t.function.name), ['read_file']);
});

test('discovery uses one user turn and excludes attachment payloads and oversized results', async () => {
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
