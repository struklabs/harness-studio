// Playground: prompt-cache warm-up. With tools on, a conversation's first request starts with a
// long fixed prefix (system prompt, tool guidance, tool definitions: ~2.2k tokens). On a slow
// GPU processing it is most of the first message's wait. When a new conversation opens on a
// model a local Harness server has loaded, the same prefix is sent once ahead with an empty user
// turn and one output token: llama-server keeps it in a slot's KV cache, and the real first
// request (whose rendered prompt starts with the same bytes) only processes the user's message.
// Pure helpers; the Playground decides when (see `warmKey`) and makes the first turn wait for a
// warm-up still in flight, so that turn is never slower than without it.

/** Only loopback addresses: a warm-up never goes to a remote server or a cloud provider. */
export function isLoopbackUrl(url: string): boolean {
  try {
    const host = new URL(url).hostname;
    return host === '127.0.0.1' || host === 'localhost' || host === '[::1]' || host === '::1';
  } catch {
    return false;
  }
}

/**
 * The warm-up request: `messages` and `extra` built exactly as the real first turn builds them
 * (same system prompt, guidance, tools and template options), plus an empty user message so
 * every chat template renders the prefix; one output token, not streamed.
 */
export function warmBody(
  messages: Record<string, unknown>[],
  tools: unknown[],
  model: string,
  extra: Record<string, unknown>,
): Record<string, unknown> {
  const body: Record<string, unknown> = {
    ...extra,
    messages: [...messages, { role: 'user', content: '' }],
    stream: false,
    max_tokens: 1,
    tools,
  };
  delete body.stream_options;
  delete body.n_probs;
  delete body.logprobs;
  delete body.top_logprobs;
  if (model) body.model = model;
  return body;
}

/**
 * Whether (and under which key) to warm: a new conversation (no turns), tools on, no attached
 * documents (they change the system message per turn), and the chat model loaded on a Harness
 * server reached over loopback. `null` = do not warm. One warm-up per key.
 */
export function warmKey(o: {
  conversation: string | null;
  model: string;
  turns: number;
  tools: number;
  documents: number;
  localModel: boolean;
  loaded: boolean;
  endpoint: string;
}): string | null {
  if (!o.model || o.turns > 0 || o.tools === 0 || o.documents > 0 || !o.localModel || !o.loaded) return null;
  if (!isLoopbackUrl(o.endpoint)) return null;
  return `${o.conversation ?? 'new'}\u0000${o.model}`;
}
