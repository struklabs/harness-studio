// Tool grouping for the Playground, the streaming tool-call accumulator and MCP config editing
// helpers. (Running tools is the core's: `tools-client.ts`; llama-server's own `/tools` is no
// longer used.) Pure (no Svelte / Tauri / DOM dependencies) so it can be tested from Node.

import { MCP_TRANSPORTS, type McpConfig, type McpServerDef, type McpTransport, type ServerTool, type ToolCall } from './mcp-types';

// ---------------------------------------------------------------------------
// Tool metadata helpers
// ---------------------------------------------------------------------------

/**
 * Group key for a tool: "" for built-ins, otherwise the MCP server name: the one the core
 * reports (`server`), else the longest configured name `N` such that the tool starts with `N_`,
 * else the text before the first underscore.
 */
export function toolGroup(tool: Pick<ServerTool, 'tool' | 'type' | 'server'>, knownServers: string[] = []): string {
  if (tool.type !== 'mcp') return '';
  if (tool.server) return tool.server;
  const match = knownServers
    .filter((n) => tool.tool.startsWith(`${n}_`))
    .sort((a, b) => b.length - a.length)[0];
  if (match) return match;
  const i = tool.tool.indexOf('_');
  return i > 0 ? tool.tool.slice(0, i) : tool.tool;
}

export function groupTools(tools: ServerTool[], knownServers: string[] = []): { group: string; tools: ServerTool[] }[] {
  const map = new Map<string, ServerTool[]>();
  for (const t of tools) {
    const g = toolGroup(t, knownServers);
    if (!map.has(g)) map.set(g, []);
    map.get(g)!.push(t);
  }
  return [...map.entries()]
    .sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : a.localeCompare(b)))
    .map(([group, list]) => ({ group, tools: list }));
}

/** Cuts a result to `max` characters (the core's `max_result_chars`, see `ToolLimits`) and marks the cut. */
export function truncateText(text: string, max: number): string {
  if (text.length <= max) return text;
  return `${text.slice(0, max)}\n…[truncated ${text.length - max} chars]`;
}

// ---------------------------------------------------------------------------
// Streaming tool-call accumulator
// ---------------------------------------------------------------------------

interface ToolCallDelta {
  index?: number;
  id?: string;
  type?: string;
  function?: { name?: string; arguments?: unknown };
}

/**
 * Accumulates OpenAI-style streamed `delta.tool_calls` fragments. Each fragment carries an
 * `index`; the first fragment of a call usually has `id` and `function.name`, later ones
 * append to `function.arguments`. Fragments without an index continue the latest call
 * unless they start a new one (a new `id`).
 */
export class ToolCallAccumulator {
  private calls: { id: string; name: string; args: string }[] = [];
  private byIndex = new Map<number, number>();
  private autoIds = 0;

  push(deltas: unknown): void {
    if (!Array.isArray(deltas)) return;
    for (const raw of deltas) {
      if (!raw || typeof raw !== 'object') continue;
      const d = raw as ToolCallDelta;
      let pos: number | undefined;
      if (typeof d.index === 'number') {
        pos = this.byIndex.get(d.index);
        if (pos === undefined) {
          pos = this.calls.push({ id: '', name: '', args: '' }) - 1;
          this.byIndex.set(d.index, pos);
        }
      } else {
        const last = this.calls.length - 1;
        const prev = this.calls[last];
        const newId = typeof d.id === 'string' && d.id !== '' && d.id !== prev?.id;
        // A new id starts a new call — unless the previous call is still anonymous and nameless.
        const startsNew = last < 0 || (newId && (prev.id !== '' || (!!d.function?.name && prev.name !== '')));
        pos = startsNew ? this.calls.push({ id: '', name: '', args: '' }) - 1 : last;
      }
      const c = this.calls[pos];
      if (typeof d.id === 'string' && d.id && !c.id) c.id = d.id;
      const fn = d.function;
      if (fn) {
        if (typeof fn.name === 'string' && fn.name) {
          // Some servers repeat the full name on every chunk; others stream it in pieces.
          c.name = !c.name || fn.name.startsWith(c.name) ? fn.name : c.name + fn.name;
        }
        if (typeof fn.arguments === 'string') c.args += fn.arguments;
        else if (fn.arguments && typeof fn.arguments === 'object') c.args = JSON.stringify(fn.arguments);
      }
    }
  }

  get size(): number {
    return this.calls.length;
  }

  /** Completed calls (ids generated where the server sent none). */
  result(): ToolCall[] {
    return this.calls
      .filter((c) => c.name)
      .map((c) => {
        if (!c.id) c.id = `call_${++this.autoIds}_${Math.random().toString(36).slice(2, 8)}`;
        return { id: c.id, type: 'function' as const, function: { name: c.name, arguments: c.args } };
      });
  }
}

/** Parses tool-call arguments: empty string = {}. */
export function parseToolArgs(args: string): { ok: true; value: Record<string, unknown> } | { ok: false; error: string } {
  const trimmed = args.trim();
  if (!trimmed) return { ok: true, value: {} };
  try {
    const v = JSON.parse(trimmed);
    if (v && typeof v === 'object' && !Array.isArray(v)) return { ok: true, value: v as Record<string, unknown> };
    return { ok: false, error: 'arguments must be a JSON object' };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

export function prettyArgs(args: string): string {
  const p = parseToolArgs(args);
  return p.ok ? JSON.stringify(p.value, null, 2) : args;
}

// ---------------------------------------------------------------------------
// MCP config helpers (mirrors the Rust validation in src-tauri/src/mcp.rs)
// ---------------------------------------------------------------------------

export const MCP_NAME_RE = /^[A-Za-z0-9_-]{1,48}$/;

export type McpValidationError =
  | { kind: 'name'; name: string }
  | { kind: 'transport'; name: string };

export function validateMcpConfig(config: McpConfig): McpValidationError[] {
  const errors: McpValidationError[] = [];
  for (const [name, def] of Object.entries(config.mcpServers ?? {})) {
    if (!MCP_NAME_RE.test(name)) errors.push({ kind: 'name', name });
    const hasCmd = typeof def.command === 'string' && def.command.trim() !== '';
    const hasUrl = typeof def.url === 'string' && /^https?:\/\//.test(def.url);
    const cmdSet = def.command !== undefined;
    const urlSet = def.url !== undefined;
    if (!((hasCmd && !urlSet) || (hasUrl && !cmdSet))) errors.push({ kind: 'transport', name });
  }
  return errors;
}

/** Checks a parsed raw-JSON config's shape; returns a cleaned config or an error string. */
export function coerceMcpConfig(value: unknown): { ok: true; config: McpConfig } | { ok: false; error: string } {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return { ok: false, error: 'root' };
  const servers = (value as { mcpServers?: unknown }).mcpServers ?? {};
  if (!servers || typeof servers !== 'object' || Array.isArray(servers)) return { ok: false, error: 'mcpServers' };
  const out: McpConfig = { mcpServers: {} };
  for (const [name, raw] of Object.entries(servers as Record<string, unknown>)) {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return { ok: false, error: name };
    const r = raw as Record<string, unknown>;
    const def: McpServerDef = {};
    if (r.command !== undefined) {
      if (typeof r.command !== 'string') return { ok: false, error: `${name}.command` };
      def.command = r.command;
    }
    if (r.args !== undefined) {
      if (!Array.isArray(r.args) || !r.args.every((a) => typeof a === 'string')) return { ok: false, error: `${name}.args` };
      if (r.args.length) def.args = r.args as string[];
    }
    for (const key of ['env', 'headers'] as const) {
      const m = r[key];
      if (m === undefined) continue;
      if (!m || typeof m !== 'object' || Array.isArray(m) || !Object.values(m).every((v) => typeof v === 'string')) {
        return { ok: false, error: `${name}.${key}` };
      }
      if (Object.keys(m).length) def[key] = { ...(m as Record<string, string>) };
    }
    if (r.url !== undefined) {
      if (typeof r.url !== 'string') return { ok: false, error: `${name}.url` };
      def.url = r.url;
    }
    if (r.disabled !== undefined) {
      if (typeof r.disabled !== 'boolean') return { ok: false, error: `${name}.disabled` };
      if (r.disabled) def.disabled = true;
    }
    if (r.transport !== undefined) {
      if (typeof r.transport !== 'string' || !(MCP_TRANSPORTS as string[]).includes(r.transport)) {
        return { ok: false, error: `${name}.transport` };
      }
      if (r.transport !== 'auto') def.transport = r.transport as McpTransport;
    }
    out.mcpServers[name] = def;
  }
  return { ok: true, config: out };
}

/** Parses `KEY=VALUE` (sep "=") or `Key: Value` (sep ":") lines. Blank lines and `#` comments are skipped. */
export function parseKeyValueLines(
  text: string,
  sep: '=' | ':'
): { ok: true; map: Record<string, string> } | { ok: false; line: number } {
  const map: Record<string, string> = {};
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line || line.startsWith('#')) continue;
    const at = line.indexOf(sep);
    if (at <= 0) return { ok: false, line: i + 1 };
    const key = line.slice(0, at).trim();
    if (!key) return { ok: false, line: i + 1 };
    map[key] = line.slice(at + 1).trim();
  }
  return { ok: true, map };
}

export function formatKeyValueLines(map: Record<string, string> | undefined, sep: '=' | ':'): string {
  return Object.entries(map ?? {})
    .map(([k, v]) => (sep === ':' ? `${k}: ${v}` : `${k}=${v}`))
    .join('\n');
}

/** `name`, or `name-2`, `name-3`… so it doesn't clash with `taken` (max 48 chars). */
export function uniqueServerName(name: string, taken: Iterable<string>): string {
  const set = new Set(taken);
  if (!set.has(name)) return name;
  for (let i = 2; ; i++) {
    const suffix = `-${i}`;
    const candidate = `${name.slice(0, 48 - suffix.length)}${suffix}`;
    if (!set.has(candidate)) return candidate;
  }
}
