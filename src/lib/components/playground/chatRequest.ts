import { flattenChatBody, hasToolHistory, shouldFlattenHistory } from './toolHistory';

// Shared by normal and split Playground requests; never persisted with conversations.
const flattenTemplates = new Set<string>();

export async function chatRequest(
  endpoint: string,
  body: Record<string, unknown>,
  headers: Record<string, string>,
  signal: AbortSignal,
  send: typeof fetch = fetch
): Promise<{ response: Response; body: Record<string, unknown> }> {
  const key = JSON.stringify([endpoint.replace(/\/+$/, ''), body.model ?? '']);
  // Only tool history breaks these templates; a conversation without it keeps its tools.
  const known = flattenTemplates.has(key) && hasToolHistory(body.messages as Record<string, unknown>[]);
  let outgoing = known ? flattenChatBody(body) : body;
  const post = () => send(`${endpoint}/chat/completions`, {
    method: 'POST', headers, body: JSON.stringify(outgoing), signal,
  });
  let response = await post();
  if (!known && response.status === 400 && !signal.aborted) {
    const error = await response.clone().text().catch(() => '');
    if (!signal.aborted && shouldFlattenHistory(response.status, error, body.messages as Record<string, unknown>[])) {
      outgoing = flattenChatBody(body);
      response = await post(); // At most one retry, before any streaming content is consumed.
      if (response.ok) flattenTemplates.add(key);
    }
  }
  return { response, body: outgoing };
}
