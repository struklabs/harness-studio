// Types for the agent tools (Harness's core tools, its own MCP client, llama-server's old-path tools)
// and the matching Tauri commands (src-tauri/src/commands/mcp.rs, tools.rs). Kept separate from types.ts on purpose.

/** One MCP server entry (Cursor-compatible `mcpServers` format). */
export interface McpServerDef {
  /** stdio server executable (e.g. `npx`, `uvx`, an absolute path). */
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  /** HTTP(S) server endpoint. */
  url?: string;
  headers?: Record<string, string>;
  /** Kept in the list but not passed to llama-server. */
  disabled?: boolean;
  /** HTTP servers: `auto` (default; Streamable HTTP, then the older HTTP+SSE), `http` or `sse`. */
  transport?: McpTransport;
}

export type McpTransport = 'auto' | 'http' | 'sse';
export const MCP_TRANSPORTS: McpTransport[] = ['auto', 'http', 'sse'];

export interface McpConfig {
  mcpServers: Record<string, McpServerDef>;
}

export type McpImportSource = 'claude' | 'claude-code' | 'codex' | 'cursor' | 'lmstudio';

/** `get_mcp_import_sources` row: [source, config path (if resolvable), file exists]. */
export type McpImportSourceInfo = [McpImportSource, string | null, boolean];

/** Minimal JSON Schema shape used for tool parameters. */
export interface JsonSchema {
  type?: string | string[];
  description?: string;
  properties?: Record<string, JsonSchema>;
  required?: string[];
  items?: JsonSchema;
  enum?: unknown[];
  default?: unknown;
  [key: string]: unknown;
}

/** OpenAI-style function tool definition, as sent in `tools: [...]`. */
export interface ToolDefinition {
  type: 'function';
  function: {
    name: string;
    description?: string;
    parameters?: JsonSchema;
  };
}

/** A tool as the Playground lists it (the core's registry: `coreToolAsServerTool`). */
export interface ServerTool {
  display_name: string;
  tool: string;
  /**
   * "harness" = Harness's own built-in tool (core), "mcp" = a tool of an MCP server run by
   * Harness's own MCP client (core; name `<server>_<tool>`).
   */
  type: 'harness' | 'mcp' | string;
  /** Core tools (`harness`, `mcp`): the permission class. */
  class?: 'safe' | 'restricted' | 'dangerous';
  /** MCP tools: the server they come from. */
  server?: string | null;
  permissions: { write: boolean };
  uses_cwd: boolean;
  definition: ToolDefinition;
}

/** A complete tool call from the model (OpenAI `tool_calls[]` item). */
export interface ToolCall {
  id: string;
  type: 'function';
  function: { name: string; arguments: string };
}
