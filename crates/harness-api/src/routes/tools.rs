//! Tool engine (`harness_core::tools`): list the tools, run one, read and change the policy,
//! read the tool log. Thin adapters over [`ToolService`]; the rules (policy, working directory,
//! limits, log) live in the core.
//!
//! * `GET /tools` and `GET /tools/policy` need `read`. Running a tool needs `tools`; changing the
//!   policy and the log (it holds the arguments of every call) need `admin`.
//! * The API cannot ask the user: a call the policy would ask about is answered `403
//!   tools.approval_required` unless the body says `approved` (a client that asked the human
//!   itself, like `harness tools run`). Both `approved` and the policy are `admin` matters, so
//!   `approved` gives an admin token nothing it could not do by saving a looser policy; a
//!   `tools` token that sends `approved` (or a `workdir` of its own, which would move the
//!   file tools' boundary) gets `403 api.insufficient_scope` (`admin`). Such a token runs
//!   exactly what the saved policy lets through, inside the saved working directory.
//!   `tools.disabled` (403) when the policy is "off", whatever `approved` says.
//! * A tool that ran but failed (bad arguments, a path outside the working directory, an edit
//!   that matches nothing, a timeout) is a normal `200` whose `error` names the code: the
//!   text is what a model should read.

use axum::http::StatusCode;
use axum::{Extension, Json};
use harness_core::analytics::{self, ToolCallQuery, ToolCallRow};
use harness_core::control_api::{Principal, Scope};
use harness_core::tools::{
    self, McpServerStatus, ResourceContent, ResourceList, RunOptions, SetWebSearchSettings,
    ToolErrorCode, ToolInvocation, ToolListing, ToolOrigin, ToolResult, ToolService, ToolSettings,
    WebSearchSettingsView,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use utoipa::{IntoParams, ToSchema};

use crate::auth::Caller;
use crate::error::{blocking, ApiError, ApiResult, Body, ErrorBody, P, Q};

/// The tools and the policy they are judged by. Each tool carries `decision`: what a call would
/// do now (`run`, `ask` = needs approval, `refuse`).
#[utoipa::path(get, path = "/tools", tag = "tools", params(ListToolsQuery),
    responses((status = 200, body = ToolListing)))]
pub async fn list_tools(Q(q): Q<ListToolsQuery>) -> ApiResult<Json<ToolListing>> {
    // MCP servers are started the first time somebody lists the tools.
    if !q.builtin_only {
        ToolService::shared().sync_mcp(false).await;
    }
    let listing = blocking(|| ToolService::shared().current_listing()).await?;
    Ok(Json(listing))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListToolsQuery {
    /// Do not start or reconnect MCP servers first (the MCP tools already connected are still
    /// listed). `harness mcp` lists this way: if Harness consumed its own `harness mcp`, a
    /// listing that starts the MCP servers would start `harness mcp` again, without end.
    #[serde(default)]
    pub builtin_only: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct McpStatusQuery {
    /// Try again servers that failed to start (they are left alone for a minute otherwise).
    #[serde(default)]
    pub retry: bool,
}

/// Harness's own MCP client: each configured server (`mcp_servers.json`) with its state
/// (`idle`, `connected`, `failed` + `error`, `disabled`), transport, negotiated MCP version and
/// the number of tools it registered. Servers are started on the first call.
#[utoipa::path(get, operation_id = "get_mcp_status", path = "/tools/mcp", tag = "tools", params(McpStatusQuery),
    responses((status = 200, body = Vec<McpServerStatus>)))]
pub async fn mcp_status(Q(q): Q<McpStatusQuery>) -> ApiResult<Json<Vec<McpServerStatus>>> {
    let service = ToolService::shared();
    service.sync_mcp(q.retry).await;
    Ok(Json(service.mcp().status()))
}

/// The state of one configured MCP server.
fn mcp_server_status(service: &ToolService, server: &str) -> ApiResult<McpServerStatus> {
    service
        .mcp()
        .status()
        .into_iter()
        .find(|s| s.name == server)
        .ok_or_else(|| ApiError::not_found("mcp_server", server))
}

/// The resources and resource templates of an MCP server (`resources/list`,
/// `resources/templates/list`). Needs `tools`.
#[utoipa::path(get, operation_id = "list_mcp_resources", path = "/tools/mcp/{server}/resources", tag = "tools",
    params(("server" = String, Path, description = "Server name in `mcp_servers.json`")),
    responses((status = 200, body = ResourceList), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn mcp_resources(P(server): P<String>) -> ApiResult<Json<ResourceList>> {
    let service = ToolService::shared();
    mcp_server_status(service, &server)?;
    Ok(Json(service.mcp().resources(&server).await?))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ResourceQuery {
    /// The resource's URI (from the list, or a filled-in template).
    pub uri: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResourceRead {
    pub contents: Vec<ResourceContent>,
}

/// Reads one resource of an MCP server (`resources/read`). Needs `tools`.
#[utoipa::path(get, operation_id = "read_mcp_resource", path = "/tools/mcp/{server}/resource", tag = "tools",
    params(("server" = String, Path, description = "Server name in `mcp_servers.json`"), ResourceQuery),
    responses((status = 200, body = ResourceRead), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn mcp_resource(
    P(server): P<String>,
    Q(q): Q<ResourceQuery>,
) -> ApiResult<Json<ResourceRead>> {
    let service = ToolService::shared();
    mcp_server_status(service, &server)?;
    let contents = service.mcp().read_resource(&server, &q.uri).await?;
    Ok(Json(ResourceRead { contents }))
}

/// Authorizes a remote MCP server (OAuth, MCP authorization spec): opens the system browser of
/// the machine Harness runs on and waits up to five minutes for the user to approve. The new
/// token is stored in the OS keychain and the server is connected with it. Needs `admin`.
#[utoipa::path(post, operation_id = "authorize_mcp_server", path = "/tools/mcp/{server}/authorize", tag = "tools",
    params(("server" = String, Path, description = "Server name in `mcp_servers.json`")),
    responses((status = 200, body = McpServerStatus), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn mcp_authorize(P(server): P<String>) -> ApiResult<Json<McpServerStatus>> {
    let service = ToolService::shared();
    mcp_server_status(service, &server)?;
    service
        .mcp()
        .authorize(
            &server,
            harness_core::tools::mcp_client::oauth::open_in_browser,
        )
        .await?;
    service.sync_mcp(true).await;
    Ok(Json(mcp_server_status(service, &server)?))
}

/// Forgets the OAuth sign-in of an MCP server and ends its connection. Needs `admin`.
#[utoipa::path(post, operation_id = "logout_mcp_server", path = "/tools/mcp/{server}/logout", tag = "tools",
    params(("server" = String, Path, description = "Server name in `mcp_servers.json`")),
    responses((status = 200, body = McpServerStatus), (status = 404, body = ErrorBody)))]
pub async fn mcp_logout(P(server): P<String>) -> ApiResult<Json<McpServerStatus>> {
    let service = ToolService::shared();
    mcp_server_status(service, &server)?;
    service.mcp().logout(&server);
    Ok(Json(mcp_server_status(service, &server)?))
}

/// The tool policy: `off`, `ask_every_time`, `allow_trusted` or `allow_all`; the trusted tools;
/// the default working directory.
#[utoipa::path(get, path = "/tools/policy", tag = "tools",
    responses((status = 200, body = ToolSettings)))]
pub async fn get_policy() -> ApiResult<Json<ToolSettings>> {
    let s = blocking(|| harness_core::config::AppSettings::load().tools).await?;
    Ok(Json(s))
}

/// Saves the tool policy (admin). A non-empty `workdir` must be an existing folder.
#[utoipa::path(put, path = "/tools/policy", tag = "tools", request_body = ToolSettings,
    responses((status = 200, body = ToolSettings), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn put_policy(Body(req): Body<ToolSettings>) -> ApiResult<Json<ToolSettings>> {
    let saved = blocking(move || tools::save_settings(req)).await??;
    Ok(Json(saved))
}

#[utoipa::path(get, path = "/tools/search-provider", tag = "tools",
    responses((status = 200, body = WebSearchSettingsView)))]
pub async fn get_search_provider() -> ApiResult<Json<WebSearchSettingsView>> {
    Ok(Json(blocking(tools::web_search_settings).await??))
}

#[utoipa::path(put, path = "/tools/search-provider", tag = "tools", request_body = SetWebSearchSettings,
    responses((status = 200, body = WebSearchSettingsView), (status = 400, body = ErrorBody)))]
pub async fn put_search_provider(
    Body(req): Body<SetWebSearchSettings>,
) -> ApiResult<Json<WebSearchSettingsView>> {
    Ok(Json(
        blocking(move || tools::save_web_search_settings(req)).await??,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RunToolRequest {
    /// The tool's arguments (its `input_schema`).
    #[serde(default = "empty_object")]
    #[schema(value_type = Object)]
    pub args: Value,
    /// Working directory of file tools and the shell; default: `workdir` of the policy
    /// settings. File tools cannot leave it.
    #[serde(default)]
    pub workdir: Option<String>,
    /// The caller asked the user and got a yes (see the module docs).
    #[serde(default)]
    pub approved: bool,
    /// Client label for the tool log (default: the token's name, or `api`).
    #[serde(default)]
    pub client: Option<String>,
    /// For the tool log.
    #[serde(default)]
    pub conversation: Option<String>,
    /// For the tool log: the model that wrote the call.
    #[serde(default)]
    pub model: Option<String>,
    /// Time limit in seconds (default 120, at most 600).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RunToolResponse {
    /// Id of the call in the tool log.
    pub id: String,
    pub tool: String,
    #[serde(flatten)]
    pub result: ToolResult,
}

fn label(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().chars().take(64).collect::<String>())
        .filter(|s| !s.is_empty())
}

/// Runs one tool call. The policy decides: `403 tools.approval_required` when it asks and the
/// body is not `approved`, `403 tools.disabled` when tools are off. Otherwise `200` with the
/// result; `error` is set when the tool ran but failed (see the module docs).
#[utoipa::path(post, path = "/tools/{name}/run", tag = "tools", request_body = RunToolRequest,
    params(("name" = String, Path, description = "Tool name (`GET /tools`)")),
    responses((status = 200, body = RunToolResponse), (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody)))]
pub async fn run_tool(
    P(name): P<String>,
    Extension(caller): Extension<Caller>,
    Body(req): Body<RunToolRequest>,
) -> ApiResult<Json<RunToolResponse>> {
    if (req.approved || req.workdir.as_deref().is_some_and(|w| !w.trim().is_empty()))
        && !caller.allows(Scope::Admin)
    {
        return Err(ApiError::forbidden_scope(Scope::Admin.as_str()));
    }
    let service = ToolService::shared();
    // Only an MCP tool (or a name not registered yet) needs its server: a built-in call must
    // not start the MCP servers (`harness mcp` runs built-ins through here; see `list_tools`).
    let builtin = service
        .registry()
        .get(&name)
        .is_some_and(|t| t.def().source != harness_core::tools::ToolSource::Mcp);
    if !builtin {
        service.sync_mcp(false).await;
    }
    let class = service
        .registry()
        .get(&name)
        .map(|t| t.def().class.as_str());
    let client = label(req.client).unwrap_or_else(|| match &caller.principal {
        Principal::Session => "api".to_string(),
        Principal::Named { name, .. } => format!("api:{name}"),
    });
    let mut origin = ToolOrigin::new(client);
    origin.conversation = label(req.conversation);
    origin.model = label(req.model);
    let inv = ToolInvocation::new(name.clone(), req.args, origin);
    let id = inv.id.clone();
    let opts = RunOptions {
        workdir: req
            .workdir
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty())
            .map(Into::into),
        approved: req.approved,
        timeout: req
            .timeout_secs
            .map(|s| Duration::from_secs(s.clamp(1, 600))),
        ..Default::default()
    };
    let result = service.run(inv, opts).await;
    match result.error {
        Some(ToolErrorCode::UnknownTool) => Err(ApiError::not_found("tool", &name)),
        Some(ToolErrorCode::PolicyOff) => {
            Err(ApiError::new(StatusCode::FORBIDDEN, "tools.disabled"))
        }
        Some(ToolErrorCode::ApprovalRequired) => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "tools.approval_required",
        )
        .with("tool", name)
        .with("class", class.unwrap_or("dangerous"))),
        _ => Ok(Json(RunToolResponse {
            id,
            tool: name,
            result,
        })),
    }
}

/// The tool log, newest first: every call from every client (also the refused ones), with the
/// arguments (long texts shortened), duration, outcome and network target. Needs `admin`.
#[utoipa::path(get, operation_id = "list_tool_calls", path = "/tools/log", tag = "tools", params(ToolCallQuery),
    responses((status = 200, body = Vec<ToolCallRow>)))]
pub async fn tool_log(Q(q): Q<ToolCallQuery>) -> ApiResult<Json<Vec<ToolCallRow>>> {
    let rows = blocking(move || -> Result<Vec<ToolCallRow>, String> {
        analytics::shared()?.tool_calls(&q)
    })
    .await?
    .map_err(ApiError::core)?;
    Ok(Json(rows))
}

#[derive(Deserialize, ToSchema)]
pub struct AutoStartRequest {
    #[schema(value_type = Object)]
    pub endpoint: harness_core::compaction::Endpoint,
    #[serde(default = "auto_count")]
    pub count: usize,
    #[serde(default)]
    pub resume: bool,
}
fn auto_count() -> usize {
    8
}
#[derive(Deserialize, ToSchema)]
pub struct AutoActionRequest {
    pub action: String,
    pub id: Option<String>,
    pub order: Option<Vec<String>>,
}
/// Every tool the automatic generator sees (built-in, installed, MCP), with its category.
#[utoipa::path(get, operation_id = "get_tool_inventory", path = "/tools/inventory", tag = "tools", responses((status = 200, body = Vec<harness_core::tools::automatic::InventoryTool>)))]
pub async fn inventory() -> ApiResult<Json<Vec<harness_core::tools::automatic::InventoryTool>>> {
    let service = ToolService::shared();
    service
        .sync_mcp_bounded(false, Duration::from_secs(8))
        .await;
    Ok(Json(tools::automatic::inventory(
        service,
        &tools::installed::tools_root(),
    )))
}
#[utoipa::path(get, operation_id = "get_tool_generation", path = "/tools/automatic", tag = "tools", responses((status = 200, description = "The automatic tool generator's run state (its queue, drafts, usage and log). The GUI's working format: not a stable contract", body = Object)))]
pub async fn auto_state() -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(tools::automatic::AutoGenerator::shared()?.snapshot())
            .map_err(|e| ApiError::core(e.to_string()))?,
    ))
}
#[utoipa::path(post, operation_id = "start_tool_generation", path = "/tools/automatic/start", tag = "tools", request_body = AutoStartRequest, responses((status = 200, description = "The automatic tool generator's run state (its queue, drafts, usage and log). The GUI's working format: not a stable contract", body = Object)))]
pub async fn auto_start(Body(req): Body<AutoStartRequest>) -> ApiResult<Json<Value>> {
    let s = tools::automatic::AutoGenerator::shared()?.launch(
        req.endpoint,
        req.count,
        req.resume,
        std::sync::Arc::new(|_| {}),
    )?;
    Ok(Json(
        serde_json::to_value(s).map_err(|e| ApiError::core(e.to_string()))?,
    ))
}
#[utoipa::path(post, operation_id = "tool_generation_action", path = "/tools/automatic/action", tag = "tools", request_body = AutoActionRequest, responses((status = 200, description = "The automatic tool generator's run state (its queue, drafts, usage and log). The GUI's working format: not a stable contract", body = Object)))]
pub async fn auto_action(Body(req): Body<AutoActionRequest>) -> ApiResult<Json<Value>> {
    let worker = tools::automatic::AutoGenerator::shared()?;
    let s = match req.action.as_str() {
        "pause" => worker.interrupt(false)?,
        "stop" => worker.interrupt(true)?,
        "reorder" => worker.reorder(
            &req.order
                .ok_or_else(|| ApiError::core("Missing queue order"))?,
        )?,
        "remove" => worker.remove(&req.id.ok_or_else(|| ApiError::core("Missing item id"))?)?,
        "retry" => worker.retry(&req.id.ok_or_else(|| ApiError::core("Missing item id"))?)?,
        "approve" => worker.approve(
            &req.id.ok_or_else(|| ApiError::core("Missing item id"))?,
            ToolService::shared(),
        )?,
        _ => return Err(ApiError::core("Unknown automatic tool action")),
    };
    Ok(Json(
        serde_json::to_value(s).map_err(|e| ApiError::core(e.to_string()))?,
    ))
}
