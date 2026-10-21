export type GpuVendor =
  | 'Nvidia'
  | 'Amd'
  | 'Intel'
  | 'Apple'
  | { Other: string };

export interface GpuInfo {
  index: number;
  name: string;
  vendor: GpuVendor;
  vram_bytes: number | null;
  driver_version: string | null;
  is_discrete: boolean;
  is_default: boolean;
}

export interface DriverCompatibility {
  cuda_supported: boolean;
  cuda_driver_version: string | null;
  vulkan_supported: boolean;
  metal_supported: boolean;
  /** AMD GPU usable by the official ROCm/HIP build (Windows: AMD GPU; Linux: /dev/kfd). */
  rocm_supported?: boolean;
  wsl?: boolean;
}

export type RecommendedBackend = 'Cuda12' | 'Vulkan' | 'Metal' | 'Cpu' | 'Rocm' | 'Sycl';

/** Why a backend is recommended (`reason_key` is an i18n key). */
export interface BackendAdvice {
  recommended: RecommendedBackend;
  reason_key: string;
  gpu_name: string | null;
  alternatives: RecommendedBackend[];
}

/** `get_backend_status`: installed managed build vs. the recommendation. */
export interface BackendStatus {
  rocm_unsupported_gpu: string | null;
  /** `vulkan`, `rocm`, `cuda`, `sycl`, `cpu`, `metal`; null = no managed build. */
  installed: string | null;
  advice: BackendAdvice;
  official: RecommendedBackend[];
}

export interface SystemHardwareInfo {
  os: string;
  os_version: string;
  arch: string;
  cpu_name: string;
  cpu_cores: number;
  total_ram_bytes: number;
  available_ram_bytes: number;
  gpus: GpuInfo[];
  primary_gpu_index?: number | null;
  driver_info: DriverCompatibility;
  recommended_backend: RecommendedBackend;
  backend_advice?: BackendAdvice;
  /** Apple Silicon: GPU share of unified memory. */
  unified_memory_budget_bytes?: number | null;
  intel_mac?: boolean;
}

export type RuntimeMode = 'Managed' | 'Custom';

export interface ManagedRuntimeConfig {
  installed_version: string | null;
  previous_version?: string | null;
  auto_update: boolean;
  backend: string | null;
}

export interface CustomBinaryConfig {
  binary_path: string | null;
  version: string | null;
  supports_metrics: boolean;
  is_valid: boolean;
}

export interface RuntimeDetectionInfo {
  managedInstalled: boolean;
  managedVersion: string | null;
  managedPath: string | null;
  systemDetected: boolean;
  systemPath: string | null;
  systemVersion: string | null;
  systemSupportsMetrics: boolean;
  activeMode: RuntimeMode;
  resolvedBinaryPath: string | null;
}

export interface ServerConfig {
  host: string;
  port: number;
  default_ngl: number;
  default_ctx: number;
  /** Seconds a start waits for `/health` (a launch can set its own `load_timeout`). */
  load_timeout_secs?: number;
  /** Restart a server automatically after it crashed while running. */
  auto_restart?: boolean;
  /** Restarts before giving up (delay 2, 4, 8… s). */
  restart_max_attempts?: number;
  /** Short llama-bench on a model's first load on an integrated GPU (default on). */
  auto_benchmark?: boolean;
}

export interface PrivacySettings {
  onboarding_completed: boolean;
  crash_reports: boolean;
  retention_days?: number;
  app_errors: boolean;
  llama_errors: boolean;
  hardware_info: boolean;
  performance_metrics: boolean;
  anonymous_analytics: boolean;
}

export interface AppSettings {
  audio_cpp_enabled: boolean;
  runtime_mode: RuntimeMode;
  managed_runtime: ManagedRuntimeConfig;
  custom_binary: CustomBinaryConfig;
  server: ServerConfig;
  privacy: PrivacySettings;
  models_dir: string;
  extra_models_dirs: string[];
  /** Load a chat model automatically when its download finishes (default off). */
  auto_load_after_download?: boolean;
  /** "Don't Ask Again" of the save-load-settings prompt (default off = ask). */
  no_save_load_prompt?: boolean;
  /** The token itself never reaches the frontend (it lives in the OS keychain). */
  hf_token_set?: boolean;
  /** Client id of the Hugging Face OAuth app (public; empty = env/build value). */
  hf_oauth_client_id?: string;
  /** Legacy base mode; kept in sync with `color_theme` so older builds open sensibly. */
  theme?: 'dark' | 'light' | 'system';
  /** Full theme id (see #lib/theme.ts `ThemeId`); empty/absent = use `theme`. */
  color_theme?: string;
  /** Colour-vision-safe status and chart colours on top of any theme. */
  cvd_safe?: boolean;
  language?: 'tr' | 'en';
  /** Interface size in percent; null/absent = never chosen (screen-based default). */
  ui_scale?: number | null;
  ui_level?: 'simple' | 'advanced' | null;
  startup: StartupSettings;
  /** Settings > Network & security (absent in settings of older builds). */
  network?: NetworkSettings;
  /** Settings > Developer > Control API; changed only through `set_control_api_settings`. */
  control_api?: ControlApiSettings;
  /** Settings > Gateway; changed only through `set_gateway_settings`. */
  gateway?: GatewaySettings;
  /** Analytics; changed only through `set_analytics_settings`. */
  analytics?: AnalyticsSettings;
  /** Settings > OpenTelemetry; changed only through `save_otel_settings`. */
  otel?: OtelSettings;
}

// --- Request analytics (harness_core::analytics) ---

export interface AnalyticsSettings {
  enabled: boolean;
  raw_retention_days: number;
}

export interface RecorderStatus {
  running: boolean;
  written: number;
  dropped: number;
  skipped: number;
  write_errors: number;
  last_error?: string | null;
}

export interface BenchmarkRef {
  run_id: string;
  finished_at: number;
  prompt_tps?: number | null;
  n_prompt?: number | null;
  gen_tps?: number | null;
  n_gen?: number | null;
}

export interface ModelStats {
  /** `*` for the total row; empty when the request never resolved a model. */
  alias: string;
  definition_id?: string | null;
  requests: number;
  errors: number;
  ooms: number;
  cancelled: number;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
  avg_gen_tps?: number | null;
  avg_prompt_tps?: number | null;
  avg_ttft_ms?: number | null;
  ttft_p50_ms?: number | null;
  ttft_p95_ms?: number | null;
  latency_p50_ms?: number | null;
  latency_p95_ms?: number | null;
  percentile_samples: number;
  vram_avg_bytes?: number | null;
  vram_peak_bytes?: number | null;
  ram_peak_bytes?: number | null;
  error_rate: number;
  oom_rate: number;
  busy_ms: number;
  loads: number;
  avg_load_ms?: number | null;
  spec_accept_rate?: number | null;
  last_used_ms?: number | null;
  benchmark?: BenchmarkRef | null;
}

export interface ClientShare {
  client: string;
  requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  share: number;
  token_share: number;
}

export interface DailyPoint {
  day_start_ms: number;
  requests: number;
  errors: number;
  prompt_tokens: number;
  completion_tokens: number;
}

export interface DirectServer {
  server_id: string;
  label: string;
  prompt_tokens: number;
  completion_tokens: number;
}

export interface DirectDay {
  day_start_ms: number;
  prompt_tokens: number;
  completion_tokens: number;
}

/** Tokens of the llama-server ports that bypass the gateway (aggregate only). */
export interface DirectUsage {
  prompt_tokens: number;
  completion_tokens: number;
  servers: DirectServer[];
  daily: DirectDay[];
}

export interface AnalyticsSummary {
  from_ms?: number | null;
  to_ms?: number | null;
  generated_at_ms: number;
  totals: ModelStats;
  models: ModelStats[];
  clients: ClientShare[];
  daily: DailyPoint[];
  raw_since_ms?: number | null;
  rollup_days: number;
  retention_days: number;
  recorder: RecorderStatus;
  direct?: DirectUsage;
}

export interface RequestRow {
  id: string;
  started_at_ms: number;
  endpoint: string;
  model?: string | null;
  alias?: string | null;
  definition_id?: string | null;
  server_id?: string | null;
  client?: string | null;
  user_agent?: string | null;
  harness_client?: string | null;
  token_name?: string | null;
  stream: boolean;
  status: number;
  error_code?: string | null;
  oom: boolean;
  cancelled: boolean;
  queue_wait_ms: number;
  load_ms?: number | null;
  evicted: string[];
  ttft_ms?: number | null;
  total_ms: number;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  cached_tokens?: number | null;
  prompt_ms?: number | null;
  predicted_ms?: number | null;
  prompt_per_second?: number | null;
  predicted_per_second?: number | null;
  draft_n?: number | null;
  draft_accepted?: number | null;
  finish_reason?: string | null;
  ctx_size?: number | null;
  context_fill?: number | null;
  vram_peak_bytes?: number | null;
  ram_peak_bytes?: number | null;
  backend?: string | null;
  compat: string[];
  content_captured: boolean;
  slots?: number | null;
  waited?: string | null;
  /** The request offered tools, carried tool results or the response made a call. */
  tool_involved: boolean;
  tools_offered: number;
  tool_calls: number;
  /** Who is at fault in a failed tool round: runtime | context | model | client. */
  tool_diag_category?: string | null;
  /** The exact finding (`call_left_in_content`, `repeated_call`...). */
  tool_diag_code?: string | null;
  /** Evidence as JSON text. */
  tool_diag_detail?: string | null;
}

export interface RequestQuery {
  model?: string | null;
  client?: string | null;
  endpoint?: string | null;
  /** ok | error | oom | cancelled */
  status?: string | null;
  /** true: only requests with a tool-call finding; false: only tool rounds without one. */
  tool_issue?: boolean | null;
  /** Only findings of this category (runtime | context | model | client). */
  tool_category?: string | null;
  from_ms?: number | null;
  to_ms?: number | null;
  before_ms?: number | null;
  limit?: number | null;
}

export interface RequestPage {
  requests: RequestRow[];
  total: number;
  models: string[];
  clients: string[];
  endpoints: string[];
}

export interface TimelinePhase {
  kind: 'load' | 'queue' | 'prompt' | 'generation' | 'other';
  start_ms: number;
  duration_ms: number;
}

export interface StoredContent {
  captured_at_ms: number;
  expires_at_ms: number;
  request: string;
  response: string;
  request_truncated: boolean;
  response_truncated: boolean;
}

export interface RequestDetail {
  request: RequestRow;
  spec_accept_rate?: number | null;
  timeline: TimelinePhase[];
  content?: StoredContent | null;
}

export interface AnalyticsStorage {
  raw_rows: number;
  rollup_rows: number;
  content_rows: number;
  oldest_raw_ms?: number | null;
  oldest_rollup_day_ms?: number | null;
  file_bytes: number;
  path: string;
}

export interface AnalyticsInfo {
  settings: AnalyticsSettings;
  storage?: AnalyticsStorage | null;
  recorder: RecorderStatus;
}

/** `settings.gateway` (see harness_core::gateway). */
/** What the gateway fulfils itself for one model (`GatewaySettings.server_tools`). */
export interface GatewayServerTools {
  /** Fulfil the hosted `web_search` tool clients send with Harness's own `web_search` tool. */
  web_search: boolean;
  /** Harness tools offered to the model on every request and run by the gateway. */
  tools: string[];
}

export interface GatewaySettings {
  enabled: boolean;
  host: string;
  port: number;
  allow_network: boolean;
  require_token: boolean;
  cors_origins: string[];
  /** 0 = no limit. */
  max_loaded_models: number;
  max_queue: number;
  /** 0 = no limit. */
  request_timeout_secs: number;
  default_ttl_secs: number | null;
  memory_aware: boolean;
  memory_headroom_mib: number;
  /** Definition id -> tool types / names the user allowed to ignore. */
  tool_ignores: Record<string, string[]>;
  /** Definition id -> server-side tools: the gateway runs the tool loop itself; off by default. */
  server_tools: Record<string, GatewayServerTools>;
  /** Per-definition text tool-call recovery; absent entries are off. */
  text_tool_calls: Record<string, boolean>;
  /** Also listen on the Ollama port (default 11434); off by default. */
  ollama_port_enabled: boolean;
  ollama_port: number;
  /** `/api/pull` may download Hugging Face references; off by default. */
  ollama_allow_pull: boolean;
  /** `/v1/systemone` and `/v1/decisions` answer with chat models too (grammar + logprobs); on by default. */
  decision_emulation: boolean;
  /** Stateful `/v1/responses` (`store`, `previous_response_id`). */
  responses: ResponsesStoreSettings;
}

/** `settings.gateway.responses` (see harness_core::gateway::responses). */
export interface ResponsesCompactionSettings {
  /** Compact every chained request over the threshold, not only the ones that ask. */
  auto: boolean;
  /** Share of the context window the conversation may fill (10-95). */
  threshold_percent: number;
  /** Newest links kept verbatim (1-50). */
  keep_links: number;
}

export interface ResponsesStoreSettings {
  /** Store responses (requests can still opt out with `store: false`). On by default. */
  store: boolean;
  /** Days kept after creation or the last continuation. */
  retention_days: number;
  /** Most stored responses kept. */
  max_stored: number;
  /** Most space used, MiB. */
  max_mib: number;
  /** Summarizing the older links of long chains (`context_management`). */
  compaction: ResponsesCompactionSettings;
}

/** Size of the stored responses in state.db. */
export interface StoredResponsesStats {
  count: number;
  bytes: number;
}

export type EndpointKind = 'managed' | 'direct';

export interface ClientEndpoint {
  kind: EndpointKind;
  host_url: string;
  base_url: string;
  requires_token: boolean;
}

export interface GatewayStatus {
  running: boolean;
  host: string | null;
  port: number | null;
  endpoint: ClientEndpoint | null;
  require_token: boolean;
  error: string | null;
  started_at: number | null;
  /** The Ollama port the gateway also listens on, when bound. */
  ollama_port?: number | null;
  /** Why the Ollama port is not served (backend key + `: <port>`). */
  ollama_warning?: string | null;
  /** The configured port was held by another program: the gateway runs on `port` instead. */
  port_fallback?: PortFallback | null;
  /** Client configs Harness moved to this port at the start (display names). */
  clients_moved?: string[];
  /** Client configs still on another address after that: their card shows "Düzelt". */
  clients_pending?: string[];
}

/** `harness_core::ports::PortHolder`: the process listening on a port. */
export interface PortHolder {
  pid: number;
  /** Executable name; empty when it could not be read. */
  name: string;
}

/** `harness_core::ports::PortFallback`: a listener that could not use its configured port. */
export interface PortFallback {
  configured: number;
  holder: PortHolder | null;
}

export type SlotState = 'unloaded' | 'loading' | 'ready' | 'draining';

/** One definition as the gateway scheduler sees it. */
export interface GatewayModelSlot {
  definition_id: string;
  alias: string;
  state: SlotState;
  server_id: string | null;
  port: number | null;
  /** Loaded by the gateway (limits, eviction, TTL apply). */
  managed: boolean;
  pinned: boolean;
  ttl_secs: number | null;
  /** Ollama `keep_alive` of the last Ollama request, while it overrides the TTL. */
  keep_alive?: 'forever' | { secs: number } | null;
  in_flight: number;
  queued: number;
  concurrency: number;
  /** Requests that found every slot busy since this load (hint: more concurrent requests). */
  waited?: number;
  /** Requests that had a slot but waited for room in the shared KV pool since this load. */
  waited_tokens?: number;
  /** Token budget of the shared KV pool (0 = none) and what running requests hold of it. */
  token_pool?: number;
  tokens_in_use?: number;
  /** This load was fitted into the free memory with other cache types / context than the definition's. */
  adapted?: { cache_type_k: string; cache_type_v: string; context_size: number; requested_context: number; gpu_layers?: number } | null;
  idle_secs: number | null;
}

/** `get_gateway_status` / `set_gateway_settings`. */
export interface GatewayOverview {
  settings: GatewaySettings;
  status: GatewayStatus;
  models: GatewayModelSlot[];
  /** Token of Harness's own clients (only while running). */
  client_token: string | null;
  owner: boolean;
  inference_tokens: number;
  /** Stored Responses (`null` when state.db is unavailable). */
  responses_stats?: StoredResponsesStats | null;
  /** Queue history of the last week per definition (slot recommendation). */
  queue_hints?: QueueHint[];
}

/** One definition's queue history (`harness_core::analytics::QueueHint`). */
export interface QueueHint {
  definition_id: string;
  alias: string;
  slots: number;
  requests: number;
  waited_slots: number;
  waited_tokens: number;
  wait_share: number;
  avg_wait_ms: number;
  suggested_slots: number | null;
}

/** `settings.control_api` (see harness_core::control_api). */
export interface ControlApiSettings {
  enabled: boolean;
  host: string;
  port: number;
  allow_network: boolean;
  cors_origins: string[];
}

export type ApiScope =
  | 'read'
  | 'inference'
  | 'models:write'
  | 'downloads:write'
  | 'servers:write'
  | 'servers:control'
  | 'conversations'
  | 'tools'
  | 'admin';

/** Gateway route groups a token can be limited to (harness_core::token_policy). */
export type GatewayEndpoint = 'chat' | 'embeddings' | 'rerank' | 'audio' | 'images' | 'videos' | 'models' | 'messages' | 'ollama' | 'metrics' | 'pipelines';

/** A token's rate limits (`null` = unlimited). */
export interface RateLimits {
  requests_per_minute: number | null;
  tokens_per_minute: number | null;
  max_concurrent: number | null;
}

/** A token's gateway policy: empty lists = no restriction. */
export interface TokenPolicy {
  /** Model alias patterns (`*`, `?`). */
  models: string[];
  endpoints: GatewayEndpoint[];
  limits: RateLimits;
}

export interface ApiTokenInfo extends TokenPolicy {
  id: string;
  name: string;
  scopes: ApiScope[];
  /** Unix seconds. */
  created_at: number;
}

/** A change to a token (omitted = keep). */
export interface ApiTokenUpdate {
  name?: string;
  scopes?: ApiScope[];
  models?: string[];
  endpoints?: GatewayEndpoint[];
  limits?: RateLimits;
}

export type OtelProtocol = 'http_protobuf' | 'grpc';

/** settings.json → otel (header values live in the keychain; only their names are here). */
export interface OtelSettings {
  enabled: boolean;
  endpoint: string;
  protocol: OtelProtocol;
  service_name: string;
  traces: boolean;
  logs: boolean;
  metrics: boolean;
  metrics_interval_secs: number;
  include_content: boolean;
  header_names: string[];
}

export interface OtelStatus {
  built: boolean;
  /** This build can export over OTLP/gRPC (cargo feature `otel-grpc`; not in default builds). */
  grpc: boolean;
  active: boolean;
  traces_url: string | null;
  metrics_url: string | null;
  logs_url: string | null;
  spans_exported: number;
  spans_dropped: number;
  logs_exported: number;
  logs_dropped: number;
  export_errors: number;
  last_error: string | null;
  last_export_ms: number | null;
}

export interface OtelOverview {
  settings: OtelSettings;
  status: OtelStatus;
}

export interface OtelTestResult {
  ok: boolean;
  url: string;
  protocol: OtelProtocol;
  elapsed_ms: number;
  trace_id: string;
  error: string | null;
}

export interface CreatedApiToken {
  info: ApiTokenInfo;
  /** Shown once. */
  token: string;
}

export interface InstanceInfo {
  pid: number;
  started_at: number;
  kind: 'gui' | 'headless';
  version: string;
  api: { host: string; port: number } | null;
  token_path: string | null;
}

/** `get_control_api_status` / `set_control_api_settings`. */
export interface ControlApiOverview {
  settings: ControlApiSettings;
  status: {
    running: boolean;
    base_url: string | null;
    host: string | null;
    port: number | null;
    token_path: string | null;
    error: string | null;
    /** The configured port was held by another program: the API runs on `port` instead. */
    port_fallback?: PortFallback | null;
  };
  owner: boolean;
  other_owner: InstanceInfo | null;
  owner_error: string | null;
  openapi_url: string | null;
  named_tokens: number;
}

/** `settings.network` (see harness_core::network). */
export interface NetworkSettings {
  /** `--cors-origins`; empty = loopback: llama-server default, LAN: no browser origin. */
  cors_origins: string[];
  /** Server id -> the user's "open anyway" for a network bind without an API key. */
  exposure_acks: Record<string, ExposureAck>;
}

export interface ExposureAck {
  /** Unix seconds. */
  acknowledged_at: number;
  host: string;
}

export type BindScope = 'loopback' | 'lan';

export interface LanAddress {
  ip: string;
  interface: string;
}

/** `get_network_overview`. */
export interface NetworkOverview {
  default_host: string;
  default_scope: BindScope;
  lan_addresses: LanAddress[];
  /** Whether the runtime has `--cors-origins` (null = unknown). */
  cors_supported: boolean | null;
  settings: NetworkSettings;
}

/** Closing the main window: ask, hide to the tray, or quit. */
export type CloseAction = 'ask' | 'tray' | 'quit';

/** Background / tray behaviour. Launch-at-login itself lives in the OS (see get/set_launch_at_login). */
export interface StartupSettings {
  /** Legacy switch, kept in step with `close_action` by the backend (older builds read it). */
  close_to_tray: boolean;
  /** What closing the window does; absent in older files (then: tray if `close_to_tray`, else ask). */
  close_action?: CloseAction | null;
  /** When launched at login, stay in the tray instead of opening the window. */
  start_minimized: boolean;
  /** Relaunch the Dashboard's last server configuration on app start. */
  restore_last_session: boolean;
  /**
   * Keep the window's page loaded while hidden to the tray. Off (default): it is closed after
   * a while in the tray and rebuilt on "show" (the tray then costs only the app process).
   */
  keep_window_in_tray?: boolean;
}

export type RuntimeAutoUpdatePhase =
  | 'idle'
  | 'checking'
  | 'downloading'
  | 'pending_activation'
  | 'updated'
  | 'skipped_unverified'
  | 'kept_current'
  | 'error';

/** `get_runtime_auto_update_status` / `run_runtime_auto_update`. */
export interface RuntimeAutoUpdateStatus {
  phase: RuntimeAutoUpdatePhase;
  /** Unix seconds. */
  last_check: number | null;
  latest_version: string | null;
  message: string | null;
}

/** `check_app_update` (null = up to date). */
export interface AppUpdateInfo {
  current_version: string;
  version: string;
  notes: string | null;
  date: string | null;
  /** Installed by a package (Linux .deb): updated from the releases page, not in place. */
  manual: boolean;
}

/** `get_app_update_progress`. */
export interface AppUpdateProgress {
  downloaded: number;
  total: number | null;
  finished: boolean;
}

export type ServerStatus =
  | 'Stopped'
  | 'Starting'
  | 'Running'
  | 'Stopping'
  | { Error: string };

export interface LaunchConfig {
  /** Settings holding the automatic choice, not a value anyone set (never saved as explicit). */
  auto_fields?: string[];
  /** Resolved by the backend from the runtime settings; ignored if sent. */
  binary_path?: string;
  model_path: string;
  gpu_layers: number;
  context_size: number;
  host: string;
  /** With `port_auto` (or 0) only the last automatic pick. */
  port: number;
  /** Harness picks a free port (11440–11499) at every start; a taken one is replaced silently. */
  port_auto?: boolean;
  threads: number | null;
  flash_attn?: boolean;
  cache_type_k?: string | null;
  cache_type_v?: string | null;
  parallel_slots?: number | null;
  logical_batch?: number | null;
  physical_batch?: number | null;
  context_shift?: boolean | null;
  split_mode?: string | null;
  main_gpu?: number | null;
  tensor_split?: string | null;
  rope_scaling?: string | null;
  rope_freq_base?: number | null;
  rope_freq_scale?: number | null;
  chat_template?: string | null;
  /** Write-only: a newly typed key ('' clears the saved one, null keeps it). Never returned by the backend. */
  api_key?: string | null;
  /** Reference to the key saved in the OS keychain (the key itself is never sent to the UI). */
  api_key_ref?: string | null;
  custom_args?: string | null;
  draft_model_path?: string | null;
  draft_gpu_layers?: number | null;
  lora_path?: string | null;
  lora_scale?: number | null;
  mmproj_path?: string | null;
  /** Passes `--mcp-servers-config` with the MCP servers enabled in Connections. */
  mcp_enabled?: boolean;
  router_mode?: boolean;
  models_path?: string | null;
  /** Catalog parameters (emitted only when the runtime has the flag). */
  params?: ParamValues;
  /** Served model id (`--alias`): the definition's alias (set by the backend at launch). */
  alias?: string | null;
  /** ModelDefinition this form / launch belongs to (the backend layers the form over it). */
  definition_id?: string | null;
  /** Runtime registry id (null / 'official' = Settings > Runtime). */
  runtime?: string | null;
  /** Set by the backend: 'gateway' for the gateway's loads, 'agents' for servers prepared for an agent tool. */
  launched_by?: SlotOrigin;
}

export interface LocalModel {
  /** Definition file absent from the configured library scan. */
  outside_scan?: boolean;
  id: string;
  name: string;
  filename: string;
  path: string;
  size_bytes: number;
  architecture: string | null;
  context_length: number | null;
  quantization: string;
  is_split: boolean;
  total_shards: number | null;
  present_shards: number | null;
  is_ready: boolean;
  shard_paths: string[];
  root_dir: string;
  gguf?: GgufMetadata | null;
  /** What the file is, from its GGUF header ("draft" is a usage, not a kind). */
  kind?: ModelKind;
  /** In a library directory, or linked in place from Ollama / LM Studio. */
  origin?: 'library' | 'linked';
  /** Hugging Face pipeline task of a `pipeline` model ("text-classification", "fill-mask", ...). */
  task?: string;
}

export type ModelKind =
  | 'llm'
  | 'embedding'
  | 'reranker'
  | 'asr'
  | 'tts'
  | 'image'
  | 'pipeline'
  | 'mmproj'
  | 'adapter'
  | 'unknown';

export interface GgufMetadata {
  /** Recommendations returned by get_model_metadata. */
  known_metadata_fixes?: { sliding_window: number | null; override_kv: string | null };
  version: number;
  tensor_count: number;
  kv_count: number;
    architecture: string | null;
    audiocpp_family?: string | null;
  model_name: string | null;
  context_length: number | null;
  file_type: number | null;
  quantization: string;
  block_count: number | null;
  embedding_length: number | null;
  head_count: number | null;
  head_count_kv: number | null;
  key_length: number | null;
  value_length: number | null;
  expert_count: number | null;
  expert_used_count: number | null;
  has_chat_template: boolean;
  template_supports_tools: boolean;
  pooling_type: number | null;
  is_mmproj: boolean;
  general_type?: string | null;
  adapter_type?: string | null;
  has_vision_encoder?: boolean;
  has_audio_encoder?: boolean;
  /** The audio projector's family (`lfm2a`, `qwen3asr`, `ultravox`...) when it has an audio encoder. */
  audio_projector?: string | null;
  /** `stt.variant` of a transcribe.cpp speech recognition export. */
  stt_variant?: string | null;
  /** `stt.capability.translate` of such an export. */
  stt_translate?: boolean;
  has_classifier_labels?: boolean;
  /** `<arch>.decision.type`: a native decision model (`/v1/systemone`, `/v1/decisions`). */
  decision_type?: string;
  /** `general.size_label` ("4B", "32x2.4B"; display only). */
  size_label?: string | null;
  basename?: string | null;
  /** Exact parameter count of this file's tensors (split models: this part only). */
  param_count?: number | null;
  template_supports_reasoning?: boolean;
  reasoning_control?: { effort: boolean; toggle: boolean; budget: boolean };
  tokenizer?: TokenizerInfo | null;
}

export interface TokenizerInfo {
  model: string | null;
  pre: string | null;
  vocab_size: number | null;
  bos_id: number | null;
  eos_id: number | null;
  add_bos: boolean | null;
  add_eos: boolean | null;
  head_hash: string | null;
}

export interface StorageDirInfo {
  path: string;
  is_primary: boolean;
  exists: boolean;
  free_bytes: number;
  total_bytes: number;
  model_count: number;
}

export interface ModelPreset {
  gpu_layers: number;
  context_size: number;
  threads: number | null;
  flash_attn?: boolean;
  cache_type_k?: string | null;
  cache_type_v?: string | null;
  parallel_slots?: number | null;
  logical_batch?: number | null;
  physical_batch?: number | null;
  context_shift?: boolean | null;
  split_mode?: string | null;
  main_gpu?: number | null;
  tensor_split?: string | null;
  rope_scaling?: string | null;
  rope_freq_base?: number | null;
  rope_freq_scale?: number | null;
  chat_template?: string | null;
  /** Write-only: a newly typed key ('' clears the saved one, null keeps it). Never returned by the backend. */
  api_key?: string | null;
  /** Reference to the key saved in the OS keychain (the key itself is never sent to the UI). */
  api_key_ref?: string | null;
  custom_args?: string | null;
  draft_model_path?: string | null;
  draft_gpu_layers?: number | null;
  lora_path?: string | null;
  lora_scale?: number | null;
  mmproj_path?: string | null;
  /** Passes `--mcp-servers-config` with the MCP servers enabled in Connections. */
  mcp_enabled?: boolean;
  /** Catalog parameters (emitted only when the runtime has the flag). */
  params?: ParamValues;
  /** Runtime registry id (null = the official runtime). */
  runtime?: string | null;
}

/** A saved run log of one server (`list_server_log_files`). */
export interface LogFileInfo {
  name: string;
  size_bytes: number;
  /** Unix seconds. */
  modified: number;
}

/** One `search_server_logs` hit (1-based line number). */
export interface LogMatch {
  line: number;
  text: string;
}

/** Whether an archive has a published SHA-256 digest to check the download against. */
export type AssetIntegrity = 'verified' | 'missing' | 'invalid';

export interface ReleaseAsset {
  name: string;
  download_url: string;
  size: number;
  digest?: string | null;
  integrity?: AssetIntegrity;
}

export interface ReleaseAssetInfo {
  tag_name: string;
  asset_name: string;
  download_url: string;
  size: number;
  digest?: string | null;
  extra_assets: ReleaseAsset[];
  integrity?: AssetIntegrity;
  /** Backend of the chosen archive (may differ from the requested one after a fallback). */
  backend?: string | null;
}

export interface RuntimeDownloadProgress {
  current_asset: string;
  downloaded_bytes: number;
  total_bytes: number;
}

export interface CustomBinaryValidation {
  is_valid: boolean;
  version: string | null;
  supports_metrics: boolean;
  error: string | null;
}

/** `harness_core::hub_tasks::TaskSource`. */
export type HfTaskSource = 'tags' | 'name' | 'files' | 'library' | 'gguf';

/** `harness_core::hub_tasks::NotRunnable`. */
export type HfNotRunnable =
  | 'foreign_speech_gguf'
  | 'needs_sea_g2p'
  | 'needs_librosa'
  | 'vibevoice_layout'
  | 'audio_cpp_incomplete_package'
  | 'sdxl_refiner_folder_required'
  | 'unsupported_weight_layout'
  | 'no_gguf'
  | 'adapter'
  | 'not_language_model'
  | 'audio_cpp_unsupported'
  | 'crispasr_pocket_english_only'
  | 'audio_cpp_espeak_missing'
  | 'needs_own_package'
  | 'unsupported_architecture'
  | 'safetensors_only'
  | 'ctranslate2_export'
  | 'apple_only'
  | 'pipeline_config_only'
  | 'component_only';

/** `harness_core::model_registry::Capability`. */
export type ModelCapability = 'chat' | 'vision' | 'audio' | 'embedding' | 'rerank' | 'asr' | 'tts' | 'image_generation' | 'video';

/** `harness_core::model_registry::ModelRecord`: capability, format, runtime (a local file or a Hub repo). */
export interface ModelRecord {
  capabilities: ModelCapability[];
  format: HfModelFormat;
  /** Runtime family that runs the format; null when no Harness runtime does. */
  runtime: string | null;
  /** Null for a file served only with another (projector, adapter). */
  compatibility: HfCompatibility | null;
  /** A safetensors chat folder the Python engine runs with transformers when mistral.rs is not registered. */
  python_chat?: boolean;
}

/** `harness_core::hub_compat::ModelFormat`. */
export type HfModelFormat = 'gguf' | 'whisper_ggml' | 'safetensors' | 'onnx' | 'diffusers' | 'pytorch' | 'mlx' | 'ctranslate2' | 'core_ml' | 'unknown';

/** `harness_core::hub_compat::Compatibility`: what Harness can do with a repo. */
export type HfCompatibility =
  | { status: 'runnable'; runtime: string }
  | { status: 'needs_runtime'; runtime: string }
  /** The family has no build for this OS / architecture (ONNX Runtime on an Intel Mac). */
  | { status: 'unsupported_platform'; runtime: string; os: string; arch: string }
  | { status: 'unsupported'; format: HfModelFormat };

export interface HfModelSearchResult {
  id: string;
  author: string | null;
  downloads: number;
  likes: number;
  tags?: string[];
  pipeline_tag?: string | null;
  /** Task guessed from tags / files when `pipeline_tag` is empty (never set together with it). */
  inferred_task?: string | null;
  /** What `inferred_task` was read from. */
  inferred_source?: HfTaskSource | null;
  /** llama-server cannot run the repo at all (no GGUF file, adapter, image model). */
  not_runnable?: HfNotRunnable | null;
  /** A LoRA adapter repo (loads on top of a base model). */
  adapter?: boolean;
  /** The adapter's base model. */
  adapter_base?: string | null;
  /** Model file format read from the listing. */
  format: HfModelFormat;
  compatibility: HfCompatibility;
  /** What the repo can do, from its task and format (`model_registry::hub_capabilities`). */
  capabilities?: ModelCapability[];
  last_modified?: string | null;
  gated?: string | null;
  /** From the `license:<id>` tag. */
  license?: string | null;
  quants?: string[];
  /** Repo creation time (ISO 8601). */
  created_at?: string | null;
  /** A Python-engine diffusers pipeline whose download holds PyTorch `.bin` weights (loaded as weights only). */
  pickle_weights?: boolean;
  /** A safetensors chat repo the Python engine runs with transformers when mistral.rs is not registered. */
  python_chat?: boolean;
}

export type HfSortKey = 'downloads' | 'likes' | 'trendingScore' | 'lastModified' | 'createdAt';

/** `search_hf_models({ options })`. Empty/null fields are not sent to Hugging Face. */
export interface HfSearchOptions {
  query: string;
  pipeline_tag?: string | null;
  /** With a task filter also list untagged repos whose task is inferred (default true). */
  include_untagged?: boolean | null;
  /** Also list repos that are not GGUF (default: GGUF only). */
  include_non_gguf?: boolean | null;
  license?: string | null;
  author?: string | null;
  sort?: HfSortKey | null;
  limit?: number | null;
  /** `HfSearchPage.next_cursor` of the previous page. */
  cursor?: string | null;
}

/** `search_hf_models_page({ options })`: an empty query lists by filters + sort alone. */
export interface HfSearchPage {
  items: HfModelSearchResult[];
  /** Cursor of the next page; null on the last one. */
  next_cursor: string | null;
}

/** `get_hf_model_card`. */
export interface HfModelCard {
  repo_id: string;
  /** Markdown, YAML front matter stripped, capped at 200 KB. */
  readme: string | null;
  license: string | null;
  /** "auto" | "manual" when gated. */
  gated: string | null;
  base_model: string[];
  tags: string[];
  pipeline_tag: string | null;
  /** Guessed task when the repo has no `pipeline_tag`. */
  inferred_task: string | null;
  inferred_source: HfTaskSource | null;
  not_runnable: HfNotRunnable | null;
  adapter_base?: string | null;
  downloads: number;
  likes: number;
  last_modified: string | null;
}

/** `check_hf_access`. */
export interface HfAccess {
  gated: string | null;
  token_present: boolean;
  /** null when it couldn't be determined. */
  has_access: boolean | null;
  /** License id from the model card / tags. */
  license?: string | null;
  /** Backend error key explaining missing access (`hub.gated_no_token`, `hub.gated_no_access`, ...). */
  reason?: string | null;
}

/** Signed-in Hugging Face account (`hf_whoami`, cached in `hf_auth_status`). */
export interface HfUser {
  name: string;
  fullname: string | null;
  avatar_url: string | null;
}

/** `hf_auth_status`: never carries a token. */
export interface HfAuthStatus {
  kind: 'none' | 'token' | 'oauth';
  user: HfUser | null;
  /** Unix seconds (OAuth access token expiry). */
  expires_at: number | null;
  scope: string | null;
  /** A client id is configured, so "Sign in with Hugging Face" can run. */
  oauth_available: boolean;
  client_id_source: 'settings' | 'env' | 'build' | null;
  client_id: string | null;
  registered_redirect_uri: string;
  scopes: string;
  login_pending: boolean;
}

export interface ModelArtifactUpdate {
  repo_id: string;
  filename: string;
  has_update: boolean;
  remote_size: number;
  local_size: number;
  remote_oid: string | null;
  message: string;
}

export interface HfShardInfo {
  filename: string;
  download_url: string;
  size_bytes: number;
  shard_index: number;
  total_shards: number;
  sha256?: string | null;
}

export interface HfModelVariant {
  name: string;
  not_runnable?: HfNotRunnable | null;
  quantization: string;
  total_size_bytes: number;
  is_split: boolean;
  shards: HfShardInfo[];
}

export type DownloadStatus =
  | 'Idle'
  | 'Queued'
  | 'Downloading'
  | 'Verifying'
  | 'Paused'
  | 'Completed'
  | { Error: string };

/** One download queue job (`list_downloads`, queue order). */
export interface DownloadProgress {
  /** Queue job id. */
  id: number;
  repo_id: string;
  variant_name: string;
  total_bytes: number;
  downloaded_bytes: number;
  current_shard_name: string;
  current_shard_index: number;
  total_shards: number;
  speed_bytes_per_sec: number;
  status: DownloadStatus;
  /** Unix seconds. */
  created_at: number;
}

export interface TelemetrySnapshot {
  server_online: boolean;
  predicted_tokens_per_sec: number;
  prompt_tokens_per_sec: number;
  avg_predicted_tokens_per_sec: number;
  avg_prompt_tokens_per_sec: number;
  kv_cache_usage_ratio: number;
  kv_cache_available: boolean;
  requests_processing: number;
  requests_deferred: number;
  spec_acceptance_rate: number | null;
  active_slots: number;
  total_slots: number;
  system_ram_used_bytes: number;
  system_ram_total_bytes: number;
  ram_breakdown: MemoryBreakdown | null;
  vram_used_bytes: number;
  vram_total_bytes: number;
  /** Used VRAM by consumer; null where the OS has no per-process GPU counter (the bar stays unsplit). */
  vram_breakdown: MemoryBreakdown | null;
  /** Every graphics adapter with its own VRAM and split (the first is the one `vram_*` above describe); empty with a single adapter. */
  gpus: GpuMemory[];
  context_size: number | null;
  context_used_tokens: number | null;
  slots: SlotInfo[];
  totals: ServerTotals | null;
  last_request: LastRequest | null;
}

/** VRAM of one graphics adapter. */
export interface GpuMemory {
  index: number;
  name: string;
  used_bytes: number;
  total_bytes: number;
  /** Used VRAM of this adapter by consumer; null where the OS has no per-process counter. */
  breakdown: MemoryBreakdown | null;
}

/** Used memory (RAM or VRAM) split into Harness Studio, each llama-server and the rest. */
export interface MemoryBreakdown {
  harness_bytes: number;
  servers: ServerRam[];
  other_bytes: number;
}

export interface ServerRam {
  id: string;
  name: string;
  bytes: number;
}

export interface SlotInfo {
  id: number;
  busy: boolean;
  prompt_tokens: number;
  decoded_tokens: number;
}

export interface ServerTotals {
  prompt_tokens: number;
  cached_prompt_tokens: number | null;
  generated_tokens: number;
  prompt_seconds: number;
  generation_seconds: number;
  max_sequence_tokens: number | null;
  requests_observed: number;
}

export interface LastRequest {
  finished_at: number;
  prompt_tokens: number;
  cached_tokens: number;
  generated_tokens: number;
  prompt_seconds: number;
  generation_seconds: number;
}

export interface ProcessStats {
  rss_bytes: number;
  cpu_percent: number | null;
  uptime_secs: number;
  process_count: number;
}

export interface PerfDetails {
  system_cpu_percent: number | null;
  cpu_cores: number;
  app: ProcessStats;
  app_webview: ProcessStats;
  server: ProcessStats | null;
  gpu: { utilization_percent: number | null; temperature_c: number | null } | null;
}

export interface DiagnosticEvent {
  event_type: string;
  message: string;
  stack_trace: string | null;
  metadata: any | null;
}

export interface DiagnosticPayload {
  id: string;
  version: string;
  os: string;
  engine: string;
  log_lines: string[];
  event_type: string;
  sanitized_message: string;
  sanitized_stack_trace: string | null;
  timestamp: number;
  hardware: any | null;
  metadata: any | null;
}

export interface TelemetryHistoryPoint {
  timestamp: number;
  predicted_tokens_per_sec: number;
  prompt_tokens_per_sec: number;
  vram_used_bytes: number;
  server_vram_used_bytes?: number | null;
  vram_total_bytes: number;
  ram_used_bytes: number;
  server_ram_used_bytes?: number | null;
  ram_total_bytes: number;
  active_slots: number;
  total_slots: number;
  kv_usage_ratio: number | null;
  requests_processing: number;
  requests_deferred: number;
}

export type FitCategory = 'full_gpu' | 'tight' | 'hybrid' | 'cpu_heavy' | 'oom_risk';

/** Input of the Rust memory estimate (`harness_core::vram::FitInput`). */
export interface FitInput {
  /** Launch path of a local model (GGUF header and measured loads are looked up). */
  model_path?: string | null;
  model_size_bytes?: number;
  context_size: number;
  gpu_layers: number;
  cache_type_k?: string | null;
  cache_type_v?: string | null;
  /** `-np`; null = automatic (`harness_core::slots`). */
  parallel_slots?: number | null;
  /** Who starts the server (decides the automatic slot count); default 'user'. */
  origin?: SlotOrigin;
  /** `--ctx-checkpoints`; null = Harness's default (4). */
  ctx_checkpoints?: number | null;
  kv_unified?: boolean | null;
  flash_attn?: boolean | null;
  ubatch?: number | null;
  swa_full?: boolean;
  kv_offload?: boolean | null;
  mmproj_path?: string | null;
  mmproj_size_bytes?: number;
  /** Automatic KV precision (`harness_core::kv_policy`): the types the launch would choose. */
  kv_auto?: boolean;
}

/** Conversation-memory precision level (`harness_core::kv_policy::KvLevel`). */
export type KvLevel = 'full' | 'keys_only' | 'balanced' | 'saver' | 'compact' | 'custom';

/** An automatic KV precision choice (`harness_core::kv_policy::KvChoice`). */
export interface KvChoice {
  level: KvLevel;
  cache_type_k: string;
  cache_type_v: string;
  reason: 'fits' | 'to_fit' | 'tightest' | 'too_big';
  kv_bytes_f16: number;
  kv_bytes: number;
  /** Prompt-processing slowdown vs f16 measured for this backend (percent); null = unknown. */
  prompt_slowdown_pct: number | null;
}

export interface MemoryEstimate {
  vram_bytes: number;
  ram_bytes: number;
  model_gpu_bytes: number;
  model_host_bytes: number;
  kv_bytes: number;
  kv_gpu_bytes: number;
  compute_gpu_bytes: number;
  compute_host_bytes: number;
  output_host_bytes?: number;
  mmproj_bytes: number;
  overhead_bytes: number;
  offloaded_layers: number;
  total_layers: number;
  kv_cells: number;
  swa_layers: number;
  swa_cells: number;
  /** Recurrent state of all slots (part of kv_bytes). */
  recurrent_bytes?: number;
  /** Context checkpoints at most, host memory (part of ram_bytes). */
  checkpoint_bytes?: number;
  /** Slots the estimate assumes. */
  slots?: number;
  exact: boolean;
  /** Draft model (speculative decoding) in device memory, part of vram_bytes. */
  draft_gpu_bytes?: number;
  /** Draft model in host memory, part of ram_bytes. */
  draft_host_bytes?: number;
}

export type SlotOrigin = 'user' | 'agents' | 'gateway';

/** Concurrent requests (llama-server slots) of a launch (`harness_core::slots::SlotAdvice`). */
export interface SlotAdvice {
  slots: number;
  /** Chosen by the automatic rule. */
  auto: boolean;
  recommended: number;
  /** What the rule picks when the model serves agents (gateway loads). */
  recommended_agents: number;
  /** i18n key (`slots.reason.*`). */
  reason: string;
  /** Memory one more slot adds (device + host). */
  per_slot_bytes: number;
  /** Context each of the recommended concurrent requests can count on. */
  ctx_per_slot: number;
}

/** `estimate` = GGUF only; `corrected` = scaled by a measured load; `measured` = same settings measured. */
export type EstimateSource = 'estimate' | 'corrected' | 'measured';

export interface FitResult {
  category: FitCategory;
  estimate: MemoryEstimate;
  /** `shared_igpu`: integrated GPU; `vram_bytes` is its share of system RAM (same pool). */
  budget: {
    vram_bytes: number;
    ram_bytes: number;
    unified: boolean;
    shared_igpu?: boolean;
    igpu_offload_faster?: boolean | null;
  };
  vram_percentage: number;
  ram_percentage: number;
  source: EstimateSource;
  measured_at: number | null;
  recommended_gpu_layers: number;
  slots?: SlotAdvice | null;
  /** The automatic KV precision the estimate used (`kv_auto` inputs only). */
  kv: KvChoice | null;
}

export interface HardwareFitResult {
  category: FitCategory;
  label: string;
  badgeClass: string;
  emoji: string;
  description: string;
  requiredVramBytes: number;
  requiredRamBytes: number;
  vramPercentage: number;
  ramPercentage: number;
  kvCacheBytes: number;
  offloadedLayers: number;
  totalLayers: number;
  /** False only when the numbers are a measured load with the same settings. */
  isEstimate: boolean;
  /** Layer count / KV shape came from the GGUF header (more accurate). */
  exact: boolean;
  source: EstimateSource;
  /** Largest -ngl under 90% of the VRAM budget (layer count + 1 = full offload). */
  recommendedGpuLayers: number;
  swaLayers: number;
  /** Integrated GPU sharing system RAM (no dedicated VRAM). */
  sharedIgpu: boolean;
  /** Dedicated GPU memory the estimate was compared with (0 = none). */
  vramBudgetBytes: number;
  /** Unified memory (Apple Silicon): the GPU shares system RAM. */
  unifiedMemory: boolean;
  /** Concurrent requests: the automatic choice or the set value, with the recommendation. */
  slots: SlotAdvice | null;
  /** Automatic KV precision choice (null for explicit cache types). */
  kv: KvChoice | null;
  /** Draft model's share of requiredVramBytes / requiredRamBytes (0 without a draft). */
  draftGpuBytes: number;
  draftHostBytes: number;
}


export interface ActiveServerInfo {
  connect_host: string;
  bind_host: string;
  port: number;
  /** Whether the server was started with an API key (fetch it on demand with `get_server_api_key`). */
  api_key_set: boolean;
  /** Requests need a key: the user's or one made for this run (Python workers always have one). */
  api_key_required?: boolean;
  model_path: string;
  router_mode: boolean;
  /** Bound to the network without an API key after the user's "open anyway". */
  exposure_acknowledged?: boolean;
  /** `--cors-origins` of this run (null = llama-server's default). */
  cors_origins?: string | null;
  /** What this run serves (`serve_mode::ServeMode`): chat, `--embeddings` or `--rerank`. */
  serve_mode?: ServeMode;
  /** Runtime family serving this run (`llama.cpp`, `mlx`, `mlx-vlm`, `mlx-audio`, ...); empty for older records. */
  engine_family?: string;
}

/** Serving mode of a llama-server run; stored as the `serve_mode` launch setting (absent = automatic). */
export type ServeMode = 'chat' | 'embedding' | 'rerank' | 'transcription' | 'image' | 'pipeline';

/** One llama-server instance managed by the backend ("default" always exists). */
export interface ServerSummary {
  id: string;
  status: ServerStatus;
  /** Connection info while starting/running. */
  active: ActiveServerInfo | null;
  /** Why the last run failed (`harness_core::failure`); null while it runs. */
  failure?: ServerFailure | null;
  /** A scheduled automatic restart after a crash. */
  restart?: RestartState | null;
}

/** Load state of one model of a router server (`harness_core::router`). */
export type RouterModelState = 'loaded' | 'loading' | 'unloaded' | 'failed';

/** One model a router server (llama-server `--models-dir`) serves. */
export interface RouterModel {
  /** The name clients send as `model`. */
  id: string;
  state: RouterModelState;
  source?: string | null;
  path?: string | null;
  context_length?: number | null;
  exit_code?: number | null;
}

export type FailureKind =
  | 'out_of_memory'
  | 'missing_library'
  | 'corrupt_model'
  | 'unsupported_architecture'
  | 'model_not_found'
  | 'port_in_use'
  | 'invalid_argument'
  | 'load_timeout'
  | 'crashed'
  | 'start_failed';

/** A classified failure of a run (`servers.<kind>` code, `failure.<kind>` message key). */
export interface ServerFailure {
  code: string;
  kind: FailureKind;
  message_key: string;
  params: Record<string, string>;
  phase: 'loading' | 'running';
  exit_code: number | null;
  evidence: string[];
  at: number;
}

export interface RestartState {
  attempt: number;
  max_attempts: number;
  delay_secs: number;
  /** Unix milliseconds when the restart begins. */
  restart_at_ms: number;
}

export type RecoveryKind =
  | 'lower_gpu_layers'
  | 'lower_context'
  | 'lower_ubatch'
  | 'kv_cache_quant'
  | 'enable_fit'
  | 'cpu_moe'
  | 'unload_server';

export interface RecoveryOption {
  id: string;
  kind: RecoveryKind;
  label_key: string;
  params: Record<string, string>;
  /** Catalog ids → value for the retry (transient unless saved). */
  overrides: ParamValues;
  /** Servers stopped before the retry. */
  unload: string[];
  recommended: boolean;
  /** Caveat shown with the option (`recovery.ram_insufficient`); values in `params`. */
  warning_key?: string | null;
}

export interface RecoveryPlan {
  server_id: string;
  failure: ServerFailure;
  options: RecoveryOption[];
  free_vram_bytes: number | null;
  estimated_need_bytes: number | null;
}

export type DoctorStatus = 'ok' | 'warn' | 'fail';

export interface DoctorCheck {
  id: string;
  status: DoctorStatus;
  summary: string;
  message_key: string;
  params: Record<string, string>;
  detail: unknown;
}

/** `harness doctor` report (`harness_core::doctor`). */
export interface DoctorReport {
  ok: boolean;
  data_dir: string;
  version: string;
  checks: DoctorCheck[];
  runtime: unknown;
}

/**
 * A saved, named launch configuration; started on the instance whose id equals the profile id.
 * (GUI view of a `ServerProfileRecord` + its `ModelDefinition`, see the domain types below.)
 */
export interface ServerProfile {
  id: string;
  name: string;
  model_id: string | null;
  /** Short free-text purpose ("OpenCode", "Open Design"), shown in the lists. */
  description: string;
  config: LaunchConfig;
  /** Started automatically when Harness Studio launches. */
  auto_start: boolean;
  updated_at: number;
}

/**
 * The Dashboard's own server ("default") while it is not running (`get_default_server`).
 * Its saved launch is the one source of truth; the settings' server defaults only describe a
 * Dashboard server that was never saved.
 */
export interface DefaultServer {
  /** The saved launch, or the settings defaults (no model) when `saved` is false. */
  config: LaunchConfig;
  saved: boolean;
}

export interface ClientIntegrationStatus {
  id: string;
  name: string;
  installed: boolean;
  configured: boolean;
  config_path: string | null;
  details: string | null;
}


// ---------------------------------------------------------------------------
// Runtime capabilities and the parameter catalog (harness_core::capabilities / params)
// ---------------------------------------------------------------------------

/** A catalog parameter value (`LaunchConfig.params`). */
export type ParamValue = boolean | number | string | string[];
export type ParamValues = Record<string, ParamValue>;

/** One flag of the runtime's `--help`. */
export interface FlagInfo {
  canonical: string;
  aliases: string[];
  negations: string[];
  value_hint: string | null;
  value_optional: boolean;
  enum_values: string[];
  env: string | null;
  default: string | null;
  description: string;
  section: string | null;
  deprecated: boolean;
  removed: boolean;
}

/** `--list-devices` entry (capability cache: values of the probe time). */
export interface RuntimeDevice {
  id: string;
  backend: string;
  name: string;
  total_mib: number | null;
  free_mib: number | null;
}

/** A device of the runtime with current free memory (`get_runtime_devices`). */
export interface RuntimeDeviceLive {
  id: string;
  /** `--main-gpu` value. */
  index: number;
  backend: string;
  name: string;
  total_bytes: number | null;
  free_bytes: number | null;
  os_used_bytes: number | null;
  available_bytes: number | null;
  integrated: boolean;
}

export interface SplitSuggestion {
  main_gpu: number;
  main_device: string;
  tensor_split: string;
  split_mode: string;
  devices: string[];
}

export interface DeviceReport {
  binary: string | null;
  devices: RuntimeDeviceLive[];
  probed_at: number;
  error: string | null;
  split: SplitSuggestion | null;
}

export interface Verification {
  ok: boolean;
  detail: string | null;
  verified_at: number;
}

export interface RuntimeCapabilities {
  schema_version: number;
  key: {
    path: string;
    mtime_ms: number | null;
    size: number | null;
    build: number | null;
    commit: string | null;
    version: string | null;
  };
  version: { raw: string; version: string | null; build: number | null; commit: string | null; built_with: string | null };
  help_parsed: boolean;
  flags: Record<string, FlagInfo>;
  devices: RuntimeDevice[];
  probed_at: number;
  endpoint_verified: Record<string, Verification>;
  metric_names: string[];
}

export type CapabilityLevel =
  | 'unsupported'
  | 'unknown'
  | 'flag_detected'
  | 'endpoint_verified'
  | 'model_dependent'
  | 'platform_dependent';

export interface FeatureStatus {
  id: string;
  level: CapabilityLevel;
  /** true: usable as far as known; false: won't work here; null: can't tell yet. */
  available: boolean | null;
  reason: { key: string; params: Record<string, string> };
  flag: string | null;
}

export type ParamKind = 'bool' | 'int' | 'float' | 'enum' | 'multi_enum' | 'string' | 'path' | 'list' | 'json';

export interface ParamSpec {
  id: string;
  flags: string[];
  off_flags: string[];
  kind: ParamKind;
  values: string[];
  values_from_help: boolean;
  group: string;
  level: 'basic' | 'advanced';
  label_key: string;
  help_key: string;
  requires: string[];
  conflicts: string[];
  deprecated_by: string | null;
  /** Typed LaunchConfig field that already emits this flag (not rendered from `params`). */
  legacy_field: string | null;
  feature: string | null;
  min: number | null;
  max: number | null;
  separator: string | null;
  loopback_only: boolean;
  placeholder: string | null;
  /** Sent when the param is left unset (Harness lowers llama-server's memory-bound defaults). */
  harness_default: number | null;
}

export interface ParamCatalog {
  schema_version: number;
  param: ParamSpec[];
}

/** A `--help` flag for the "all flags" explorer. */
export interface RuntimeFlag extends FlagInfo {
  param_id: string | null;
  protected: boolean;
}

/** A LoRA adapter of a running server (`list_lora_adapters`). */
export interface LoraAdapterLive {
  id: number;
  path: string;
  /** 0 = loaded but not applied. */
  scale: number;
}

/** What a running server actually uses (`get_effective_launch_values`). */
export interface EffectiveLaunchValues {
  requested_ctx: number;
  requested_gpu_layers: number;
  requested_slots: number | null;
  n_ctx: number | null;
  total_slots: number | null;
  n_ctx_train: number | null;
  kv_unified: boolean | null;
  gpu_layers_offloaded: number | null;
  gpu_layers_total: number | null;
  /** `log` (the `offloaded N/M` line, only at `-lv 4`) or `derived` (from -ngl and the model's layer count). */
  gpu_layers_source: 'log' | 'derived' | null;
  /** `--fit on` chose the layers and the log did not say how many. */
  gpu_layers_by_fit: boolean;
  ctx_adjusted: boolean;
  model_alias: string | null;
  build_info: string | null;
  chat_format: string | null;
  reasoning_format: string | null;
  speculative_types: string | null;
  modalities: Record<string, boolean>;
  /** K/V pair (`q8_0-f16`) the CUDA build converts to f16 on every token (slow). */
  kv_fa_fallback?: string | null;
  /** llama-server could not fit the settings into free device memory and started anyway. */
  fit_failed?: boolean;
  /** Light shared-GPU-memory pressure: long prompts may be slower (heavy spill is `fit_failed`). */
  shared_memory_slowdown?: boolean;
  shared_memory_bytes?: number | null;
  observed_at: number;
}

// --- Domain model (private hs-internal architecture notes) ---

/** Base weights of a definition: a local GGUF or a Hugging Face file not downloaded yet. */
export type ModelRef = { kind: 'local'; path: string } | { kind: 'hf'; repo: string; file: string };

/** A logical model: what clients ask for by `alias`. `model_definitions.json`. */
export interface ModelDefinition {
  /** Empty when creating. */
  id: string;
  /** API `model` name: [A-Za-z0-9._:-], unique (case-insensitive). */
  alias: string;
  base: ModelRef;
  description?: string;
  mmproj?: string | null;
  adapters?: { path: string; scale?: number | null }[];
  draft?: { model: string; params?: ParamValues } | null;
  chat_template?: string | null;
  /** JSON object text for --chat-template-kwargs. */
  chat_template_kwargs?: string | null;
  extra_args?: string | null;
  /** Default load settings: catalog id -> value. */
  load: ParamValues;
  default_preset_id?: string | null;
  /** Reserved for the gateway. */
  ttl_seconds?: number | null;
  pinned?: boolean;
  /** Store request/response content of gateway requests in analytics.db (off by default). */
  capture_content?: boolean;
  /** Days captured content is kept (default 7). */
  content_retention_days?: number | null;
  /** Made by Harness Studio (migration, profile split); removed with its profile unless edited. */
  auto_created?: boolean;
  created_at: number;
  updated_at: number;
}

/** How a definition is served (`server_profiles.json`; id "default" = the Dashboard session). */
export interface ServerProfileRecord {
  id: string;
  name: string;
  purpose?: string;
  definition_id?: string | null;
  router?: { models_dir?: string | null } | null;
  host: string;
  port: number;
  auto_start: boolean;
  api_key_ref?: string | null;
  /** Layer over the definition's load; an empty string means "unset". */
  serve_overrides: ParamValues;
  created_at: number;
  updated_at: number;
}

export interface SamplingParams {
  temperature?: number;
  top_p?: number;
  top_k?: number;
  min_p?: number;
  typical_p?: number;
  top_n_sigma?: number;
  repeat_penalty?: number;
  repeat_last_n?: number;
  presence_penalty?: number;
  frequency_penalty?: number;
  dry_multiplier?: number;
  dry_base?: number;
  dry_allowed_length?: number;
  dry_penalty_last_n?: number;
  dry_sequence_breakers?: string[];
  xtc_probability?: number;
  xtc_threshold?: number;
  mirostat?: number;
  mirostat_tau?: number;
  mirostat_eta?: number;
  dynatemp_range?: number;
  dynatemp_exponent?: number;
  adaptive_target?: number;
  adaptive_decay?: number;
  logprobs?: boolean;
  top_logprobs?: number;
  seed?: number;
  max_tokens?: number;
  stop?: string[];
  samplers?: string[];
  logit_bias?: unknown;
  grammar?: string;
  json_schema?: unknown;
}

/** Request-side settings. Built-ins (`builtin:*`) are read-only. `inference_presets.json`. */
export interface InferencePreset {
  id: string;
  name: string;
  builtin?: boolean;
  description?: string;
  system_prompt?: string | null;
  sampling: SamplingParams;
  reasoning?: ReasoningSettings;
  response_format?: unknown;
  tool_choice?: unknown;
  created_at: number;
  updated_at: number;
}

/** Schema version of a data file; `read_only` = written by a newer Harness Studio. */
export interface DataFileStatus {
  file: string;
  version: number | null;
  supported: number;
  read_only: boolean;
}

export interface ReasoningSettings {
  /** chat_template_kwargs.enable_thinking */
  enabled?: boolean;
  /** reasoning_format: none | auto | deepseek | deepseek-legacy */
  format?: string;
  /** reasoning_effort (gpt-oss): low | medium | high */
  effort?: string;
  /** thinking_budget_tokens (-1 = unlimited) */
  budget?: number;
}

/** Who uses a definition (`get_definition_refs`). */
export interface DefinitionRefs {
  profiles: { id: string; name: string }[];
  model_default_for: string[];
  session: boolean;
}

export type DefinitionRunState = 'missing' | 'idle' | 'loading' | 'running';

/** Run state of a definition across the server instances (`list_definition_states`). */
export interface DefinitionState {
  definition_id: string;
  alias: string;
  state: DefinitionRunState;
  servers: { server_id: string; status: ServerStatus; port: number }[];
}

/** Where `load_definition` runs a definition. */
export type LoadTarget =
  | { kind: 'server'; server_id: string }
  | { kind: 'profile'; profile_id: string }
  | { kind: 'new_server' };

/** A read `*.harness.toml` (`preview_model_definition_import`); nothing saved yet. */
export interface DefinitionImport {
  definition: ModelDefinition;
  model_found: boolean;
  hf_ref: string | null;
  missing: string[];
  alias_taken: boolean;
  suggested_alias: string;
  preset: InferencePreset | null;
  /** Embedded chat template text; the import saves it as a template file. */
  chat_template_text?: string | null;
}

/** One value an Ollama Modelfile import sets (`harness_core::modelfile::Mapped`). */
export interface ModelfileMapped {
  source: string;
  target: string;
  value: string;
}

/** Something a Modelfile import could not carry over; `code` selects the message (`mf.w_<code>`). */
export interface ModelfileWarning {
  code: string;
  line: number | null;
  detail: string;
}

/** A read Ollama Modelfile (`preview_modelfile_import`); nothing saved yet. */
export interface ModelfileImport {
  import: DefinitionImport;
  mapped: ModelfileMapped[];
  warnings: ModelfileWarning[];
  from: string;
}

// ---- My Models (Kütüphanem; `harness_core::library`) ----

export interface ParamInfo {
  count: number | null;
  label: string | null;
  /** `count` is an estimate (shown with "≈"). */
  estimated: boolean;
}

/** One companion file (VAE, text encoder) of a curated image model bundle. */
export interface ImageBundleComponent {
  /** Catalog id of the definition setting it fills (`vae`, `clip_l`, `clip_g`, `t5xxl`). */
  role: string;
  repo: string;
  file: string;
  size: number;
  /** The file on disk, when it is there. */
  path: string | null;
}

/** What an image model file needs besides itself (`get_image_bundle_plan`). */
export interface ImageBundlePlan {
  bundle_id: string;
  label: string;
  components: ImageBundleComponent[];
  /** Generation settings the model was made for (`width`, `height`, `steps`, `cfg_scale`...). */
  defaults: Record<string, string | number | boolean>;
}

export interface ModelCaps {
  chat: boolean;
  tools: boolean;
  reasoning: boolean;
  reasoning_control?: { effort: boolean; toggle: boolean; budget: boolean };
  vision: boolean;
  audio: boolean;
  audio_input: boolean;
  embedding: boolean;
  rerank: boolean;
  /** Speech recognition (whisper.cpp ggml). */
  asr?: boolean;
  /** Image / video generation (stable-diffusion.cpp). */
  image?: boolean;
  /** Speech synthesis (Kokoro / Piper folder run by sherpa-onnx). */
  tts?: boolean;
  /** What `/props` of a real run reported; null until the model was loaded once. */
  verified?: VerifiedCaps | null;
}

/** Chat-template capabilities as llama-server reported them (`/props` `chat_template_caps`). */
export interface VerifiedCaps {
  tools: boolean;
  /** The template takes a reasoning level or keeps earlier reasoning; `false` is not conclusive. */
  reasoning: boolean;
  reasoning_control?: { effort: boolean; toggle: boolean; budget: boolean };
  /** The template the server really used has the reasoning switches (fallback when `reasoning` is false). */
  template_reasoning?: boolean | null;
  /** Unix seconds of that run. */
  observed_at: number;
}

export interface ShardStatus {
  total: number;
  present: number;
  missing: string[];
}

export interface RepairInfo {
  repo_id: string | null;
  target_dir: string | null;
  /** Why the parts cannot be downloaded automatically (null = they can). */
  reason: string | null;
  /** Recorded download address of a part that is still there (custom URL / moved file). */
  source_url?: string | null;
  /** The user may give the address of one part to repair with. */
  url_allowed?: boolean;
}

export interface LibraryEntry {
  model: LocalModel & { source?: string | null; mmproj_path?: string | null };
  params: ParamInfo;
  caps: ModelCaps;
  /** Capabilities, format and runtime as the model registry joins them. */
  record: ModelRecord;
  /** LLM: its projector ids. Projector: the LLM ids it belongs to. */
  pairs: string[];
  multimodal: boolean;
  shards: ShardStatus | null;
  repair: RepairInfo | null;
  /** Hugging Face repo (`owner/name`) when the file sits in a downloaded repo's folder. */
  repo_id: string | null;
  favorite: boolean;
  /** Unix ms. */
  last_used_at: number | null;
  last_definition_id: string | null;
  preset_used: string | null;
  /** Unix ms the file arrived. */
  added_at: number | null;
  /** Bigger local LLMs this one can draft for (model ids). */
  draft_for: string[];
}

export interface LibraryView {
  entries: LibraryEntry[];
  state_error: string | null;
}

export type UseRole = 'base' | 'draft' | 'mmproj' | 'adapter';

export interface DeletePreview {
  model_id: string;
  name: string;
  linked: boolean;
  files: { path: string; size_bytes: number }[];
  definitions: { id: string; alias: string; role: UseRole }[];
  profiles: { id: string; name: string }[];
  running_on: string[];
}

export interface DeleteReport {
  files: string[];
  definitions_deleted: string[];
  definition_errors: string[];
}

export interface DuplicateGroup {
  size_bytes: number;
  pre_hash: string;
  members: { model_id: string; name: string; path: string; origin: 'library' | 'linked' }[];
}

export interface VerifiedMember {
  model_id: string;
  sha256: string;
  recorded: boolean;
}

export interface DraftSuggestion {
  model_id: string;
  path: string;
  name: string;
  quantization: string;
  size_bytes: number;
  params: ParamInfo;
  ratio: number;
}

// --- Runtime registry (P2 alternative runtimes; harness_core::runtimes) ---

export interface RuntimeInstalledAsset {
  name: string;
  url: string;
  size: number;
  sha256: string | null;
}

export type RuntimeSource =
  | { kind: 'github'; repo: string; tag: string; assets: RuntimeInstalledAsset[] }
  | { kind: 'local'; path: string; copied: boolean };

export type RuntimeTrust = 'verified' | 'user_confirmed' | 'local_files';

export interface RuntimeEntry {
  id: string;
  name: string;
  /** Runtime family (`llama.cpp`); older entries have none and are llama.cpp. */
  family?: string;
  source: RuntimeSource;
  dir: string;
  version: string | null;
  build: number | null;
  commit: string | null;
  backend: string | null;
  trust: RuntimeTrust;
  files: { name: string; size: number; mtime_ms: number | null; sha256: string }[];
  quant_types: string[];
  added_at: number;
  verified_at: number;
}

export interface RuntimeView {
  id: string;
  /** Empty for the official runtime. */
  name: string;
  kind: 'official' | 'system' | 'github' | 'local';
  entry: RuntimeEntry | null;
  binary_path: string | null;
  available: boolean;
  error: string | null;
  version: RuntimeCapabilities['version'] | null;
  backend: string | null;
  help_parsed: boolean;
  flag_count: number;
  /** Catalog parameter ids this binary has no flag for (hidden for it). */
  unsupported_params: string[];
  quant_types: string[];
  /** Official runtime only: configured mode vs what is really used. */
  official?: RuntimeOfficialState | null;
}

export interface RuntimeOfficialState {
  configured_mode: 'managed' | 'custom';
  source: 'managed' | 'custom' | 'system' | null;
  /** The configured mode can't be used; `source` is a fallback. */
  fallback: boolean;
  managed_installed: boolean;
  managed_version: string | null;
  /** A downloaded managed build waiting in staging (never activated). */
  staged: RuntimeCapabilities['version'] | null;
}

export interface RuntimeVerifyReport {
  id: string;
  ok: boolean;
  version: string | null;
  changed: string[];
  missing: string[];
  added: string[];
  accepted: boolean;
  error: string | null;
}

export interface RuntimeReleasePick {
  /** Runtime family the archives belong to. */
  family?: string;
  repo: string;
  tag: string;
  published_at: string | null;
  assets: ReleaseAsset[];
  backend: string | null;
  unverified: string[];
}

export type QuantSupport = 'supported' | 'unsupported' | 'unknown';

export interface TensorTypeInfo {
  id: number;
  name: string | null;
}

export interface RuntimeAdvice {
  model_path: string;
  special_types: TensorTypeInfo[];
  runtimes: { runtime_id: string; support: QuantSupport; missing: TensorTypeInfo[] }[];
  official_unsupported: boolean;
  suggested_runtime: string | null;
  fork: { id: string; name: string; repo: string; url: string } | null;
}

export interface RuntimeUsers {
  running: string[];
  definitions: string[];
  profiles: string[];
}

/** Provider for the built-in web_search tool; DuckDuckGo needs no API key. */
export type SearchProvider = 'none' | 'duckduckgo' | 'searxng' | 'brave' | 'tavily';

/** Managed ONNX Runtime (`harness-onnx`): one build of this platform and its state. */
export interface OnnxRuntimeFile {
  name: string;
  size: number;
  sha256: string;
}

export interface OnnxRuntimeInstalled {
  variant: string;
  version: string;
  dir: string;
  lib: string;
  files: OnnxRuntimeFile[];
  size_bytes: number;
  installed_at: number;
}

export interface OnnxRuntimeVariant {
  variant: string;
  version: string;
  /** Size of the archives to download. */
  download_bytes: number;
  /** Size of the installed files on disk. */
  disk_bytes: number;
  default: boolean;
  installed: OnnxRuntimeInstalled | null;
  /** Files of an earlier install that are missing or changed in size (install again). */
  damaged?: string[];
}

/** `onnx_runtime::PlatformSupport`: whether this OS / architecture has any ONNX Runtime build. */
export interface OnnxRuntimeSupport {
  supported: boolean;
  os: string;
  arch: string;
}

export interface OnnxRuntimeProgress {
  variant: string | null;
  current: string;
  downloaded_bytes: number;
  total_bytes: number;
}

/** Lemonade (Ryzen AI NPU and its own llama.cpp backends): the Settings > Runtime card. */
export type LemonadeModelKind = 'chat' | 'embedding' | 'rerank' | 'transcription' | 'speech' | 'image' | 'other';

export interface LemonadeModel {
  id: string;
  recipe: string;
  checkpoint: string;
  labels: string[];
  kind: LemonadeModelKind;
  size_gb: number;
  downloaded: boolean;
  suggested: boolean;
  update_available: boolean;
}

export interface LemonadeBackend {
  recipe: string;
  backend: string;
  state: string;
  version: string | null;
  message: string | null;
}

export interface LemonadeNpu {
  detected: boolean;
  generation: string | null;
  name: string | null;
  driver_installed: boolean;
  hardware_id: string | null;
}

export interface LemonadeInstall {
  id: string;
  name: string;
  version: string | null;
  tag: string | null;
  added_at: number;
  active: boolean;
}

export interface LemonadeSettings {
  backend: string;
  runtime: string | null;
}

export interface LemonadePull {
  target: string;
  file: string;
  file_index: number;
  total_files: number;
  bytes_downloaded: number;
  bytes_total: number;
  total_download_size: number;
  percent: number;
}

export interface LemonadeStatus {
  supported: boolean;
  installs: LemonadeInstall[];
  settings: LemonadeSettings;
  running: boolean;
  port: number | null;
  websocket_port: number | null;
  pid: number | null;
  base_url: string | null;
  running_runtime: string | null;
  running_version: string | null;
  started_at: number | null;
  npu: LemonadeNpu;
  backends: LemonadeBackend[];
  models_dir: string;
  log_file: string;
  install_progress: { current_asset: string; downloaded_bytes: number; total_bytes: number } | null;
  pull: LemonadePull | null;
}

export interface LemonadeModelList {
  live: boolean;
  updated_at: number;
  models: LemonadeModel[];
  backends: LemonadeBackend[];
}

/** Safetensors to GGUF conversion (`harness_core::convert`). */
export interface ConvertPlanFile {
  name: string;
  size: number;
  sha256: string | null;
}

export interface ConvertQuantEstimate {
  quant: string;
  size_bytes: number;
  disk_bytes: number;
}

export interface ConvertPlan {
  repo_id: string;
  architecture: string | null;
  task: string | null;
  license: string | null;
  gated: boolean;
  convertible: boolean;
  /** Keyed error (`convert.unsupported_architecture`...) when it is not convertible. */
  reason: string | null;
  vision: boolean;
  files: ConvertPlanFile[];
  download_bytes: number;
  parameters: number;
  /** vocab_size x hidden_size of the config (0 unknown). */
  embedding_parameters?: number;
  /** Weights a k-quant cannot hold (rows not a multiple of 256), stored in a fallback type. */
  fallback_parameters?: number;
  quants: ConvertQuantEstimate[];
  default_quant: string;
  free_bytes: number;
  work_root: string;
  env_ready: boolean;
  env_estimated_bytes: number;
  runtime_tag: string | null;
  architectures_checked: boolean;
  /** `quantization_config.quant_method` of a pre-quantized checkpoint. */
  quant_method?: string | null;
  /** The model this one was made from (card `base_model`). */
  base_model?: string | null;
}

export type ConvertStage = 'queued' | 'preparing' | 'downloading' | 'converting' | 'quantizing' | 'finalizing' | 'done';
export type ConvertStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';

export interface ConvertJob {
  id: number;
  repo_id: string;
  quant: string;
  keep_original: boolean;
  status: ConvertStatus;
  stage: ConvertStage;
  progress: number | null;
  detail: string;
  downloaded_bytes: number;
  total_bytes: number;
  error: string | null;
  output: string | null;
  mmproj: string | null;
  warning: string | null;
  kept_bytes: number;
  created_at: number;
  updated_at: number;
}

export interface ConvertEnvProgress {
  step: string;
  detail: string;
  downloaded_bytes: number;
  total_bytes: number;
}

/** A managed Python environment of an optional engine (`pyenv::envs::EnvStatus`). */
export interface PythonEnvStatus {
  name: string;
  supported: boolean;
  /** Why this machine can't build it (a backend error key with values), when it can't. */
  refusal?: string | null;
  installed: boolean;
  ready: boolean;
  manifest: { uv_version: string; python: string; lock_sha256: string; installed_at: number } | null;
  size_bytes: number;
  dir: string;
  estimated_size_bytes: number;
  progress: ConvertEnvProgress | null;
  /** The CPU env this is a GPU build of (`hf-cuda` of `hf-cpu`). */
  gpu_build_of?: string | null;
  /** The one of a CPU / GPU pair to install on this machine. */
  preferred?: boolean;
}

export interface ConvertEnvStatus {
  supported: boolean;
  installed: boolean;
  ready: boolean;
  runtime_tag: string | null;
  manifest: { uv_version: string; python: string; tag: string; requirements_sha256: string; installed_at: number } | null;
  size_bytes: number;
  dir: string;
  estimated_size_bytes: number;
  progress: ConvertEnvProgress | null;
}

/** Settings > Automations: the user's own n8n (the API key is in the keychain, never here). */
export interface N8nView {
  url: string;
  key_set: boolean;
}

export interface N8nConnection {
  ok: boolean;
  url: string;
  has_workflows: boolean;
}

export interface N8nWebhook {
  method: string;
  path: string;
  url: string;
}

export interface N8nWorkflow {
  id: string;
  name: string;
  active: boolean;
  triggers: string[];
  webhook: N8nWebhook | null;
  updated_at: string | null;
  editor_url: string;
}

export interface N8nExecution {
  id: string;
  workflow_id: string;
  status: string;
  mode: string;
  started_at: string | null;
  stopped_at: string | null;
  editor_url: string;
}

export interface N8nExecutionError {
  message: string | null;
  node: string | null;
}

export interface N8nTriggerResult {
  status: number;
  ok: boolean;
  body: string;
}

export interface N8nOpenAiConnection {
  running: boolean;
  base_url: string | null;
  requires_token: boolean;
}

/** Playground-only tools do not change a model's runtime serving mode. */
export type PlaygroundMode = ServeMode | 'ocr';
