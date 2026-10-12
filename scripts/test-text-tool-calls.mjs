import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { salvageToolCalls, TextCallStreamBuffer } from '../src/lib/components/playground/textToolCalls.ts';

test('ordinary chunks are visible immediately; only a marker suffix is held', () => {
  const buffer = new TextCallStreamBuffer();
  assert.equal(buffer.push('Merhaba '), 'Merhaba ');
  assert.equal(buffer.push('dünya.'), 'dünya.');
  assert.equal(buffer.push(' Şimdi <too'), ' Şimdi ');
  assert.equal(buffer.push('ls>\n{"name":'), '');
  assert.equal(buffer.drain(), '<tools>\n{"name":');
});

test('all openings survive arbitrary chunk boundaries; initial JSON ignores whitespace', () => {
  for (const marker of ['<tool_call', '<tools', '<function=', '[TOOL_CALLS]', '<|python_tag|>', '```']) {
    const buffer = new TextCallStreamBuffer();
    let visible = '';
    for (const char of `Önce. ${marker}payload`) visible += buffer.push(char);
    assert.equal(visible, 'Önce. ');
    assert.equal(buffer.drain(), `${marker}payload`);
  }
  const buffer = new TextCallStreamBuffer();
  assert.equal(buffer.push(' \n'), '');
  assert.equal(buffer.push('{"name"'), '');
  assert.equal(buffer.drain(), ' \n{"name"');
  const prose = new TextCallStreamBuffer();
  assert.equal(prose.push(' \n'), '');
  assert.equal(prose.push('Merhaba'), ' \nMerhaba');
  assert.equal(prose.drain(), '');
});

test('failed recovery and IPC errors preserve every buffered character', async () => {
  for (const fail of [false, true]) {
    const msg = { ...message(), content: '' };
    const buffer = new TextCallStreamBuffer();
    const original = 'Metin <tool_call>invalid';
    msg.content += buffer.push(original);
    const work = salvageToolCalls(msg, ['web_search'], new AbortController().signal, async () => {
      if (fail) throw new Error('IPC failed');
      return { content: original, tool_calls: [] };
    }, msg.content + buffer.drain());
    if (fail) await assert.rejects(work); else await work;
    assert.equal(msg.content, original);
  }
});

const message = () => ({ id: 'assistant', role: 'assistant', content: 'Searching. <tool_call>{}</tool_call>' });
test('greater-than prefix survives real token splits; rejected content is lossless', async () => {
  const chunks = ['>', '{"name"', ':"web_search",', '"arguments":{', '"query":"x"', '}}'];
  const raw = chunks.join('');
  for (let split = 0; split <= raw.length; split++) {
    const buffer = new TextCallStreamBuffer();
    assert.equal(buffer.push(raw.slice(0, split)), '');
    assert.equal(buffer.push(raw.slice(split)), '');
    assert.equal(buffer.drain(), raw);
  }
  for (const parts of [chunks, [raw], ['>{"name"', ':"web_search","arguments":{"query":"x"}}']]) {
    const buffer = new TextCallStreamBuffer();
    for (const part of parts) assert.equal(buffer.push(part), '');
    assert.equal(buffer.drain(), raw);
  }
  for (const parts of [['>', ' ', '`', '``json\n', raw.slice(1), '\n```'], [' \n> \n', raw.slice(1), ' \n']]) {
    const buffer = new TextCallStreamBuffer();
    for (const part of parts) assert.equal(buffer.push(part), '');
    assert.equal(buffer.drain(), parts.join(''));
  }
  for (const text of ['>', '>broken', '>{"name":', '>>{"name":"web_search","arguments":{}}', 'Örnek: {"name":"web_search","arguments":{}} Done.', 'Bir\niki\n```json\n{"name":"web_search","arguments":{}}\n```']) {
    const buffer = new TextCallStreamBuffer();
    let visible = '';
    for (const char of text) visible += buffer.push(char);
    visible += buffer.drain();
    assert.equal(visible, text);
    const msg = { ...message(), content: visible };
    await salvageToolCalls(msg, ['web_search'], new AbortController().signal,
      async (content) => ({ content, tool_calls: [] }));
    assert.equal(msg.content, text);
  }
});
const recovered = { content: 'Searching.', tool_calls: [{ id: 'call_recovered', name: 'web_search', arguments: '{"query":"x"}' }] };

// Load the actual pure loop/history helpers without their unrelated Tauri/Svelte dependencies.
const source = readFileSync(new URL('../src/lib/components/playground/agentLoop.ts', import.meta.url), 'utf8');
const helpers = source.slice(source.indexOf('export interface AgentLoopOptions'), source.indexOf('// ---- Response stats ----'));
const js = ts.transpileModule(helpers, { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { guardToolCalls } = await import('../src/lib/components/playground/toolLoopGuard.ts');
globalThis.__textCallGuard = guardToolCalls;
const guardJs = 'const guardToolCalls = globalThis.__textCallGuard;';
const { runAgentLoop, buildApiMessages } = await import(`data:text/javascript;base64,${Buffer.from(guardJs + '\nconst activeCompaction = () => undefined; const CANCELLED_TEXT = "Cancelled"; const DENIED_TEXT = "Denied"; const summaryBlock = (s) => s; const userContent = (text) => text;\n' + js).toString('base64')}`);

test('core recovery replaces content with a pending call, preserving the explanation and id', async () => {
  const msg = message();
  await salvageToolCalls(msg, ['web_search'], new AbortController().signal, async (content, names) => {
    assert.equal(content, msg.content);
    assert.deepEqual(names, ['web_search']);
    return recovered;
  });
  assert.equal(msg.content, 'Searching.');
  assert.deepEqual(msg.toolCalls, [{ ...recovered.tool_calls[0], status: 'pending', recoveredFromText: true }]);
  assert.equal(msg.finishReason, 'tool_calls');
});

test('native calls, disabled tools, errors, stopped and truncated responses never invoke recovery', async () => {
  for (const extra of [{ toolCalls: [{ id: 'native' }] }, { error: 'broken stream' }, { stopped: true }, { finishReason: 'length' }, { finishReason: 'content_filter' }]) {
    const msg = { ...message(), ...extra };
    const original = structuredClone(msg);
    await salvageToolCalls(msg, ['web_search'], new AbortController().signal, () => assert.fail('must not parse'));
    assert.deepEqual(msg, original);
  }
  await salvageToolCalls(message(), [], new AbortController().signal, () => assert.fail('tools off'));
  const c = new AbortController();
  c.abort();
  await salvageToolCalls(message(), ['web_search'], c.signal, () => assert.fail('cancelled'));
});

test('stop during IPC and unrecognized JSON leave the response unchanged', async () => {
  const msg = message();
  const original = structuredClone(msg);
  const c = new AbortController();
  await salvageToolCalls(msg, ['web_search'], c.signal, async () => { c.abort(); return recovered; });
  assert.deepEqual(msg, original);
  await salvageToolCalls(msg, ['web_search'], new AbortController().signal, async () => ({ content: msg.content, tool_calls: [] }));
  assert.deepEqual(msg, original);
});

test('buffered raw JSON never becomes visible during the IPC await', async () => {
  const msg = { ...message(), content: '' };
  let resolve;
  const pending = salvageToolCalls(msg, ['web_search'], new AbortController().signal,
    () => new Promise((done) => { resolve = done; }), message().content);
  await Promise.resolve();
  assert.equal(msg.content, '');
  assert.equal(msg.toolCalls, undefined);
  resolve(recovered);
  await pending;
  assert.equal(msg.content, 'Searching.');
  assert.equal(msg.toolCalls[0].recoveredFromText, true);
});

test('salvaged call executes in the actual agent loop and becomes assistant/tool history', async () => {
  const c = new AbortController();
  const messages = [{ id: 'user', role: 'user', content: 'Search the internet' }];
  let rounds = 0;
  await runAgentLoop({
    controller: c, maxRounds: 3, activeToolCount: () => 1,
    streamTurn: async () => {
      const msg = rounds++ === 0 ? message() : { id: 'final', role: 'assistant', content: 'Found it.' };
      await salvageToolCalls(msg, ['web_search'], c.signal, async () => rounds === 1 ? recovered : { content: msg.content, tool_calls: [] });
      messages.push(msg);
      return msg;
    },
    execute: async (msg) => { msg.toolCalls[0].status = 'done'; msg.toolCalls[0].result = 'Search result'; },
    persist: () => {},
  });
  assert.equal(rounds, 2);
  const api = buildApiMessages(messages, { systemPrompt: '' });
  assert.deepEqual(api[1], { role: 'assistant', content: 'Searching.', tool_calls: [{ id: 'call_recovered', type: 'function', function: { name: 'web_search', arguments: '{"query":"x"}' } }] });
  assert.deepEqual(api[2], { role: 'tool', tool_call_id: 'call_recovered', content: 'Search result' });
  assert.deepEqual(api[3], { role: 'assistant', content: 'Found it.' });
});

test('short introductions stream immediately and then hold JSON or fences', () => {
  for (const intro of ['Arıyorum.', 'Örnek:', 'ş'.repeat(120), '😀'.repeat(120)]) {
    for (const opening of ['{', '```json\n']) {
      const buffer = new TextCallStreamBuffer();
      assert.equal(buffer.push(intro), intro);
      assert.equal(buffer.push('\n' + opening), '\n');
      assert.equal(buffer.push('payload'), '');
      assert.equal(buffer.drain(), opening + 'payload');
    }
  }
});

test('short introduction and call openings survive every Unicode character split', () => {
  const call = '{"name":"web_search","arguments":{}}';
  for (const intro of ['Arıyorum.', 'ş'.repeat(120), '😀'.repeat(120)]) {
    for (const body of [call, '```json\n' + call + '\n```']) {
      const chars = [...intro + '\n' + body];
      for (let split = 0; split <= chars.length; split++) {
        const buffer = new TextCallStreamBuffer();
        let visible = buffer.push(chars.slice(0, split).join(''));
        visible += buffer.push(chars.slice(split).join(''));
        assert.ok(!visible.includes('{') && !visible.includes('`'));
        assert.equal(visible + buffer.drain(), chars.join(''));
      }
    }
  }
});

test('review: quote pushes are immediate', () => {
 const b = new TextCallStreamBuffer();
 for (const [chunk, expected] of [['>', ''], [' Not: ', '> Not: '], ['[1]', '[1]'], [' Bu normal alıntı.', ' Bu normal alıntı.']]) assert.equal(b.push(chunk), expected);
});

test('bracket introductions release prose and hold the following call at every split', async () => {
 const call = '{"name":"web_search","arguments":{}}';
 const intro = '[örnek] ';
 for (let split=0; split<=intro.length; split++) {
   const b = new TextCallStreamBuffer();
   const visible = b.push(intro.slice(0,split)) + b.push(intro.slice(split));
   assert.equal(visible, intro);
   assert.equal(b.push(call), '');
   assert.equal(b.drain(), call);
 }
 for (const prefix of ['', '> ']) {
   const b = new TextCallStreamBuffer();
   assert.equal(b.push(prefix + '['), '');
   assert.equal(b.push('1'), prefix + '[1');
   assert.equal(b.push('] normal'), '] normal');
 }
});

 test('plural missing close and XML fences stay buffered until core recovery', async () => {
   const call = '{"name":"web_search","arguments":{"query":"x"}}';
   for (const tail of ['', 'end_tool_calls', '</tool_calls>']) {
     for (const raw of [`<tool_calls>${call}${tail}`, '```xml\n<tool_calls>' + call + tail + '\n```']) {
       const b = new TextCallStreamBuffer();
       let visible = '';
       for (const c of raw) visible += b.push(c);
       assert.equal(visible, '');
       assert.equal(b.drain(), raw);
       const msg = { id: 'a', role: 'assistant', content: '' };
       await salvageToolCalls(msg, ['web_search'], new AbortController().signal, async (text) => {
         assert.equal(text, raw);
         return { content: '', tool_calls: [{ id: 'c', name: 'web_search', arguments: '{"query":"x"}' }] };
       }, raw);
       assert.equal(msg.toolCalls.length, 1);
     }
   }
 });

test('agent loop never executes an oversized recovered turn', async () => {
  const c = new AbortController();
  const msg = { id: 'a', role: 'assistant', content: '', toolCalls: Array.from({length:16},(_,n)=>({id:'c'+n,name:'read',arguments:JSON.stringify({n}),status:'pending'})) };
  let executed = false;
  await runAgentLoop({controller:c,maxRounds:5,activeToolCount:()=>16,streamTurn:async()=>msg,execute:async()=>{executed=true;},persist:()=>{}});
  assert.equal(c.signal.aborted,true);
  assert.equal(executed,false);
  assert.equal(msg.toolCalls.length,1);
  assert.equal(msg.toolLimitReached,true);
  assert.ok(msg.toolCalls.every(c=>c.status==='stopped'));
});

 test('creator alias calls after a long introduction stay buffered and use the Rust adapter', async () => {
   const intro = 'İşte text_stats aracını kullanarak verdiğiniz metnin sözcük ve karakter sayısını hesaplamak üzere hazırlanmış, araç çağrısını içeren JSON nesnesi:';
   assert.ok([...intro].length > 120);
   const call = '{"function_name":"text_stats","arguments":{"text":"Merhaba dünya"}}';
   for (const body of [call, '```json\n' + call + '\n```']) {
     const raw = intro + '\n' + body;
     const b = new TextCallStreamBuffer();
     let visible = '';
     for (const c of raw) visible += b.push(c);
     assert.equal(visible, intro + '\n');
     const msg = { id: 'a', role: 'assistant', content: visible };
     await salvageToolCalls(msg, ['text_stats'], new AbortController().signal, async (content, names) => {
       assert.equal(content, raw);
       assert.deepEqual(names, ['text_stats']);
       return { content: intro, tool_calls: [{ id: 'c', name: 'text_stats', arguments: '{"text":"Merhaba dünya"}' }] };
     }, visible + b.drain());
     assert.equal(msg.toolCalls[0].name, 'text_stats');
   }
 });
