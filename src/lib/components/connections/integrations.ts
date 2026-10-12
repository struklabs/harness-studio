// Data access for client integrations. ALL backend calls for this feature live here so
// ClientIntegrations.svelte only deals with presentation.
import { invoke } from '$lib/ipc';

export type IntegrationId = string;

/** `config`: Harness writes the tool's file. `guide`: steps + copyable fields. `sdk`: code only. */
export type IntegrationKind = 'config' | 'guide' | 'sdk';
export type WireApi = 'anthropic' | 'responses' | 'chat';
export type LaunchToolId = 'claude' | 'codex' | 'opencode' | 'aider' | 'goose';

export interface Integration {
  id: IntegrationId;
  name: string;
  homepage: string | null;
  docsUrl: string | null;
  kind: IntegrationKind;
  /** True when the app can write this tool's config itself right now. */
  applyable: boolean;
  /** The tool supports automatic setup, but the current context window is too small. */
  blockedByContext: boolean;
  /** Smallest server context the tool works with (0 = no requirement). */
  minCtx: number;
  /** null = not detectable. */
  installed: boolean | null;
  configured: boolean;
  configPaths: string[];
  /** Model id the tool's own config currently uses for Harness (null = none / not readable). */
  configuredModel: string | null;
  /** The tool's own default model as written (OpenCode / Kilo `model`); null = unset. */
  defaultModel: string | null;
  /** Snippet rendered by the backend; the UI may replace it with a richer one. */
  backendSnippet: string | null;
  /** `harness launch <tool>` / "Başlat" works for this tool. */
  launch: LaunchToolId | null;
  /** API used by "Test connection" (null: no test, e.g. SDK snippets). */
  testApi: WireApi | null;
  /** Apply asks for a file path (LibreChat). */
  needsPath: boolean;
  /** Files changed after Harness last wrote them. */
  drifted: string[];
  /** Timestamped backups, oldest first. */
  backups: string[];
  appliedAt: number | null;
}

export interface ApplyContext {
  baseUrl: string;
  apiKey: string | null;
  modelId: string;
  contextSize: number;
  /** LibreChat: the librechat.yaml path. */
  targetPath?: string | null;
  /** OpenCode-format providers: every model to list (the chosen one is always listed). */
  models?: ProviderModel[];
  /** Point the tool's default model at `modelId` (true), leave it (false), or the backend rule (unset/harness only). */
  setDefaultModel?: boolean | null;
}

export interface ProviderModel {
  id: string;
  /** 0 = the request's context size. */
  context_size: number;
}

/** Integrations whose config is an OpenCode-format provider block (several models, a default model). */
export const OPENCODE_FORMAT = new Set(['opencode', 'kilo', 'opendesign']);

export interface FileChange {
  path: string;
  before: string | null;
  after: string;
}

export interface ApplyResult {
  message: string;
  written: string[];
  backups: string[];
  warnings: string[];
}

export interface RevertOutcome {
  written: string[];
  deleted: string[];
  backups: string[];
  warnings: string[];
}

export interface ConnectionTest {
  ok: boolean;
  api: WireApi;
  url: string;
  status: number | null;
  latency_ms: number;
  tool_called: boolean;
  model: string | null;
  detail: string;
}

interface IntegrationInfo {
  id: string;
  name: string;
  homepage: string;
  docs_url: string;
  kind: 'config' | 'guide';
  detectable: boolean;
  installed: boolean;
  configured: boolean;
  can_apply: boolean;
  min_ctx: number;
  config_paths: string[];
  configured_model: string | null;
  default_model: string | null;
  snippet: string;
  launch: LaunchToolId | null;
  test_api: WireApi;
  needs_path: boolean;
  drifted: string[];
  backups: string[];
  applied_at: number | null;
}

/** SDK / API snippets (no tool to configure). */
const SDKS: [string, string][] = [
  ['curl', 'cURL'],
  ['openai-python', 'OpenAI SDK (Python)'],
  ['openai-js', 'OpenAI SDK (JavaScript)'],
  ['anthropic-python', 'Anthropic SDK (Python)'],
  ['langchain', 'LangChain'],
  ['llamaindex', 'LlamaIndex'],
  ['vercel-ai', 'Vercel AI SDK'],
];

function sdkItems(): Integration[] {
  return SDKS.map(([id, name]) => ({
    id,
    name,
    homepage: null,
    docsUrl: null,
    kind: 'sdk',
    applyable: false,
    blockedByContext: false,
    minCtx: 0,
    installed: null,
    configured: false,
    configPaths: [],
    configuredModel: null,
    defaultModel: null,
    backendSnippet: null,
    launch: null,
    testApi: null,
    needsPath: false,
    drifted: [],
    backups: [],
    appliedAt: null,
  }));
}

/** Shown before (or if) detection fails. */
export function baseCatalog(): Integration[] {
  return sdkItems();
}

/** `check_integration_alias_drift`: a configured tool naming models Harness no longer serves. */
export interface AliasDrift {
  id: string;
  name: string;
  /** Every model name the tool's config holds for Harness (default first). */
  configured: string[];
  /** The ones no definition / server answers to. */
  missing: string[];
  /** Replacement for the default model when one matches unambiguously. */
  suggestion: string | null;
  /** Hand-written provider blocks that point at the Harness gateway (never rewritten). */
  user_blocks: string[];
}

/** Tools whose config names models that no longer exist; `served` = names a running server answers to. */
export function checkAliasDrift(served: string[]): Promise<AliasDrift[]> {
  return invoke<AliasDrift[]>('check_integration_alias_drift', { served });
}

/** `check_integration_endpoint_drift`: a configured tool whose saved address is not the gateway's. */
export interface EndpointDrift {
  id: string;
  name: string;
  /** Base URL Harness wrote into the tool's config. */
  configured: string;
  /** The gateway's base URL now. */
  current: string;
}

/** Tools set up for another address than the running gateway's `baseUrl`. */
export function checkEndpointDrift(baseUrl: string): Promise<EndpointDrift[]> {
  return invoke<EndpointDrift[]>('check_integration_endpoint_drift', { baseUrl });
}

export async function listIntegrations(ctx: ApplyContext): Promise<Integration[]> {
  const list = await invoke<IntegrationInfo[]>('list_integrations', {
    baseUrl: ctx.baseUrl,
    modelId: ctx.modelId,
    contextSize: ctx.contextSize,
  });
  return [
    ...list.map(
      (i): Integration => ({
        id: i.id,
        name: i.name,
        homepage: i.homepage || null,
        docsUrl: i.docs_url || null,
        kind: i.kind,
        applyable: i.can_apply,
        blockedByContext: !i.can_apply && i.kind === 'config',
        minCtx: i.min_ctx ?? 0,
        installed: i.detectable ? i.installed : null,
        configured: i.configured,
        configPaths: i.config_paths,
        configuredModel: i.configured_model ?? null,
        defaultModel: i.default_model ?? null,
        backendSnippet: i.snippet || null,
        launch: i.launch,
        testApi: i.test_api,
        needsPath: i.needs_path,
        drifted: i.drifted ?? [],
        backups: i.backups ?? [],
        appliedAt: i.applied_at ?? null,
      })
    ),
    ...sdkItems(),
  ];
}

function request(id: IntegrationId, ctx: ApplyContext) {
  const modelName = ctx.modelId.split(/[\\/]/).pop()?.replace(/\.gguf$/i, '') || ctx.modelId;
  return {
    id,
    baseUrl: ctx.baseUrl,
    apiKey: ctx.apiKey,
    modelId: ctx.modelId,
    modelName,
    contextSize: ctx.contextSize,
    targetPath: ctx.targetPath ?? null,
    models: ctx.models ?? null,
    setDefaultModel: ctx.setDefaultModel ?? null,
  };
}

/** The files that would change, without writing anything. */
export function previewIntegration(id: IntegrationId, ctx: ApplyContext): Promise<FileChange[]> {
  return invoke<FileChange[]>('preview_integration', request(id, ctx));
}

/** Writes the tool's config (timestamped backup + ledger entry). */
export function applyIntegration(id: IntegrationId, ctx: ApplyContext): Promise<ApplyResult> {
  return invoke<ApplyResult>('apply_integration', request(id, ctx));
}

/** Removes (or restores) only the keys Harness set. */
export function revertIntegration(id: IntegrationId): Promise<RevertOutcome> {
  return invoke<RevertOutcome>('revert_integration', { id });
}

/** One real small request with a tool, in the API the tool speaks. */
export function testConnection(api: WireApi, ctx: ApplyContext): Promise<ConnectionTest> {
  return invoke<ConnectionTest>('test_integration_connection', {
    api,
    baseUrl: ctx.baseUrl,
    apiKey: ctx.apiKey,
    model: ctx.modelId,
  });
}

/** tool id -> executable path (null = not on PATH). */
export async function launchTools(): Promise<Record<string, string | null>> {
  const rows = await invoke<[string, string | null][]>('launch_tools');
  return Object.fromEntries(rows);
}

export interface LaunchRequest {
  tool: LaunchToolId;
  alias: string;
  baseUrl: string;
  apiKey: string | null;
  contextSize: number | null;
  managed: boolean;
  cwd: string | null;
  hideExternalSkills: boolean;
}

/** Starts the tool in a new terminal window (Windows); resolves with the redacted plan. */
export function launchTool(r: LaunchRequest): Promise<{ pid: number; program: string; plan: unknown }> {
  return invoke('launch_integration_tool', { ...r });
}

/** Copy-ready CLI equivalent of "Başlat". */
export function launchCommand(tool: LaunchToolId, alias: string, hideExternalSkills = true): string {
  return `harness launch ${tool} --model ${alias}${tool === 'opencode' && !hideExternalSkills ? ' --keep-skills' : ''}`;
}

/** An OpenCode desktop app (mirrors `harness_core::integrations::opencode_desktop::DesktopApp`). */
export interface DesktopApp {
  path: string;
  /** The full CLI this build ships (`opencode-cli`), or null. */
  cli: string | null;
  version: string | null;
  source: 'known_path' | 'registry';
  /** Running now: it reads the config only at start. */
  running: boolean;
}

export function opencodeDesktopApps(): Promise<DesktopApp[]> {
  return invoke<DesktopApp[]>('opencode_desktop_apps');
}

/** Opens a desktop app found by `opencodeDesktopApps` (it reads the persistent config). */
export function openOpencodeDesktop(path: string, hideExternalSkills = true): Promise<number> {
  return invoke<number>('open_opencode_desktop', { path, hideExternalSkills });
}
