type Message = Record<string, unknown>;

function text(value: unknown): string {
  return value == null ? '' : typeof value === 'string' ? value : JSON.stringify(value);
}

function shortResult(value: unknown): string {
  const result = text(value);
  return result.length > 2000 ? result.slice(0, 2000) + '… [truncated]' : result;
}

type Part = Record<string, unknown>;

function parts(content: unknown): Part[] {
  if (Array.isArray(content)) return content as Part[];
  const value = text(content);
  return value ? [{ type: 'text', text: value }] : [];
}

/** Joins two contents: plain strings stay a string, otherwise one multimodal part list. */
function mergeContent(a: unknown, b: unknown): unknown {
  if (!Array.isArray(a) && !Array.isArray(b)) return [text(a), text(b)].filter(Boolean).join('\n\n');
  return [...parts(a), ...parts(b)];
}

export function hasToolHistory(messages: Message[]): boolean {
  return messages.some((m) => m.role === 'tool' || (Array.isArray(m.tool_calls) && m.tool_calls.length > 0));
}

/** Wire-only fallback for templates that can render only alternating user/assistant turns. */
export function flattenToolHistory(messages: Message[]): Message[] {
  const out: Message[] = [];
  // Non-tool content (image/audio parts included) is kept as is; only tool records become text.
  const append = (role: unknown, content: unknown) => {
    const previous = out.at(-1);
    if (role !== 'system' && previous && previous.role === role) previous.content = mergeContent(previous.content, content);
    else out.push({ role, content });
  };
  for (let i = 0; i < messages.length; i++) {
    const message = messages[i];
    if (message.role === 'system') {
      out.push({ ...message });
      continue;
    }
    if (message.role === 'assistant' && Array.isArray(message.tool_calls)) {
      const results: Message[] = [];
      while (messages[i + 1]?.role === 'tool') results.push(messages[++i]);
      const used = new Set<Message>();
      const records = message.tool_calls.map((call: { id?: string; function?: { name?: string; arguments?: unknown } }) => {
        const matches = results.filter((result) => result.tool_call_id === call.id);
        matches.forEach((result) => used.add(result));
        const result = matches.length ? matches.map((m) => shortResult(m.content)).join('\n') : '(no result)';
        return `[${call.function?.name ?? 'tool'}(${text(call.function?.arguments)}) → ${result}]`;
      });
      for (const result of results) {
        if (!used.has(result)) records.push(`[${text(result.name || result.tool_call_id || 'tool')} → ${shortResult(result.content)}]`);
      }
      append('assistant', mergeContent(message.content, records.join('\n\n')));
    } else if (message.role === 'tool') {
      append('assistant', `[${text(message.name || message.tool_call_id || 'tool')} → ${shortResult(message.content)}]`);
    } else append(message.role, message.content);
  }
  return out;
}

export function shouldFlattenHistory(status: number, error: string, messages: Message[]): boolean {
  return status === 400 && (
    /conversation roles must alternate/i.test(error) ||
    (hasToolHistory(messages) && /jinja exception|unable to generate parser for this template|(?:chat[ _-]?)?template[^\n]*(?:reject|error|unsupported|not support|cannot|failed)/i.test(error))
  );
}

export function flattenChatBody(body: Record<string, unknown>): Record<string, unknown> {
  const { tools: _tools, tool_choice: _choice, ...rest } = body;
  // These templates cannot render tools at all, even after tool history is flattened.
  return { ...rest, messages: flattenToolHistory(body.messages as Message[]) };
}
