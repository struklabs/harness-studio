// Client of the Harness tool engine (`harness_core::tools`) through the `tools_*` Tauri
// commands: the tool list with the policy decision of each tool, running one call, cancelling
// it, the policy settings and the tool log. The rules (policy, working directory, limits, log)
// live in the core; nothing is decided here.

import { invoke } from '#lib/ipc.js';
import { Channel } from '@tauri-apps/api/core';
import type { JsonSchema, ServerTool } from './mcp-types';
import type { SearchProvider } from './types';
import type { TextCallRecovery } from './components/playground/textToolCalls';

export function parseTextToolCalls(content: string, names: string[]): Promise<TextCallRecovery> {
  return invoke('tools_recover_text_calls', { content, names });
}

export type { SearchProvider } from './types';

export type PermissionClass = 'safe' | 'restricted' | 'dangerous';
export type ToolPolicy = 'off' | 'ask_every_time' | 'allow_trusted' | 'allow_all';
/** What a call would do under the current policy. */
export type ToolDecision = 'run' | 'ask' | 'refuse';

export const TOOL_POLICIES: ToolPolicy[] = ['off', 'ask_every_time', 'allow_trusted', 'allow_all'];

/** `settings.tools`. */
export interface ToolSettings {
  policy: ToolPolicy;
  /** Tools the user trusts; they only count under "allow_trusted". */
  trusted: string[];
  /** Default working directory ('' = none). */
  workdir: string;
  /** Saved folders, most recently used first. */
  workspaces?: string[];
  /** The user's class for an MCP tool (registry name), over what its annotations say. */
  mcp_classes?: Record<string, PermissionClass>;
  web_search: { provider: SearchProvider; searxng_url: string };
  /** Offer the built-in tools to MCP clients (`harness mcp`, `/mcp`) too. Off by default. */
  mcp_expose?: boolean;
}

export interface WebSearchSettings { provider: SearchProvider; searxng_url: string; api_key_set: boolean }
export interface SetWebSearchSettings { provider: SearchProvider; searxng_url: string; api_key?: string | null }

/** Limits every surface shares (core `ToolLimits`). */
export interface ToolLimits {
  max_rounds: number;
  timeout_secs: number;
  max_result_chars: number;
}

export interface CoreTool {
  name: string;
  description: string;
  input_schema: JsonSchema;
  class: PermissionClass;
  source: 'builtin' | 'mcp' | 'installed' | 'generated';
  uses_workspace: boolean;
  decision: ToolDecision;
  /** MCP tools: the server they come from. */
  server?: string | null;
  /** The class before the user's own choice (an MCP tool's annotation class); `class` is the one in force. */
  default_class?: PermissionClass | null;
}

export interface ToolListing {
  policy: ToolPolicy;
  trusted: string[];
  workdir: string;
  workspaces: string[];
  missing_workspaces: string[];
  web_search: { provider: SearchProvider; searxng_url: string };
  mcp_expose?: boolean;
  limits: ToolLimits;
  tools: CoreTool[];
}

/** Error codes of a failed call (core `ToolErrorCode`). */
export type ToolErrorCode =
  | 'unknown_tool'
  | 'invalid_arguments'
  | 'policy_off'
  | 'approval_required'
  | 'no_workspace'
  | 'outside_workspace'
  | 'not_found'
  | 'io'
  | 'failed'
  | 'timeout'
  | 'cancelled'
  | 'not_configured'
  | 'network'
  | 'response_too_large'
  | 'unsupported_content_type';

export interface CoreToolResult {
  id: string;
  /** The text the model reads (the error sentence when `error` is set). */
  content: string;
  error: ToolErrorCode | null;
  duration_ms: number;
  truncated: boolean;
  network_target: string | null;
}

export interface ToolRunRequest {
  /** Unique per run: `cancelCoreTool` cancels by it and it is the tool log's row id. */
  call_id: string;
  tool: string;
  args: Record<string, unknown>;
  /** Absent/empty follows the live default; 'harness://no-workspace' explicitly opts out. */
  workdir?: string;
  conversation?: string | null;
  model?: string | null;
  /** The user was asked (the decision was `ask`) and said yes: this call only. */
  approved?: boolean;
}

export interface ToolLogRow {
  id: string;
  started_at_ms: number;
  client: string;
  conversation: string | null;
  model: string | null;
  tool: string;
  class: PermissionClass | string;
  source: string;
  args: string;
  approved: boolean;
  error_code: string | null;
  duration_ms: number;
  result_chars: number;
  truncated: boolean;
  network_target: string | null;
}

export interface ToolLogQuery {
  tool?: string | null;
  client?: string | null;
  from_ms?: number | null;
  before_ms?: number | null;
  limit?: number | null;
}

export const listCoreTools = () => invoke<ToolListing>('tools_list');
export const coreToolDecision = (tool: string) => invoke<ToolDecision | null>('tools_decision', { tool });
export const runCoreTool = (req: ToolRunRequest) => invoke<CoreToolResult>('tools_run', { req });
/** Logs a call the user refused (nothing runs). */
export const denyCoreTool = (req: Omit<ToolRunRequest, 'approved'>) => invoke<void>('tools_deny', { req });
export const cancelCoreTool = (callId: string) => invoke<boolean>('tools_cancel', { callId });
export const getToolSettings = () => invoke<ToolSettings>('tools_get_settings');
export const setToolSettings = (settings: ToolSettings) => invoke<ToolSettings>('tools_set_settings', { settings });
export type McpServerState = 'disabled' | 'connected' | 'failed' | 'idle' | 'connecting';

/** Harness's own MCP client: one configured server (core `McpServerStatus`). */
export interface McpServerStatus {
  name: string;
  /** `sse` is the older HTTP+SSE transport (2024-11-05). */
  transport: 'stdio' | 'http' | 'sse';
  state: McpServerState;
  error: string | null;
  tools: number;
  /** Negotiated MCP version (`2026-07-28`, `2025-11-25`...). */
  protocol: string | null;
  server: string | null;
  /** OAuth state of a remote server: `required` = it answered 401, `authorized` = a sign-in is stored. */
  auth?: 'required' | 'authorized' | null;
  /** The connected server offers resources. */
  resources?: boolean;
}

export interface McpResource {
  uri: string;
  name: string;
  title: string | null;
  description: string | null;
  mime_type: string | null;
  size: number | null;
}

export interface McpResourceTemplate {
  uri_template: string;
  name: string;
  title: string | null;
  description: string | null;
  mime_type: string | null;
}

export interface McpResourceList {
  resources: McpResource[];
  templates: McpResourceTemplate[];
}

/** The resources and templates of a connected MCP server. */
export const listMcpResources = (server: string) => invoke<McpResourceList>('tools_mcp_resources', { server });
/** Reads a resource and saves it as a file the document reader understands; returns its path. */
export const mcpResourceToFile = (server: string, uri: string, name?: string | null) =>
  invoke<string>('tools_mcp_resource_to_file', { server, uri, name: name ?? null });
/** Opens the browser for the server's OAuth sign-in and waits for the approval. */
export const authorizeMcpServer = (server: string) => invoke<McpServerStatus[]>('tools_mcp_authorize', { server });
export const cancelMcpAuthorize = (server: string) => invoke<boolean>('tools_mcp_cancel_authorize', { server });
export const logoutMcpServer = (server: string) => invoke<McpServerStatus[]>('tools_mcp_logout', { server });

/** State of every configured MCP server; `retry` tries again those that failed to start. */
export const getMcpStatus = (retry = false) => invoke<McpServerStatus[]>('tools_mcp_status', { retry });
/** Sets (`null` clears) the user's permission class for one MCP tool. */
export const setMcpToolClass = (tool: string, cls: PermissionClass | null) =>
  invoke<ToolSettings>('tools_set_mcp_class', { tool, class: cls });
export const getWebSearchSettings = () => invoke<WebSearchSettings>('tools_get_search_provider');
export const setWebSearchSettings = (settings: SetWebSearchSettings) => invoke<WebSearchSettings>('tools_set_search_provider', { settings });
export const fetchToolLog = (query: ToolLogQuery) => invoke<ToolLogRow[]>('tools_log', { query });

/** A core tool in the shape the Playground's tool list uses (`type: 'harness'`, or `'mcp'` for an MCP server's tool). */
export function coreToolAsServerTool(t: CoreTool): ServerTool {
  return {
    display_name: t.name,
    tool: t.name,
    type: t.source === 'mcp' ? 'mcp' : 'harness',
    server: t.server ?? null,
    permissions: { write: t.class === 'dangerous' },
    uses_cwd: t.uses_workspace,
    class: t.class,
    definition: { type: 'function', function: { name: t.name, description: t.description, parameters: t.input_schema } },
  };
}

/** A fresh id for one tool run (see `ToolRunRequest.call_id`). */
export function newRunId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `run_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
}

/** A package under `<data dir>/tools/` (`harness_core::tools::installed::InstalledTool`). */
export interface InstalledTool {
  category: string;
  name: string;
  version: string;
  description: string;
  output: string;
  /** [] denies network; ['*'] permits public internet; otherwise listed domains only. */
  network: string[];
  /** Workspace folders it may read / write. */
  fs_read: string[];
  fs_write: string[];
  class: PermissionClass;
  enabled: boolean;
  has_test: boolean;
  readme: string | null;
  /** Keyed error when the package no longer validates. */
  error: string | null;
}

export const listInstalledTools = () => invoke<InstalledTool[]>('installed_tools_list');
/** Validates a package folder and returns what it asks for; installs nothing. */
export const inspectInstalledTool = (path: string) => invoke<InstalledTool>('installed_tools_inspect', { path });
export const installInstalledTool = (path: string) => invoke<InstalledTool>('installed_tools_install', { path });
export const removeInstalledTool = (name: string) => invoke<void>('installed_tools_remove', { name });
export const setInstalledToolEnabled = (name: string, enabled: boolean) =>
  invoke<void>('installed_tools_set_enabled', { name, enabled });

/** One case of a package's test (`harness_core::tools::package_test::CaseReport`). */
export interface TestCase {
  name: string;
  passed: boolean;
  /** '' when passed, else contains / equals / error_expected / error_mismatch / unexpected_error / bad_case. */
  reason: string;
  expected: string;
  actual: string;
}

export interface TestReport {
  passed: boolean;
  cases: TestCase[];
  /** The test could not run at all (plain engine text). */
  error: string | null;
  warnings?: string[];
}

/** Runs the `test` file of an installed package in the sandbox. */
export const testInstalledTool = (name: string) => invoke<TestReport>('installed_tools_test', { name });

/** What the user typed into the generator: the tool they want and the upper bound of its permissions. */
export interface GenerateToolRequest {
  description: string;
  /** Upper bound: [] = none, ['*'] = public internet, otherwise listed domains only. */
  network: string[];
  fs_read: string[];
  fs_write: string[];
  /** The chat model's endpoint (the Playground's own). */
  endpoint: { base_url: string; headers: Record<string, string>; model: string | null };
}

export interface GeneratorUsage { prompt_tokens: number; completion_tokens: number; estimated: boolean }
export interface GeneratorAttempt { number: number; change: string; failed_cases: number; total_cases?: number; feedback: string; usage: GeneratorUsage; repeated: boolean; strategy: string; compacted: boolean }
export interface GeneratorEstimate { low: number; high: number; prompt_per_tool: number; reply_per_tool: number; historical: boolean }

/** A generated package waiting for the user's decision (`harness_core::tools::generator::Draft`). */
export interface ToolDraft {
  id: string;
  info: InstalledTool;
  manifest_json: string;
  code: string;
  test: string;
  report: TestReport;
  /** Chat completions it took (1 = the first reply was right). */
  attempts: number;
  history?: GeneratorAttempt[];
  usage?: GeneratorUsage;
  /** Code kept returning the same value while changing: the test expectation is probably wrong. */
  suspect?: { case: string; expected: string; actual: string } | null;
}

/** Models below this many parameters rarely write a working tool (`generator::SMALL_MODEL_PARAMS`). */
export const SMALL_MODEL_PARAMS = 7_000_000_000;

export const generateTool = (request: GenerateToolRequest, onProgress?: (progress: string) => void) => {
  const onProgressChannel = new Channel<string>();
  onProgressChannel.onmessage = progress => onProgress?.(progress);
  return invoke<ToolDraft>('generated_tool_generate_observed', { request, onProgress: onProgressChannel });
};
export const cancelToolGeneration = () => invoke<boolean>('generated_tool_cancel');
/** Registers the draft for this app session (source `generated`). */
export const approveGeneratedTool = (id: string) => invoke<InstalledTool>('generated_tool_approve', { id });
export const discardToolDraft = (id: string) => invoke<void>('generated_tool_discard', { id });
/** Writes a tool generated in this session to `<data dir>/tools/<name>/`. */
export const saveGeneratedTool = (name: string) => invoke<InstalledTool>('generated_tool_save', { name });
export const removeGeneratedTool = (name: string) => invoke<boolean>('generated_tool_remove', { name });
/** `[names generated in this session, limit]`. */
export const generatedToolsSession = () => invoke<[string[], number]>('generated_tools_session');
export const modelParams = (path: string) => invoke<number | null>('generated_tool_model_params', { path });
