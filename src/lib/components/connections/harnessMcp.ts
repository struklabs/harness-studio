// Data access for "Harness MCP'yi ekle". All backend calls of HarnessMcp.svelte live here.
import { invoke } from '#lib/ipc.js';
import type { ControlApiOverview } from '#lib/types.js';
import type { FileChange } from './integrations';

export interface McpTarget {
  id: 'claude-code' | 'codex' | 'opencode' | 'cursor' | string;
  name: string;
  docs_url: string;
  config_path: string | null;
  installed: boolean;
  configured: boolean;
  /** The current `harness` entry (as JSON). */
  entry: Record<string, unknown> | null;
  /** The config does not parse (apply is refused). */
  error: string | null;
}

export interface McpOverview {
  targets: McpTarget[];
  /** `harness` CLI next to the app or on PATH. */
  cli_path: string | null;
}

export type McpMode = 'stdio' | 'http';

export interface McpRequest {
  mode: McpMode;
  command: string;
  allowModelsWrite: boolean;
  url: string;
  token: string;
}

export const DEFAULT_MCP_URL = 'http://127.0.0.1:11500/api/harness/v1/mcp';

export function mcpOverview(): Promise<McpOverview> {
  return invoke<McpOverview>('harness_mcp_overview');
}

/** The Control API's MCP URL while it runs (null = API off). */
export async function controlApiMcpUrl(): Promise<string | null> {
  const o = await invoke<ControlApiOverview>('get_control_api_status');
  return o.status.running && o.status.base_url ? `${o.status.base_url}/mcp` : null;
}

function args(id: string, r: McpRequest) {
  return {
    id,
    mode: r.mode,
    command: r.command || null,
    allowModelsWrite: r.allowModelsWrite,
    url: r.url || null,
    token: r.token || null,
  };
}

export function previewMcp(id: string, r: McpRequest): Promise<FileChange[]> {
  return invoke<FileChange[]>('preview_harness_mcp', args(id, r));
}

export function applyMcp(id: string, r: McpRequest): Promise<string> {
  return invoke<string>('apply_harness_mcp', args(id, r));
}

export function removeMcp(id: string): Promise<string> {
  return invoke<string>('remove_harness_mcp', { id });
}

/** The `harness` entry inside a config's new content (shown instead of a diff for big files). */
export function entryText(id: string, after: string): string {
  if (id === 'codex') {
    const lines = after.split(/\r?\n/);
    const start = lines.findIndex((l) => l.trim() === '[mcp_servers.harness]');
    if (start < 0) return '';
    let end = lines.findIndex((l, i) => i > start && /^\s*\[/.test(l));
    if (end < 0) end = lines.length;
    return lines.slice(start, end).join('\n').trim();
  }
  try {
    const root = JSON.parse(after);
    const servers = id === 'opencode' ? root?.mcp : root?.mcpServers;
    return JSON.stringify({ harness: servers?.harness ?? null }, null, 2);
  } catch {
    return '';
  }
}

/** A one-line command for tools that have their own "add MCP server" CLI. */
export function manualCommand(id: string, r: McpRequest): string | null {
  const q = (s: string) => (/[\s"]/.test(s) ? `"${s.replace(/"/g, '\\"')}"` : s);
  const scope = r.allowModelsWrite ? ' --scope read,servers:write,tools,models:write' : '';
  const cmd = r.command || 'harness';
  if (id === 'claude-code') {
    return r.mode === 'stdio'
      ? `claude mcp add -s user harness -- ${q(cmd)} mcp${scope}`
      : `claude mcp add -s user --transport http harness ${r.url} -H "Authorization: Bearer <token>"`;
  }
  if (id === 'codex' && r.mode === 'stdio') {
    return `codex mcp add harness -- ${q(cmd)} mcp${scope}`;
  }
  return null;
}
