import assert from 'node:assert/strict';
import { test } from 'node:test';
import { thinkingBody, thinkingOptions, splitThinking, observeThinking, resetThinking } from '../src/lib/components/playground/thinking.ts';

const effort = { effort: true, toggle: false, budget: false };
const budget = { effort: false, toggle: true, budget: true };
const toggle = { ...budget, budget: false };

test('effort levels use the shared table and never offer Off', () => {
  for (const [level, value] of [['off', 'low'], ['light', 'low'], ['normal', 'medium'], ['deep', 'high']]) {
    assert.deepEqual(thinkingBody({}, level, effort), { reasoning_effort: value });
  }
  assert.deepEqual(thinkingOptions(effort), ['auto', 'light', 'normal', 'deep']);
});

test('toggle and budget models use only fields they support', () => {
  for (const [level, value] of [['light', 1024], ['normal', 4096], ['deep', -1]]) {
    assert.deepEqual(thinkingBody({}, level, budget), { chat_template_kwargs: { enable_thinking: true }, thinking_budget_tokens: value });
    assert.deepEqual(thinkingBody({}, level, toggle), { chat_template_kwargs: { enable_thinking: true } });
  }
  assert.deepEqual(thinkingBody({}, 'off', budget), { chat_template_kwargs: { enable_thinking: false } });
  assert.deepEqual(thinkingOptions(toggle), ['auto', 'off', 'normal']);
});

test('chat overrides preserve presets and unrelated template kwargs', () => {
  const preset = { temperature: 0.4, reasoning_effort: 'high', thinking_budget_tokens: 12, reasoning_format: 'auto', chat_template_kwargs: { enable_thinking: false, custom: 7 } };
  assert.equal(thinkingBody(preset, 'auto', budget), preset);
  const out = thinkingBody(preset, 'normal', budget, 250.9);
  assert.equal(out.thinking_budget_tokens, 250);
  assert.equal(out.reasoning_effort, undefined);
  assert.deepEqual(out.chat_template_kwargs, { enable_thinking: true, custom: 7 });
  assert.equal(preset.chat_template_kwargs.enable_thinking, false);
  assert.equal(preset.thinking_budget_tokens, 12);
  assert.equal(out.reasoning_format, 'auto');
  assert.equal(thinkingBody(preset, 'deep', { effort: false, toggle: false, budget: false }), preset);
});

test('inline reasoning handles open, closed, and chunked tags without changing quoted tags', () => {
  assert.deepEqual(splitThinking('<thi'), { content: '', reasoning: '', live: true });
  assert.deepEqual(splitThinking('<think>work</thi'), { content: '', reasoning: 'work', live: true });
  assert.deepEqual(splitThinking(' <think>work</think>\nanswer'), { content: 'answer', reasoning: 'work', live: false });
  assert.equal(splitThinking('Use `<think>` here').content, 'Use `<think>` here');
  assert.deepEqual(splitThinking('so it is 400.\n</think>\n\n400', 'counting'), { content: '400', reasoning: 'counting\nso it is 400.', live: false });
  assert.equal(splitThinking('a </think> b').content, 'a </think> b');
});

test('duration measures reasoning to answer and survives subsequent answer tokens', () => {
  const message = { content: '', reasoning: 'first' };
  observeThinking(message, 100);
  message.content = 'answer'; observeThinking(message, 12100);
  assert.equal(message.reasoningMs, 12000);
  observeThinking(message, 15100);
  assert.equal(message.reasoningMs, 12000);
  resetThinking(message);
  message.content = '<think>next'; message.reasoning = ''; observeThinking(message, 16000);
  message.content += '</think>answer'; observeThinking(message, 17000);
  assert.equal(message.reasoningMs, 1000);
});
