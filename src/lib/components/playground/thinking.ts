import levels from '../../thinking-levels.json' with { type: 'json' };

export interface ReasoningControl { effort: boolean; toggle: boolean; budget: boolean }
export type ThinkingLevel = 'auto' | keyof typeof levels;
export const NO_REASONING: ReasoningControl = { effort: false, toggle: false, budget: false };

export function thinkingDefaultBudget(level: ThinkingLevel): number | null {
  return level === 'auto' ? null : levels[level].budget;
}

export function thinkingOptions(control: ReasoningControl): ThinkingLevel[] {
  if (control.effort) return ['auto', 'light', 'normal', 'deep'];
  if (control.budget) return ['auto', 'off', 'light', 'normal', 'deep'];
  return ['auto', 'off', 'normal'];
}

/** The per-chat choice overrides the preset without changing its request settings. */
export function thinkingBody(body: Record<string, unknown>, level: ThinkingLevel, control: ReasoningControl, budget: number | null = null): Record<string, unknown> {
  if ((!control.effort && !control.toggle) || (level === 'auto' && budget === null)) return body;
  const out = { ...body };
  if (level !== 'auto') {
    const row = levels[level];
    delete out.reasoning_effort;
    delete out.thinking_budget_tokens;
    delete out.reasoning_budget_tokens;
    const kwargs = { ...(out.chat_template_kwargs as Record<string, unknown> | undefined) };
    delete kwargs.enable_thinking;
    if (control.effort) out.reasoning_effort = row.effort;
    else {
      kwargs.enable_thinking = row.enabled;
      if (control.budget && row.enabled) out.thinking_budget_tokens = row.budget;
    }
    if (Object.keys(kwargs).length) out.chat_template_kwargs = kwargs;
    else delete out.chat_template_kwargs;
  }
  if (control.budget && level !== 'off' && budget !== null && Number.isFinite(budget) && budget >= -1) out.thinking_budget_tokens = Math.trunc(budget);
  return out;
}

/** Only a leading thinking block is protocol markup; quoted tags in answers stay intact. */
export function splitThinking(content: string, reasoning = ''): { content: string; reasoning: string; live: boolean } {
  const trimmed = content.trimStart();
  if (!trimmed.startsWith('<think>')) {
    if (trimmed && '<think>'.startsWith(trimmed)) return { content: '', reasoning, live: true };
    // A model whose thinking budget ran out may keep reasoning in the answer and close it with
    // its own stray tag: that part belongs to the thinking block.
    const stray = reasoning ? content.indexOf('</think>') : -1;
    if (stray >= 0) return { content: content.slice(stray + 8).trimStart(), reasoning: `${reasoning}\n${content.slice(0, stray).trimEnd()}`, live: false };
    return { content, reasoning, live: false };
  }
  const end = trimmed.indexOf('</think>', 7);
  let inline = end < 0 ? trimmed.slice(7) : trimmed.slice(7, end);
  if (end < 0) {
    // Hide a closing tag split across stream chunks.
    for (let n = 1; n < 8; n++) if (inline.endsWith('</think>'.slice(0, n))) { inline = inline.slice(0, -n); break; }
  }
  return { content: end < 0 ? '' : trimmed.slice(end + 8).trimStart(), reasoning: reasoning + inline, live: end < 0 };
}

const starts = new WeakMap<object, number>();
export function resetThinking(message: { reasoningMs?: number }): void {
  starts.delete(message);
  message.reasoningMs = undefined;
}
export function observeThinking(message: { content: string; reasoning?: string; reasoningMs?: number }, now: number): void {
  const parts = splitThinking(message.content, message.reasoning);
  if (parts.reasoning && !starts.has(message)) starts.set(message, now);
  const start = starts.get(message);
  if (start !== undefined && parts.content && message.reasoningMs === undefined) message.reasoningMs = Math.max(0, now - start);
}
