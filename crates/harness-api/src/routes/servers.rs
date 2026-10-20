//! Server instances and profiles: list, create, start/stop, load/unload, metrics, logs.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Extension, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::Stream;
use harness_core::capabilities::verify::EffectiveLaunchValues;
use harness_core::failure::ServerFailure;
use harness_core::lora_live::{self, LoraAdapter, LoraScale};
use harness_core::orchestrator::{self, LoadTarget};
use harness_core::params::ParamValues;
use harness_core::process::{self, ActiveServerInfo, LaunchConfig, RestartState, ServerStatus};
use harness_core::recovery::RecoveryPlan;
use harness_core::servers::{self, ServerProfile, ServerRegistry};
use harness_core::telemetry::{TelemetryHistoryPoint, TelemetrySnapshot};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::auth::Caller;
use crate::error::{blocking, with_id, ApiError, ApiResult, Body, ErrorBody, OptBody, P, Q};
use crate::AppState;
use harness_core::control_api::Principal;

/// A server: a running/known instance, a saved profile, or both (same id).
#[derive(Serialize, ToSchema)]
pub struct ServerView {
    pub id: String,
    /// Profile name (instances without a profile: the id).
    pub name: String,
    /// A saved profile exists with this id.
    pub is_profile: bool,
    pub status: ServerStatus,
    /// Connection info while starting/running (never the API key).
    pub active: Option<ActiveServerInfo>,
    /// OpenAI-compatible base URL while starting/running (`http://host:port/v1`).
    pub endpoint: Option<String>,
    /// Model id the server serves (`/v1/models`): definition alias or file stem.
    pub served_alias: Option<String>,
    pub definition_id: Option<String>,
    /// The saved profile (effective launch config; the API key is never included).
    pub profile: Option<ServerProfile>,
    /// Why the last run failed (`servers.out_of_memory`...); `None` while it runs.
    pub failure: Option<ServerFailure>,
    /// A scheduled automatic restart after a crash.
    pub restart: Option<RestartState>,
}

fn strip_secrets(mut config: LaunchConfig) -> LaunchConfig {
    config.api_key = None;
    config.binary_path = Default::default();
    config.mcp_config_path = None;
    config
}

fn view(registry: &ServerRegistry, id: &str, profile: Option<ServerProfile>) -> ServerView {
    let pm = registry.get(id);
    let status = pm
        .as_ref()
        .map(|pm| pm.get_status())
        .unwrap_or(ServerStatus::Stopped);
    let active = pm.as_ref().and_then(|pm| pm.active_server());
    let last = pm.as_ref().and_then(|pm| pm.launch_config());
    let config = last
        .clone()
        .or_else(|| profile.as_ref().map(|p| p.config.clone()));
    let endpoint = active.as_ref().map(|a| {
        let engine = last
            .as_ref()
            .map(harness_core::engine::server_for)
            .unwrap_or(harness_core::engine::llama_cpp());
        engine.openai_base(&format!("http://{}:{}", a.connect_host, a.port))
    });
    ServerView {
        id: id.to_string(),
        name: profile
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| id.to_string()),
        is_profile: profile.is_some(),
        status,
        active,
        endpoint,
        served_alias: config.as_ref().and_then(process::served_alias),
        definition_id: config.as_ref().and_then(|c| c.definition_id.clone()),
        profile: profile.map(|mut p| {
            p.config = strip_secrets(p.config);
            p
        }),
        failure: pm.as_ref().and_then(|pm| pm.last_failure()),
        restart: pm.as_ref().and_then(|pm| pm.restart_state()),
    }
}

async fn profiles() -> ApiResult<Vec<ServerProfile>> {
    blocking(servers::load_profiles).await
}

async fn find_view(s: &AppState, id: &str) -> ApiResult<ServerView> {
    let profile = profiles().await?.into_iter().find(|p| p.id == id);
    if profile.is_none() && s.ctx.registry.get(id).is_none() {
        return Err(ApiError::not_found("server", id));
    }
    Ok(view(&s.ctx.registry, id, profile))
}

#[utoipa::path(get, path = "/servers", tag = "servers",
    responses((status = 200, body = Vec<ServerView>)))]
pub async fn list_servers(State(s): State<AppState>) -> ApiResult<Json<Vec<ServerView>>> {
    let profiles = profiles().await?;
    let mut ids: Vec<String> = s.ctx.registry.all().into_iter().map(|(id, _)| id).collect();
    for p in &profiles {
        if !ids.contains(&p.id) {
            ids.push(p.id.clone());
        }
    }
    Ok(Json(
        ids.iter()
            .map(|id| {
                let profile = profiles.iter().find(|p| &p.id == id).cloned();
                view(&s.ctx.registry, id, profile)
            })
            .collect(),
    ))
}

/// Creates a server profile (`id` empty = derived from the name). `config.api_key` is written
/// to the OS keychain; only its reference is stored.
#[utoipa::path(post, path = "/servers", tag = "servers", request_body = ServerProfile,
    responses((status = 201, body = ServerView), (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn create_server(
    State(s): State<AppState>,
    Body(body): Body<serde_json::Value>,
) -> ApiResult<(StatusCode, Json<ServerView>)> {
    let given = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let mut profile: ServerProfile = with_id(body, &given)?;
    let existing = profiles().await?;
    if profile.id.trim().is_empty() {
        let mut taken: Vec<String> = s.ctx.registry.all().into_iter().map(|(id, _)| id).collect();
        taken.extend(existing.iter().map(|p| p.id.clone()));
        taken.push(servers::DEFAULT_SERVER_ID.to_string());
        profile.id = orchestrator::new_server_id(&profile.name, &taken);
    } else if existing.iter().any(|p| p.id == profile.id) {
        return Err(ApiError::conflict("servers.exists").with("id", profile.id));
    }
    let id = profile.id.clone();
    blocking(move || servers::save_profile(profile)).await??;
    Ok((StatusCode::CREATED, Json(find_view(&s, &id).await?)))
}

#[utoipa::path(get, path = "/servers/{id}", tag = "servers",
    params(("id" = String, Path)),
    responses((status = 200, body = ServerView), (status = 404, body = ErrorBody)))]
pub async fn get_server(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<ServerView>> {
    Ok(Json(find_view(&s, &id).await?))
}

/// Replaces a saved profile (takes effect on the next start).
#[utoipa::path(put, path = "/servers/{id}", tag = "servers",
    params(("id" = String, Path)), request_body = ServerProfile,
    responses((status = 200, body = ServerView), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn update_server(
    State(s): State<AppState>,
    P(id): P<String>,
    Body(body): Body<serde_json::Value>,
) -> ApiResult<Json<ServerView>> {
    let profile: ServerProfile = with_id(body, &id)?;
    if !profiles().await?.iter().any(|p| p.id == id) {
        return Err(ApiError::not_found("profile", &id));
    }
    blocking(move || servers::save_profile(profile)).await??;
    Ok(Json(find_view(&s, &id).await?))
}

/// Deletes the profile (and its keychain key) and removes the instance (stopping it).
/// The default instance cannot be removed.
#[utoipa::path(delete, path = "/servers/{id}", tag = "servers",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn delete_server(State(s): State<AppState>, P(id): P<String>) -> ApiResult<StatusCode> {
    let is_profile = profiles().await?.iter().any(|p| p.id == id);
    let instance = s.ctx.registry.get(&id).is_some();
    if !is_profile && !instance {
        return Err(ApiError::not_found("server", &id));
    }
    if is_profile {
        let pid = id.clone();
        blocking(move || servers::delete_profile(&pid)).await??;
    }
    if instance && id != servers::DEFAULT_SERVER_ID {
        s.ctx.registry.remove(&id).await?;
        s.ctx.telemetry.lock().await.forget(&id);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema, Default)]
pub struct StartRequest {
    /// Definition to serve (instances without a profile). A profile serves its own.
    pub definition_id: Option<String>,
    /// Catalog parameters layered over the definition/profile for this start only.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub overrides: ParamValues,
}

/// Starts a server. Network rules apply (`network.lan_without_key` → 409); "open anyway"
/// needs the admin-only `/servers/{id}/exposure-ack`.
#[utoipa::path(post, path = "/servers/{id}/start", tag = "servers",
    params(("id" = String, Path)), request_body = StartRequest,
    responses((status = 202, body = ServerView), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn start_server(
    State(s): State<AppState>,
    P(id): P<String>,
    OptBody(body): OptBody<StartRequest>,
) -> ApiResult<(StatusCode, Json<ServerView>)> {
    let req = body.unwrap_or_default();
    let plan =
        orchestrator::plan_start(&s.ctx.registry, &id, req.definition_id, req.overrides).await?;
    orchestrator::start_plan(&s.ctx.registry, &s.ctx.runtime, &s.ctx.gate, plan).await?;
    Ok((StatusCode::ACCEPTED, Json(find_view(&s, &id).await?)))
}

/// The last failure of a server and, after an out-of-memory failure, the ways to retry
/// (lower `-ngl`/context/`-ub`, KV cache quantization, `--fit`, MoE experts on the CPU, stop
/// another server), each with concrete values. `servers.no_failure` (409) when it has none.
#[utoipa::path(get, operation_id = "get_server_recovery", path = "/servers/{id}/recovery", tag = "servers",
    params(("id" = String, Path)),
    responses((status = 200, body = RecoveryPlan), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn recovery(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<RecoveryPlan>> {
    if s.ctx.registry.get(&id).is_none() {
        return Err(ApiError::not_found("server", &id));
    }
    Ok(Json(
        orchestrator::recovery_plan(&s.ctx.registry, &id).await?,
    ))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct RetryRequest {
    /// A recovery option id from `GET /servers/{id}/recovery` (its overrides and servers to
    /// stop are used).
    pub option: Option<String>,
    /// Catalog parameters over the last launch (added to the option's).
    #[serde(default)]
    #[schema(value_type = Object)]
    pub overrides: ParamValues,
    /// Servers to stop first (added to the option's).
    #[serde(default)]
    pub unload: Vec<String>,
    /// Also save the values to the model definition (and the profile's own overrides).
    #[serde(default)]
    pub save: bool,
}

/// Starts a stopped or failed server again: its last launch with the given overrides (for
/// this start only unless `save`). A real start; poll `GET /servers/{id}` for the result.
#[utoipa::path(post, operation_id = "retry_server", path = "/servers/{id}/retry", tag = "servers",
    params(("id" = String, Path)), request_body = RetryRequest,
    responses((status = 202, body = ServerView), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn retry(
    State(s): State<AppState>,
    P(id): P<String>,
    OptBody(body): OptBody<RetryRequest>,
) -> ApiResult<(StatusCode, Json<ServerView>)> {
    let req = body.unwrap_or_default();
    if s.ctx.registry.get(&id).is_none() {
        return Err(ApiError::not_found("server", &id));
    }
    let mut overrides = ParamValues::new();
    let mut unload = Vec::new();
    if let Some(option) = &req.option {
        let plan = orchestrator::recovery_plan(&s.ctx.registry, &id).await?;
        let opt = plan
            .options
            .into_iter()
            .find(|o| &o.id == option)
            .ok_or_else(|| ApiError::not_found("recovery option", option))?;
        overrides = opt.overrides;
        unload = opt.unload;
    }
    overrides.extend(req.overrides);
    unload.extend(req.unload);
    orchestrator::retry_with_overrides(
        &s.ctx.registry,
        &s.ctx.runtime,
        &s.ctx.gate,
        &id,
        overrides,
        unload,
        req.save,
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(find_view(&s, &id).await?)))
}

#[utoipa::path(post, path = "/servers/{id}/stop", tag = "servers",
    params(("id" = String, Path)),
    responses((status = 200, body = ServerView), (status = 404, body = ErrorBody)))]
pub async fn stop_server(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<ServerView>> {
    let pm = s
        .ctx
        .registry
        .get(&id)
        .ok_or_else(|| ApiError::not_found("server", &id))?;
    pm.stop().await?;
    Ok(Json(find_view(&s, &id).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ExposureAckRequest {
    /// The network address the server binds (as shown to the user).
    pub host: String,
}

/// "Open anyway" for a network bind without an API key (admin only; same record as the GUI's
/// confirmation). Never allowed while built-in tools / MCP are on.
#[utoipa::path(post, operation_id = "acknowledge_server_exposure", path = "/servers/{id}/exposure-ack", tag = "servers",
    params(("id" = String, Path)), request_body = ExposureAckRequest,
    responses((status = 204), (status = 403, body = ErrorBody)))]
pub async fn acknowledge_exposure(
    P(id): P<String>,
    Body(req): Body<ExposureAckRequest>,
) -> ApiResult<StatusCode> {
    blocking(move || harness_core::network::acknowledge(&id, &req.host)).await??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct LoadRequest {
    /// Definition id, or use `alias`.
    pub definition_id: Option<String>,
    /// Definition alias (the served model name).
    pub alias: Option<String>,
    /// Instance or profile to load it on; omitted = a new instance on the next free port.
    pub server_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct LoadResponse {
    pub server_id: String,
    pub server: ServerView,
}

/// Loads a definition: on an instance (restarted if it runs something else), on a profile
/// serving it, or on a new instance.
#[utoipa::path(post, operation_id = "load_model", path = "/load", tag = "servers", request_body = LoadRequest,
    responses((status = 202, body = LoadResponse), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn load(
    State(s): State<AppState>,
    Body(req): Body<LoadRequest>,
) -> ApiResult<(StatusCode, Json<LoadResponse>)> {
    let key = req
        .definition_id
        .or(req.alias)
        .ok_or_else(|| ApiError::bad_request("api.definition_or_alias_required"))?;
    let def = blocking(move || orchestrator::resolve_definition(&key)).await??;
    let target = match req.server_id {
        None => LoadTarget::NewServer,
        Some(id) if profiles().await?.iter().any(|p| p.id == id) => {
            LoadTarget::Profile { profile_id: id }
        }
        Some(id) => LoadTarget::Server { server_id: id },
    };
    let id =
        orchestrator::load_definition(&s.ctx.registry, &s.ctx.runtime, &s.ctx.gate, &def, target)
            .await?;
    let server = find_view(&s, &id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(LoadResponse {
            server_id: id,
            server,
        }),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct UnloadRequest {
    /// Stop every server serving this definition alias (or id).
    pub alias: Option<String>,
    /// Stop this server.
    pub server_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct UnloadResponse {
    /// Servers that were stopped.
    pub stopped: Vec<String>,
}

#[utoipa::path(post, operation_id = "unload_model", path = "/unload", tag = "servers", request_body = UnloadRequest,
    responses((status = 200, body = UnloadResponse), (status = 404, body = ErrorBody)))]
pub async fn unload(
    State(s): State<AppState>,
    Body(req): Body<UnloadRequest>,
) -> ApiResult<Json<UnloadResponse>> {
    if let Some(id) = req.server_id {
        let pm = s
            .ctx
            .registry
            .get(&id)
            .ok_or_else(|| ApiError::not_found("server", &id))?;
        pm.stop().await?;
        return Ok(Json(UnloadResponse { stopped: vec![id] }));
    }
    let key = req
        .alias
        .ok_or_else(|| ApiError::bad_request("api.alias_or_server_required"))?;
    let def = blocking(move || orchestrator::resolve_definition(&key)).await??;
    let stopped = orchestrator::unload_definition(&s.ctx.registry, &def).await?;
    if let Some(sched) = harness_core::gateway::scheduler() {
        sched.clear_keep_alive(&def);
    }
    Ok(Json(UnloadResponse { stopped }))
}

fn router_endpoint(s: &AppState, id: &str) -> ApiResult<harness_core::router::RouterEndpoint> {
    let pm = s
        .ctx
        .registry
        .get(id)
        .ok_or_else(|| ApiError::not_found("server", id))?;
    Ok(harness_core::router::endpoint(&pm)?)
}

#[derive(Serialize, ToSchema)]
pub struct RouterModelsResponse {
    pub models: Vec<harness_core::router::RouterModel>,
}

/// The models of a router server (llama-server `--models-dir`) and whether each is loaded.
/// 409 when the server is not running or not in router mode.
#[utoipa::path(get, operation_id = "list_router_models", path = "/servers/{id}/router/models", tag = "servers",
    params(("id" = String, Path)),
    responses((status = 200, body = RouterModelsResponse), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn router_models(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<RouterModelsResponse>> {
    let ep = router_endpoint(&s, &id)?;
    Ok(Json(RouterModelsResponse {
        models: harness_core::router::list(&ep).await?,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct RouterModelRequest {
    /// The model's name as listed by `GET /servers/{id}/router/models`.
    pub model: String,
    /// Load only: wait up to this many seconds until the model is loaded (0 = answer at
    /// once with the state the router reports).
    #[serde(default)]
    pub wait_seconds: u64,
}

/// Loads one model of a router server (it stays loaded until unloaded or evicted by
/// `--models-max`).
#[utoipa::path(post, operation_id = "router_load_model", path = "/servers/{id}/router/models/load", tag = "servers",
    params(("id" = String, Path)), request_body = RouterModelRequest,
    responses((status = 200, body = harness_core::router::RouterModel),
        (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn router_load(
    State(s): State<AppState>,
    P(id): P<String>,
    Body(req): Body<RouterModelRequest>,
) -> ApiResult<Json<harness_core::router::RouterModel>> {
    let ep = router_endpoint(&s, &id)?;
    let wait = (req.wait_seconds > 0).then(|| Duration::from_secs(req.wait_seconds));
    Ok(Json(
        harness_core::router::load(&ep, &req.model, wait).await?,
    ))
}

/// Unloads one model of a router server.
#[utoipa::path(post, operation_id = "router_unload_model", path = "/servers/{id}/router/models/unload", tag = "servers",
    params(("id" = String, Path)), request_body = RouterModelRequest,
    responses((status = 200, body = harness_core::router::RouterModel),
        (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn router_unload(
    State(s): State<AppState>,
    P(id): P<String>,
    Body(req): Body<RouterModelRequest>,
) -> ApiResult<Json<harness_core::router::RouterModel>> {
    let ep = router_endpoint(&s, &id)?;
    Ok(Json(harness_core::router::unload(&ep, &req.model).await?))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MetricsQuery {
    /// Also return up to this many history points (1 Hz, newest last).
    pub history: Option<usize>,
}

#[derive(Serialize, ToSchema)]
pub struct MetricsResponse {
    pub snapshot: TelemetrySnapshot,
    pub history: Vec<TelemetryHistoryPoint>,
}

/// Live telemetry (sampled once a second from `/metrics` and `/slots`).
#[utoipa::path(get, operation_id = "get_server_metrics", path = "/servers/{id}/metrics", tag = "servers",
    params(("id" = String, Path), MetricsQuery),
    responses((status = 200, body = MetricsResponse), (status = 404, body = ErrorBody)))]
pub async fn metrics(
    State(s): State<AppState>,
    P(id): P<String>,
    Q(q): Q<MetricsQuery>,
) -> ApiResult<Json<MetricsResponse>> {
    servers::validate_id(&id)?;
    if s.ctx.registry.get(&id).is_none() {
        return Err(ApiError::not_found("server", &id));
    }
    let snapshot =
        harness_core::telemetry::TelemetryPoller::fresh_snapshot(&s.ctx.telemetry, &id).await;
    let poller = s.ctx.telemetry.lock().await;
    Ok(Json(MetricsResponse {
        snapshot,
        history: match q.history {
            Some(n) if n > 0 => poller.get_history(&id, Some(n)),
            _ => Vec::new(),
        },
    }))
}

#[derive(Serialize, ToSchema)]
pub struct ServerKeyResponse {
    /// The key requests to the server's own endpoint need: the user's, or the one made for this
    /// run (a Python worker always has one). `null` when it needs none or is not running.
    #[schema(example = "<key of this run>")]
    pub api_key: Option<String>,
}

/// Only the session token (the desktop app's, or `harness`'s on this computer) reads server keys,
/// and only while the API listens on loopback: a named token or a network bind could hand them to
/// another computer.
pub(crate) fn may_read_server_key(caller: &Caller, loopback: bool) -> bool {
    matches!(caller.principal, Principal::Session) && loopback
}

/// The key of a running server's own endpoint, for local clients that talk to it directly
/// (`harness run` without the gateway). Session token on a loopback API only
/// (`api.server_key_local_only`, 403); never cached (`Cache-Control: no-store`).
#[utoipa::path(get, operation_id = "get_server_api_key", path = "/servers/{id}/api-key", tag = "servers",
    params(("id" = String, Path)),
    responses(
        (status = 200, description = "The key of the server's run (`null`: none, or not running); never cached", body = ServerKeyResponse),
        (status = 403, description = "`api.server_key_local_only`: not the local session token, or the API is not bound to loopback", body = ErrorBody),
        (status = 404, description = "Unknown server", body = ErrorBody)))]
pub async fn api_key(
    State(s): State<AppState>,
    Extension(caller): Extension<Caller>,
    P(id): P<String>,
) -> ApiResult<Response> {
    if !may_read_server_key(&caller, s.loopback) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "api.server_key_local_only",
        ));
    }
    servers::validate_id(&id)?;
    let Some(server) = s.ctx.registry.get(&id) else {
        return Err(ApiError::not_found("server", &id));
    };
    let api_key = server
        .active_server()
        .and_then(|a| a.api_key)
        .filter(|k| !k.is_empty());
    let mut resp = Json(ServerKeyResponse { api_key }).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(resp)
}

#[derive(Serialize, ToSchema)]
pub struct EffectiveResponse {
    /// Launch config of the current (or last) run (no API key, no binary path).
    pub config: Option<LaunchConfig>,
    /// What the running server actually uses (`None` until checked after start).
    pub values: Option<EffectiveLaunchValues>,
}

#[utoipa::path(get, operation_id = "get_effective_server_config", path = "/servers/{id}/effective", tag = "servers",
    params(("id" = String, Path)),
    responses((status = 200, body = EffectiveResponse), (status = 404, body = ErrorBody)))]
pub async fn effective(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<EffectiveResponse>> {
    let pm = s
        .ctx
        .registry
        .get(&id)
        .ok_or_else(|| ApiError::not_found("server", &id))?;
    Ok(Json(EffectiveResponse {
        config: pm.launch_config().map(strip_secrets),
        values: pm.effective_values(),
    }))
}

#[derive(Serialize, ToSchema)]
pub struct LoraResponse {
    /// The adapters of the running server with their current scales.
    pub adapters: Vec<LoraAdapter>,
}

#[derive(Deserialize, ToSchema)]
pub struct LoraSetRequest {
    /// Scale changes by adapter id; adapters not named keep their scale.
    pub adapters: Vec<LoraScale>,
}

/// LoRA adapters a running server loaded (`GET /lora-adapters` of llama-server), with their
/// current scales. `lora.server_not_running` (409) unless the server runs.
#[utoipa::path(get, operation_id = "list_server_loras", path = "/servers/{id}/lora", tag = "servers",
    params(("id" = String, Path)),
    responses((status = 200, body = LoraResponse), (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn lora_list(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<LoraResponse>> {
    servers::validate_id(&id)?;
    Ok(Json(LoraResponse {
        adapters: lora_live::list_for(&s.ctx.registry, &id).await?,
    }))
}

/// Sets the scales of a running server's LoRA adapters without a restart (scale 0 turns an
/// adapter off). Returns the adapters as the server reports them afterwards.
#[utoipa::path(post, operation_id = "set_server_loras", path = "/servers/{id}/lora", tag = "servers",
    params(("id" = String, Path)), request_body = LoraSetRequest,
    responses((status = 200, body = LoraResponse), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn lora_set(
    State(s): State<AppState>,
    P(id): P<String>,
    Body(req): Body<LoraSetRequest>,
) -> ApiResult<Json<LoraResponse>> {
    servers::validate_id(&id)?;
    Ok(Json(LoraResponse {
        adapters: lora_live::set_scales_for(&s.ctx.registry, &id, &req.adapters).await?,
    }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogsQuery {
    /// Sequence number to continue from (`next` of the previous call); omitted = the buffer.
    pub since: Option<u64>,
    /// At most this many lines (the newest).
    pub limit: Option<usize>,
}

#[derive(Serialize, ToSchema)]
pub struct LogsResponse {
    /// Pass as `since` to get only newer lines.
    pub next: u64,
    pub lines: Vec<String>,
}

/// Live log of the current run (ring buffer of the last 1000 lines).
#[utoipa::path(get, operation_id = "get_server_logs", path = "/servers/{id}/logs", tag = "servers",
    params(("id" = String, Path), LogsQuery),
    responses((status = 200, body = LogsResponse), (status = 404, body = ErrorBody)))]
pub async fn logs(
    State(s): State<AppState>,
    P(id): P<String>,
    Q(q): Q<LogsQuery>,
) -> ApiResult<Json<LogsResponse>> {
    let pm = s
        .ctx
        .registry
        .get(&id)
        .ok_or_else(|| ApiError::not_found("server", &id))?;
    let (next, mut lines) = pm.get_run_logs_since(q.since.unwrap_or(0));
    if let Some(limit) = q.limit {
        let skip = lines.len().saturating_sub(limit);
        lines.drain(..skip);
    }
    Ok(Json(LogsResponse { next, lines }))
}

/// New log lines as server-sent events (`event: log`, data = `LogsResponse`), from `since`
/// (default: lines arriving from now on).
#[utoipa::path(get, operation_id = "stream_server_logs", path = "/servers/{id}/logs/stream", tag = "events",
    params(("id" = String, Path), LogsQuery),
    responses((status = 200, content_type = "text/event-stream", body = LogsResponse),
        (status = 404, body = ErrorBody)))]
pub async fn logs_stream(
    State(s): State<AppState>,
    P(id): P<String>,
    Q(q): Q<LogsQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let pm = s
        .ctx
        .registry
        .get(&id)
        .ok_or_else(|| ApiError::not_found("server", &id))?;
    let start = q.since.unwrap_or_else(|| pm.get_logs_since(u64::MAX).0);
    let stream = futures_util::stream::unfold((pm, start), |(pm, since)| async move {
        loop {
            let (next, lines) = pm.get_run_logs_since(since);
            if !lines.is_empty() {
                let data = serde_json::to_string(&LogsResponse { next, lines }).unwrap_or_default();
                let event = Event::default().event("log").data(data);
                return Some((Ok(event), (pm, next)));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[cfg(test)]
mod key_tests {
    use super::*;

    /// The session token reads server keys only while the API is bound to loopback; named
    /// tokens never do, whatever their scope.
    #[test]
    fn server_keys_are_for_the_local_session_only() {
        let session = Caller {
            principal: Principal::Session,
            scopes: vec![harness_core::control_api::Scope::Admin],
        };
        let named = Caller {
            principal: Principal::Named {
                id: "t".into(),
                name: "remote".into(),
            },
            scopes: vec![harness_core::control_api::Scope::Admin],
        };
        let browser = Caller {
            principal: Principal::Browser,
            scopes: vec![harness_core::control_api::Scope::Admin],
        };
        assert!(!may_read_server_key(&browser, true));
        assert!(!may_read_server_key(&browser, false));
        assert!(may_read_server_key(&session, true));
        assert!(!may_read_server_key(&session, false), "network bind");
        assert!(!may_read_server_key(&named, true));
        assert!(!may_read_server_key(&named, false));
    }
}
