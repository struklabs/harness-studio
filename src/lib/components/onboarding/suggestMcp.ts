// MCP servers other tools on this machine already know (Claude Code, Codex, Cursor, ...), offered
// by the wizard. Reads through the same backend commands as Connections > MCP > Import.
import { invoke } from '#lib/ipc.js';
import { uniqueServerName, validateMcpConfig } from '#lib/mcp-client.js';
import type { McpConfig, McpImportSource, McpImportSourceInfo, McpServerDef } from '#lib/mcp-types.js';

export const MCP_SOURCE_LABEL: Record<McpImportSource, string> = {
  claude: 'Claude Desktop',
  'claude-code': 'Claude Code',
  codex: 'Codex',
  cursor: 'Cursor',
  lmstudio: 'LM Studio',
};

export interface FoundMcpServer {
  /** `<source>:<name>`, unique across sources. */
  key: string;
  source: McpImportSource;
  name: string;
  def: McpServerDef;
}

/**
 * The servers of every import source whose config file exists, minus those the Harness config
 * already has (same name and definition) and repeats of the same server in several sources.
 */
export async function findMcpServers(): Promise<FoundMcpServer[]> {
  const [sources, current] = await Promise.all([
    invoke<McpImportSourceInfo[]>('get_mcp_import_sources'),
    invoke<McpConfig>('get_mcp_config'),
  ]);
  const have = new Set(Object.entries(current?.mcpServers ?? {}).map(([n, d]) => `${n}\n${JSON.stringify(d)}`));
  const out: FoundMcpServer[] = [];
  for (const [source, , exists] of sources) {
    if (!exists) continue;
    let cfg: McpConfig;
    try {
      cfg = await invoke<McpConfig>('read_mcp_import_source', { source });
    } catch {
      continue; // an unreadable file of another tool is not the wizard's problem
    }
    for (const [name, def] of Object.entries(cfg.mcpServers ?? {}).sort(([a], [b]) => a.localeCompare(b))) {
      const sig = `${name}\n${JSON.stringify(def)}`;
      if (have.has(sig) || validateMcpConfig({ mcpServers: { [name]: def } }).length > 0) continue;
      have.add(sig);
      out.push({ key: `${source}:${name}`, source, name, def });
    }
  }
  return out;
}

/** Adds the chosen servers to the Harness config (name clashes get a -2, -3 suffix). Throws on invalid input or save failure. */
export async function importMcpServers(chosen: FoundMcpServer[]): Promise<number> {
  if (chosen.length === 0) return 0;
  const current = await invoke<McpConfig>('get_mcp_config');
  const next: McpConfig = { mcpServers: { ...(current?.mcpServers ?? {}) } };
  const taken = new Set(Object.keys(next.mcpServers));
  for (const s of chosen) {
    const name = uniqueServerName(s.name, taken);
    taken.add(name);
    next.mcpServers[name] = s.def;
  }
  const errors = validateMcpConfig(next);
  if (errors.length) throw new Error(errors.map((e) => e.name).join(', '));
  await invoke('save_mcp_config', { config: next });
  return chosen.length;
}
