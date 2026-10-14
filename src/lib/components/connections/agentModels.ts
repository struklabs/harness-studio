// Models an agent tool can use through Harness, with their tool-calling verdict and prompt
// speed (`harness_core::integrations::agent_models`). The verdict comes from real answers (a
// tool check, the gateway log) before the chat template; no model names are hard-coded.
import { invoke } from '#lib/ipc.js';

export type ToolSupport = 'verified' | 'failed' | 'template_only' | 'no_template' | 'unknown';
export type ToolEvidence = 'check' | 'gateway_log' | 'template' | 'none';
/** How reliably a model calls tools, learned from the gateway's diagnosed tool rounds. */
export type ToolLevel = 'native' | 'generic' | 'limited';
/** Tool rounds needed before a level is stated (`gateway::tool_diag::level::MIN_ROUNDS`). */
export const TOOL_MIN_ROUNDS = 5;

export interface ToolCheck {
  alias: string;
  model_path: string | null;
  chat_template: string | null;
  tool_called: boolean;
  /** The answer was text that looks like a tool call (not parsed by llama-server). */
  text_call: boolean;
  detail: string;
  latency_ms: number;
  checked_at: number;
}

export interface AgentModel {
  alias: string;
  definition_id: string | null;
  model_path: string | null;
  chat: boolean | null;
  context_size: number | null;
  params: number | null;
  size_label: string | null;
  file_bytes: number | null;
  template_tools: boolean | null;
  tools: ToolSupport;
  evidence: ToolEvidence;
  check: ToolCheck | null;
  native_tool_calls: number;
  /** Tool rounds with a clean call / rounds the model or its template failed. */
  tool_rounds_ok: number;
  tool_rounds_bad: number;
  /** null: too few rounds yet. */
  tool_level: ToolLevel | null;
  /** Most tools worth enabling at once (model size and level). */
  recommended_tools: number;
  prompt_tps: number | null;
  prompt_tokens_timed: number;
  /** Prompt speed of the newest benchmark of this file (tokens/s). */
  bench_prompt_tps?: number | null;
}

/** Where a prompt speed came from. */
export type PromptSpeedSource = 'gateway' | 'benchmark' | 'live';
export interface PromptSpeed {
  tps: number;
  source: PromptSpeedSource;
}

/** A first answer slower than this many seconds gets a warning. */
export const FIRST_REPLY_WARN_SECONDS = 60;

// USER_TEST_20261010 S19 logging proxy: 13,544 tokens with both skill flags, ~31k
// with the user's external skills. Approximate totals: installed skills/tools vary.
export const OPENCODE_PROMPT_TOKENS = 31_000;
export function opencodePromptTokens(hideExternalSkills: boolean): number {
  return hideExternalSkills ? 13_500 : OPENCODE_PROMPT_TOKENS;
}

/** Below this many measured prompt tokens the speed is dominated by short prompts. */
const MIN_TIMED_TOKENS = 2_000;

export function listAgentModels(): Promise<AgentModel[]> {
  return invoke<AgentModel[]>('list_agent_models');
}

/** One real request with a tool (through the gateway this loads the model if needed). */
export function checkToolCalling(baseUrl: string, apiKey: string | null, alias: string): Promise<ToolCheck> {
  return invoke<ToolCheck>('check_model_tool_calling', { baseUrl, apiKey, alias });
}

/** The agent model a served id belongs to: same alias, else the same GGUF file name. */
export function findAgentModel(models: AgentModel[], id: string | null | undefined): AgentModel | undefined {
  if (!id) return undefined;
  const lower = id.toLowerCase();
  const stem = (p: string | null) => p?.split(/[\\/]/).pop()?.replace(/\.gguf$/i, '').toLowerCase();
  return (
    models.find((m) => m.alias.toLowerCase() === lower) ??
    models.find((m) => stem(m.model_path) === lower.replace(/\.gguf$/i, '').split(/[\\/]/).pop())
  );
}

/** Backend already excludes speech/embedding/rerank; this also handles explicit false rows. */
export { chatModelsOnly } from '../../chatModels';

/** Only a running chat endpoint provides context evidence; an unloaded gateway model does not. */
export function hasChatContext(managed: boolean, loadedChat: boolean, running: boolean, mode?: string | null): boolean {
  return managed ? loadedChat : running && mode === 'chat';
}

/** Tool calls will not work as the agent expects. */
export function toolWarning(m: AgentModel | undefined): boolean {
  return m?.tools === 'failed' || m?.tools === 'no_template';
}

/** Other chat models that may do native tool calls: verified first, then template-capable. */
export function toolSuggestions(models: AgentModel[], alias: string): AgentModel[] {
  const rank = (m: AgentModel) => (m.tools === 'verified' ? 0 : m.tools === 'template_only' ? 1 : 2);
  return models
    .filter((m) => m.alias !== alias && m.chat !== false && rank(m) < 2)
    .sort((a, b) => rank(a) - rank(b) || (b.params ?? 0) - (a.params ?? 0))
    .slice(0, 3);
}

/**
 * Available prompt speed of a model: aggregate gateway timing, else the
 * newest benchmark, else the running server's own average (`liveTps`, mostly short prompts, so
 * the last resort). Aggregate token counts do not prove any one request was long;
 * callers must present this as a lower bound. Null = never measured.
 */
export function promptSpeed(m: AgentModel | undefined, liveTps?: number | null): PromptSpeed | null {
  if (m?.prompt_tps && m.prompt_tokens_timed >= MIN_TIMED_TOKENS) return { tps: m.prompt_tps, source: 'gateway' };
  if (m?.bench_prompt_tps) return { tps: m.bench_prompt_tps, source: 'benchmark' };
  if (liveTps && liveTps > 0) return { tps: liveTps, source: 'live' };
  return null;
}

/** Seconds the first answer waits for `tokens` of prompt at this speed. */
export function firstReplySeconds(speed: PromptSpeed, tokens: number): number {
  return tokens / speed.tps;
}
