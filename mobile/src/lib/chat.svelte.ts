// One chat at a time, streamed from a Harness model or a BYOK model. Each reply carries where
// it ran (provider or Harness server/device) and the model's capabilities and cost.
import { connections } from './connections.svelte.ts';
import { accountFor, placementFromHeaders, resolveDevice, type Placement } from './harness.ts';
import { httpStream, type HttpRequest } from './native.ts';
import { chatRequest, cost, errorMessage, mergeUsage, providerHeaders, providerLabel, statusError, streamDelta, type ChatMessage, type ProviderModel, type Usage } from './providers.ts';
import { SseParser } from './sse.ts';

export type Target = { type: 'harness'; connection: string; model: string } | { type: 'provider'; connection: string; model: string };

export interface Turn {
  role: 'user' | 'assistant';
  content: string;
  placement?: Placement;
  usage?: Usage | null;
  cost?: number | null;
  error?: string | null;
  detail?: string | null;
  streaming?: boolean;
}

export function targetKey(t: Target): string {
  return `${t.type}:${t.connection}:${t.model}`;
}

export function parseTargetKey(key: string): Target | null {
  const m = key.match(/^(harness|provider):([^:]+):(.+)$/);
  return m ? { type: m[1] as Target['type'], connection: m[2], model: m[3] } : null;
}

class Chat {
  turns = $state<Turn[]>([]);
  target = $state<Target | null>(null);
  busy = $state(false);
  private cancel: (() => void) | null = null;

  /** The BYOK model's listing (context, tools, vision, prices), when the target is one. */
  providerModel(): ProviderModel | null {
    const t = this.target;
    if (t?.type !== 'provider') return null;
    return connections.status[t.connection]?.models.find((m) => m.id === t.model) ?? null;
  }

  clear() {
    this.stop();
    this.turns = [];
  }

  stop() {
    this.cancel?.();
    this.cancel = null;
    const last = this.turns.at(-1);
    if (last?.streaming) last.streaming = false;
    this.busy = false;
  }

  async send(text: string) {
    const target = this.target;
    if (!target || !text.trim() || this.busy) return;
    const history: ChatMessage[] = this.turns.filter((t) => !t.error).map((t) => ({ role: t.role, content: t.content }));
    history.push({ role: 'user', content: text.trim() });
    this.turns.push({ role: 'user', content: text.trim() });
    this.turns.push({ role: 'assistant', content: '', streaming: true });
    const reply = this.turns[this.turns.length - 1];
    this.busy = true;

    let req: HttpRequest;
    let kind: Parameters<typeof streamDelta>[0];
    let placement: Placement;
    const model = this.providerModel();
    if (target.type === 'harness') {
      const c = connections.harness.find((h) => h.id === target.connection);
      if (!c) return this.fail(reply, 'mobile.err.no_connection');
      kind = 'harness';
      const body = chatRequest('openai', target.model, history);
      const url = `${c.gatewayUrl}/v1${body.path}`;
      req = { method: 'POST', url, headers: [['content-type', 'application/json']], body: body.body, secret: { account: accountFor(c, url), scheme: 'bearer' } };
      placement = { where: 'harness', provider: c.name, model: target.model, server: null, device: null, backend: null, queueMs: null, loadMs: null };
    } else {
      const p = connections.providers.find((x) => x.id === target.connection);
      if (!p) return this.fail(reply, 'mobile.err.no_connection');
      kind = p.kind;
      const body = chatRequest(p.kind, target.model, history);
      const keyed = connections.status[p.id]?.state !== 'key_missing';
      req = {
        method: 'POST',
        url: `${p.baseUrl}${body.path}`,
        headers: providerHeaders(p.kind),
        body: body.body,
        secret: keyed ? { account: p.account, scheme: p.kind === 'anthropic' ? 'x_api_key' : 'bearer' } : undefined,
      };
      placement = { where: p.kind === 'custom' ? 'endpoint' : 'provider', provider: p.kind === 'custom' ? p.name : providerLabel(p.kind), model: target.model, server: null, device: null, backend: null, queueMs: null, loadMs: null };
    }
    reply.placement = placement;

    const parser = new SseParser();
    let status = 0;
    let errorBody = '';
    let usage: Usage | null = null;
    const handle = (event: string | null, data: string) => {
      const d = streamDelta(kind, event, data);
      if (d.error) {
        reply.error = 'mobile.err.provider';
        reply.detail = d.error;
      }
      if (d.text) reply.content += d.text;
      usage = mergeUsage(usage, d.usage);
    };
    try {
      this.cancel = await httpStream(req, (e) => {
        if (e.event === 'head') {
          status = e.status;
          if (target.type === 'harness') {
            const c = connections.harness.find((h) => h.id === target.connection)!;
            reply.placement = { ...placementFromHeaders(c, e.headers), model: placementFromHeaders(c, e.headers).model ?? target.model };
            void resolveDevice(c, reply.placement).then((p) => (reply.placement = p));
          }
        } else if (e.event === 'data') {
          if (status >= 400) {
            errorBody += e.text;
            return;
          }
          for (const ev of parser.push(e.text)) handle(ev.event, ev.data);
        } else if (e.event === 'end') {
          for (const ev of parser.flush()) handle(ev.event, ev.data);
          if (status >= 400) {
            reply.error = statusError(status);
            reply.detail = errorMessage(errorBody);
          }
          reply.usage = usage;
          reply.cost = cost(model, usage);
          reply.streaming = false;
          this.busy = false;
          this.cancel = null;
        } else if (e.event === 'error') {
          this.fail(reply, e.message);
        }
      });
    } catch (e) {
      this.fail(reply, typeof e === 'string' ? e : 'mobile.transport');
    }
  }

  private fail(reply: Turn, key: string) {
    reply.error = key;
    reply.streaming = false;
    this.busy = false;
    this.cancel = null;
  }
}

export const chat = new Chat();
