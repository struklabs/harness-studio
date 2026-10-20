// Tool engine for the Playground: thin adapters over
// `harness_core::tools::ToolService`, the same service the Control API and the CLI use. The
// policy, working directory, limits and the tool log all live in the core; the UI only shows
// the approval prompt when `tools_decision` says "ask" and passes `approved` for that one call.

/// Running Playground tool calls by the id the UI chose (the Stop button cancels by it).
pub(crate) type ToolRuns = Arc<crate::tools::RunTable>;

/// Pure recovery: the caller passes precisely the names offered on this model request.
#[tauri::command]
pub(crate) fn tools_recover_text_calls(content: String, names: Vec<String>) -> crate::tools::text_call::Recovery {
    crate::tools::text_call::recover_text_calls(&content, &names)
}

/// The tools, the policy they are judged by (each tool's `decision`), the saved default
/// working directory and the limits every surface shares.
#[tauri::command]
pub(crate) async fn tools_list() -> Result<crate::tools::ToolListing, String> {
    let service = crate::tools::ToolService::shared();
    // The first listing starts the MCP servers; a slow one (a first `npx` download) must not
    // hold the Playground: it shows up as `connecting` in `tools_mcp_status` and in the next
    // listing.
    service
        .sync_mcp_bounded(false, std::time::Duration::from_secs(8))
        .await;
    blocking(|| crate::tools::ToolService::shared().current_listing()).await
}

/// Harness's own MCP client: every configured server with its state, protocol version and tool
/// count. `retry` tries again the servers that failed to start.
#[tauri::command]
pub(crate) async fn tools_mcp_status(
    retry: bool,
) -> Result<Vec<crate::tools::McpServerStatus>, String> {
    let service = crate::tools::ToolService::shared();
    service
        .sync_mcp_bounded(retry, std::time::Duration::from_secs(8))
        .await;
    Ok(service.mcp().status())
}

/// The resources and resource templates of a connected MCP server.
#[tauri::command]
pub(crate) async fn tools_mcp_resources(
    server: String,
) -> Result<crate::tools::ResourceList, String> {
    let service = crate::tools::ToolService::shared();
    service
        .sync_mcp_bounded(false, std::time::Duration::from_secs(8))
        .await;
    service.mcp().resources(&server).await
}

/// Reads one resource of an MCP server and saves it under the app's data folder as a file the
/// document reader understands; the Playground attaches that file to the chat like any
/// document. Returns the file's path.
#[tauri::command]
pub(crate) async fn tools_mcp_resource_to_file(
    server: String,
    uri: String,
    name: Option<String>,
) -> Result<String, String> {
    let contents = crate::tools::ToolService::shared()
        .mcp()
        .read_resource(&server, &uri)
        .await?;
    let (file, bytes) =
        harness_core::tools::mcp_client::resource_as_file(&uri, name.as_deref(), &contents)?;
    blocking(move || {
        let folder: String = server
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let dir = harness_core::paths::data_root()
            .join("mcp-resources")
            .join(folder);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(file);
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await?
}

/// Signs a remote MCP server in: opens the system browser on its authorization page and waits
/// (loopback redirect on 127.0.0.1) until the user approves, declines, cancels
/// ([`tools_mcp_cancel_authorize`]) or five minutes pass.
#[tauri::command]
pub(crate) async fn tools_mcp_authorize(
    app: tauri::AppHandle,
    server: String,
) -> Result<Vec<crate::tools::McpServerStatus>, String> {
    use tauri_plugin_opener::OpenerExt;
    let service = crate::tools::ToolService::shared();
    service
        .mcp()
        .authorize(&server, |url| {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())
        })
        .await?;
    service
        .sync_mcp_bounded(true, std::time::Duration::from_secs(8))
        .await;
    Ok(service.mcp().status())
}

#[tauri::command]
pub(crate) fn tools_mcp_cancel_authorize(server: String) -> bool {
    harness_core::tools::mcp_client::oauth::cancel_login(&server)
}

/// Forgets the OAuth sign-in of an MCP server and ends its connection.
#[tauri::command]
pub(crate) async fn tools_mcp_logout(
    server: String,
) -> Result<Vec<crate::tools::McpServerStatus>, String> {
    let service = crate::tools::ToolService::shared();
    service.mcp().logout(&server);
    Ok(service.mcp().status())
}

/// Sets (`class`) or clears (`null`) the permission class of one MCP tool: the user's word over
/// what the server's annotations say.
#[tauri::command]
pub(crate) async fn tools_set_mcp_class(
    tool: String,
    class: Option<crate::tools::PermissionClass>,
) -> Result<crate::tools::ToolSettings, String> {
    blocking(move || crate::tools::set_mcp_class(&tool, class)).await?
}

/// What a call of `tool` would do right now: `run`, `ask` or `refuse` (`None`: no such tool).
/// Asked before each call so a policy changed in Settings applies at once.
#[tauri::command]
pub(crate) async fn tools_decision(tool: String) -> Result<Option<crate::tools::Decision>, String> {
    blocking(move || {
        crate::tools::ToolService::shared().decision(&tool, &AppSettings::load().tools)
    })
    .await
}

#[derive(serde::Deserialize)]
pub(crate) struct ToolRunRequest {
    /// Chosen by the UI, unique per run; `tools_cancel` cancels by it and it is the row id in
    /// the tool log (a model's own `tool_call_id` can repeat across conversations).
    pub call_id: String,
    pub tool: String,
    #[serde(default = "empty_args")]
    pub args: serde_json::Value,
    /// Absent/empty follows the live default. The reserved `harness://no-workspace`
    /// URI explicitly opts out; it is never passed to filesystem tools as a path.
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub conversation: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// The project of the chat: its folders bound the working directory, and files the call
    /// writes are recorded as the project's outputs.
    #[serde(default)]
    pub project: Option<String>,
    /// The user was asked (the decision was `ask`) and said yes. Counts for this call only and
    /// never overrides a policy of "off".
    #[serde(default)]
    pub approved: bool,
}

fn empty_args() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

#[derive(serde::Serialize)]
pub(crate) struct ToolRunResponse {
    pub id: String,
    #[serde(flatten)]
    pub result: crate::tools::ToolResult,
}

/// A tool-log label: trimmed and bounded, empty = none.
fn tool_label(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().chars().take(64).collect::<String>())
        .filter(|s| !s.is_empty())
}

const NO_WORKSPACE: &str = "harness://no-workspace";

fn chat_workspace_path(workdir: Option<&str>) -> Option<std::path::PathBuf> {
    workdir.map(str::trim)
        .filter(|w| !w.is_empty() && *w != NO_WORKSPACE)
        .map(Into::into)
}

/// Preserve legacy empty/omitted defaults; honor only the explicit reserved opt-out.
fn chat_workspace_settings(
    workdir: Option<&str>,
    mut settings: crate::tools::ToolSettings,
) -> Option<crate::tools::ToolSettings> {
    if workdir.is_some_and(|w| w.trim() == NO_WORKSPACE) {
        settings.workdir.clear();
        Some(settings)
    } else {
        None
    }
}

/// Runs one call under the policy. Every outcome is a result (`error` names the code);
/// `Err` only when the task could not run at all.
#[tauri::command]
pub(crate) async fn tools_run(
    req: ToolRunRequest,
    runs: State<'_, ToolRuns>,
    store: State<'_, ConversationStore>,
) -> Result<ToolRunResponse, String> {
    let project_id = req.project.clone().filter(|p| !p.trim().is_empty());
    let mut workdir = chat_workspace_path(req.workdir.as_deref());
    let db = match &project_id {
        Some(_) => Some(store.0.clone()?),
        None => None,
    };
    if let (Some(db), Some(project_id)) = (&db, &project_id) {
        // A project chat's tools run inside the project's folders (its folder by default).
        let (db, project_id, requested) = (db.clone(), project_id.clone(), workdir.clone());
        workdir = Some(
            blocking(move || {
                let project = db.project(&project_id).map_err(String::from)?;
                harness_core::projects::tool_workdir(&project, requested.as_deref())
            })
            .await??,
        );
    }
    let from = harness_core::projects::OutputSource {
        source: "tool".into(),
        model: req.model.clone(),
        conversation_id: req.conversation.clone(),
        agent_task_id: None,
    };
    let (tool, args) = (req.tool.clone(), req.args.clone());
    let mut origin = crate::tools::ToolOrigin::new("playground");
    origin.conversation = tool_label(req.conversation);
    origin.model = tool_label(req.model);
    let inv = crate::tools::ToolInvocation::new(req.tool, req.args, origin).with_id(req.call_id);
    let id = inv.id.clone();
    let opts = crate::tools::RunOptions {
        workdir: workdir.clone(),
        // Only the reserved URI opts out; old clients sending "" follow the default.
        settings: chat_workspace_settings(
            req.workdir.as_deref(),
            crate::config::AppSettings::load().tools,
        ),
        approved: req.approved,
        ..Default::default()
    };
    let result = crate::tools::ToolService::shared()
        .run_tracked(runs.inner(), inv, opts)
        .await;
    if let (Some(db), Some(project_id), Some(dir), false) =
        (db, project_id, workdir, result.is_error())
    {
        let _ = blocking(move || {
            harness_core::projects::record_tool_output(&db, &project_id, &tool, &args, &dir, &from)
        })
        .await;
    }
    Ok(ToolRunResponse { id, result })
}

/// The user was asked about a call and refused: only the tool log hears of it, nothing runs.
#[tauri::command]
pub(crate) async fn tools_deny(req: ToolRunRequest) -> Result<(), String> {
    let mut origin = crate::tools::ToolOrigin::new("playground");
    origin.conversation = tool_label(req.conversation);
    origin.model = tool_label(req.model);
    let inv = crate::tools::ToolInvocation::new(req.tool, req.args, origin).with_id(req.call_id);
    crate::tools::ToolService::shared().record_denied(inv).await;
    Ok(())
}

/// Cancels a running call (kills a running shell's process tree). `false`: it already ended.
#[tauri::command]
pub(crate) fn tools_cancel(call_id: String, runs: State<'_, ToolRuns>) -> bool {
    runs.cancel(&call_id)
}

#[tauri::command]
pub(crate) async fn tools_get_settings() -> Result<crate::tools::ToolSettings, String> {
    blocking(|| AppSettings::load().tools).await
}

/// Saves the policy, the trusted tools and the default working directory (the one place the
/// Settings screen changes them). Returns what was stored.
#[tauri::command]
pub(crate) async fn tools_set_settings(
    settings: crate::tools::ToolSettings,
) -> Result<crate::tools::ToolSettings, String> {
    blocking(move || crate::tools::save_settings(settings)).await?
}

#[tauri::command]
pub(crate) async fn tools_get_search_provider() -> Result<crate::tools::WebSearchSettingsView, String> {
    blocking(crate::tools::web_search_settings).await?
}

#[tauri::command]
pub(crate) async fn tools_set_search_provider(
    settings: crate::tools::SetWebSearchSettings,
) -> Result<crate::tools::WebSearchSettingsView, String> {
    blocking(move || crate::tools::save_web_search_settings(settings)).await?
}

/// The tool log, newest first (`analytics.db` `tool_calls`).
#[tauri::command]
pub(crate) async fn tools_log(
    query: crate::analytics::ToolCallQuery,
) -> Result<Vec<crate::analytics::ToolCallRow>, String> {
    blocking(move || crate::analytics::shared()?.tool_calls(&query)).await?
}

/// The installed tools (`<data dir>/tools/`) with their permissions and
/// whether they are switched on; a package that no longer validates carries its keyed error.
#[tauri::command]
pub(crate) async fn installed_tools_list() -> Result<Vec<crate::tools::installed::InstalledTool>, String> {
    blocking(|| crate::tools::installed::list(&crate::tools::installed::tools_root())).await
}

#[tauri::command]
pub(crate) async fn official_registry_list() -> Result<harness_core::tools::official_registry::Snapshot, String> {
    let mut snapshot = harness_core::tools::official_registry::fetch().await.map_err(registry_error)?;
    let platform = if cfg!(target_os = "macos") { "macos" } else { std::env::consts::OS };
    snapshot.index.entries = snapshot.index.compatible(platform, env!("CARGO_PKG_VERSION"));
    Ok(snapshot)
}

fn registry_error(detail: String) -> String {
    harness_core::error_key::err("tools.registry_failed").with("detail", detail).into()
}

pub(crate) type RegistryStaging = std::sync::Mutex<Option<tempfile::TempDir>>;

#[derive(serde::Serialize)]
pub(crate) struct RegistryPrepared {
    path: Option<String>,
    config: Option<harness_core::mcp::McpConfig>,
    skill: Option<String>,
}

#[tauri::command]
pub(crate) async fn official_registry_prepare(commit: String, id: String, staged: State<'_, RegistryStaging>) -> Result<RegistryPrepared, String> {
    use harness_core::tools::official_registry::{self, Kind};
    let (snapshot, entry) = official_registry::selected(&commit, &id).map_err(registry_error)?;
    match entry.kind {
        Kind::Tool => {
            let dir = official_registry::prepare_tool(&snapshot, &entry).await.map_err(registry_error)?;
            let path = dir.path().to_string_lossy().into_owned();
            // One review at a time; keep its staging folder alive until the next review.
            *staged.lock().map_err(|e| registry_error(e.to_string()))? = Some(dir);
            Ok(RegistryPrepared { path: Some(path), config: None, skill: None })
        }
        Kind::Mcp => Ok(RegistryPrepared { path: None, config: Some(official_registry::prepare_mcp(&snapshot, &entry).await.map_err(registry_error)?), skill: None }),
        Kind::Skill => Ok(RegistryPrepared { path: None, config: None, skill: Some(official_registry::prepare_skill(&snapshot, &entry).await.map_err(registry_error)?) }),
    }
}

#[tauri::command]
pub(crate) async fn skills_list() -> Result<Vec<harness_core::skills::Skill>, String> {
    blocking(|| harness_core::skills::list(&harness_core::skills::root())).await
}

#[tauri::command]
pub(crate) async fn skills_set_enabled(name: String, enabled: bool) -> Result<(), String> {
    blocking(move || harness_core::skills::set_enabled(&harness_core::skills::root(), &name, enabled).map_err(registry_error)).await?
}

#[tauri::command]
pub(crate) async fn skills_remove(name: String) -> Result<(), String> {
    blocking(move || harness_core::skills::remove(&harness_core::skills::root(), &name).map_err(registry_error)).await?
}

#[tauri::command]
pub(crate) async fn skills_folder() -> Result<String, String> {
    blocking(|| { let root = harness_core::skills::root(); std::fs::create_dir_all(&root).map_err(|e| registry_error(e.to_string()))?; Ok(root.to_string_lossy().into_owned()) }).await?
}

#[tauri::command]
pub(crate) async fn official_skill_install(commit: String, id: String) -> Result<harness_core::skills::Skill, String> {
    let (snapshot, entry) = harness_core::tools::official_registry::selected(&commit, &id).map_err(registry_error)?;
    let text = harness_core::tools::official_registry::prepare_skill(&snapshot, &entry).await.map_err(registry_error)?;
    blocking(move || harness_core::skills::install(&harness_core::skills::root(), &text).map_err(registry_error)).await?
}

/// Validates the package folder `path` and returns what it asks for, without installing it:
/// the install dialog shows this and the user confirms before `installed_tools_install`.
#[tauri::command]
pub(crate) async fn installed_tools_inspect(
    path: String,
) -> Result<crate::tools::installed::InstalledTool, String> {
    blocking(move || crate::tools::installed::inspect(std::path::Path::new(&path))).await?
}

/// Installs the package folder `path` (replacing an older version) and registers it.
#[tauri::command]
pub(crate) async fn installed_tools_install(
    path: String,
) -> Result<crate::tools::installed::InstalledTool, String> {
    blocking(move || {
        let service = crate::tools::ToolService::shared();
        let info = crate::tools::installed::install(
            &crate::tools::installed::tools_root(),
            std::path::Path::new(&path),
            service.registry(),
        )?;
        service.reload_installed();
        Ok(info)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn installed_tools_remove(name: String) -> Result<(), String> {
    blocking(move || {
        crate::tools::installed::remove(&crate::tools::installed::tools_root(), &name)?;
        crate::tools::ToolService::shared().reload_installed();
        Ok(())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn installed_tools_set_enabled(name: String, enabled: bool) -> Result<(), String> {
    blocking(move || {
        crate::tools::installed::set_enabled(&crate::tools::installed::tools_root(), &name, enabled)?;
        crate::tools::ToolService::shared().reload_installed();
        Ok(())
    })
    .await?
}

/// Runs the `test` file of an installed package in the sandbox (same permissions and limits as
/// the tool); the report lists every case.
#[tauri::command]
pub(crate) async fn installed_tools_test(name: String) -> Result<crate::tools::package_test::TestReport, String> {
    let root = crate::tools::installed::tools_root();
    if !crate::tools::package::valid_name(&name) || !root.join(&name).is_dir() {
        return Err(harness_core::error_key::err("tools.package.not_installed").with("name", &name).into());
    }
    crate::tools::package_test::run_package(&root.join(&name)).await
}

/// The generator's flag: one generation at a time, and the Cancel button reaches it.
fn generator_flag() -> &'static std::sync::Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> {
    static FLAG: std::sync::OnceLock<std::sync::Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>> =
        std::sync::OnceLock::new();
    FLAG.get_or_init(Default::default)
}

/// The user asked for a tool: the chat model writes manifest, code and
/// test, the sandbox runs the test, and the draft comes back for the user to read. Nothing is
/// registered yet (`generated_tool_approve`).
#[tauri::command]
pub(crate) async fn generated_tool_generate(
    request: crate::tools::generator::GenerateRequest,
) -> Result<crate::tools::generator::Draft, String> {
    generate_tool(request, None).await
}

#[tauri::command]
pub(crate) async fn generated_tool_generate_observed(
    request: crate::tools::generator::GenerateRequest,
    on_progress: tauri::ipc::Channel<String>,
) -> Result<crate::tools::generator::Draft, String> {
    generate_tool(request, Some(on_progress)).await
}

async fn generate_tool(
    request: crate::tools::generator::GenerateRequest,
    on_progress: Option<tauri::ipc::Channel<String>>,
) -> Result<crate::tools::generator::Draft, String> {
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut slot = generator_flag().lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            return Err(harness_core::error_key::err("tools.generator.busy").into());
        }
        *slot = Some(flag.clone());
    }
    let out = crate::tools::generator::generate_observed(
        &request, crate::tools::ToolService::shared().registry(), &flag, false, None,
        &|progress| {
            if let Some(channel) = &on_progress {
                let _ = channel.send(progress.to_string());
            }
        },
    ).await;
    *generator_flag().lock().unwrap_or_else(|e| e.into_inner()) = None;
    out
}

/// Stops the running generation; false when none runs.
#[tauri::command]
pub(crate) fn generated_tool_cancel() -> bool {
    match generator_flag().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(flag) => {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// The user read the draft and approved it: it joins the registry for this app session.
#[tauri::command]
pub(crate) async fn generated_tool_approve(id: String) -> Result<crate::tools::installed::InstalledTool, String> {
    blocking(move || crate::tools::generator::approve(&id, crate::tools::ToolService::shared().registry())).await?
}

#[tauri::command]
pub(crate) fn generated_tool_discard(id: String) {
    crate::tools::generator::discard(&id);
}

/// Persists a tool generated in this session to `<data dir>/tools/<name>/`.
#[tauri::command]
pub(crate) async fn generated_tool_save(name: String) -> Result<crate::tools::installed::InstalledTool, String> {
    blocking(move || {
        let service = crate::tools::ToolService::shared();
        let info = crate::tools::generator::save(&crate::tools::installed::tools_root(), &name, service.registry())?;
        service.reload_installed();
        Ok(info)
    })
    .await?
}

/// Takes a tool generated in this session out of the registry.
#[tauri::command]
pub(crate) async fn generated_tool_remove(name: String) -> bool {
    blocking(move || crate::tools::generator::remove(&name, crate::tools::ToolService::shared().registry()))
        .await
        .unwrap_or(false)
}

/// Parameters of a local model file, to warn before a small model writes a tool.
#[tauri::command]
pub(crate) async fn generated_tool_model_params(path: String) -> Option<u64> {
    blocking(move || crate::tools::generator::model_params(std::path::Path::new(&path)))
        .await
        .ok()
        .flatten()
}

/// The tools generated and approved in this app session, and the limit.
#[tauri::command]
pub(crate) async fn generated_tools_session() -> (Vec<String>, usize) {
    crate::tools::generator::session_tools(crate::tools::ToolService::shared().registry())
}

#[cfg(test)]
mod tools_command_tests {
    use super::*;

    #[test]
    fn legacy_empty_follows_live_default_and_explicit_choices_override_it() {
        for default in ["C:/default", "C:/changed"] {
            for (choice, expected) in [(None, Some(default)), (Some(""), Some(default)),
                (Some(NO_WORKSPACE), None), (Some(" C:/chat "), Some("C:/chat"))] {
                let settings = crate::tools::ToolSettings { workdir: default.into(), ..Default::default() };
                let settings = chat_workspace_settings(choice, settings.clone()).unwrap_or(settings);
                // Same resolution as ToolService::run: explicit path, then live settings.
                let resolved = chat_workspace_path(choice).or_else(||
                    (!settings.workdir.is_empty()).then(|| settings.workdir.into()));
                assert_eq!(resolved, expected.map(std::path::PathBuf::from));
            }
        }
    }

    #[test]
    fn chat_no_folder_disables_default_without_changing_policy() {
        let settings = crate::tools::ToolSettings {
            workdir: "C:/default".into(),
            policy: crate::tools::ToolPolicy::Off,
            workspaces: Some(vec!["C:/default".into()]),
            ..Default::default()
        };
        assert!(chat_workspace_settings(None, settings.clone()).is_none());
        assert!(chat_workspace_settings(Some("C:/chat"), settings.clone()).is_none());
        assert!(chat_workspace_settings(Some(""), settings.clone()).is_none());
        assert!(chat_workspace_settings(Some("  "), settings.clone()).is_none());
        let cleared = chat_workspace_settings(Some(NO_WORKSPACE), settings.clone()).unwrap();
        assert!(cleared.workdir.is_empty());
        assert_eq!(cleared.policy, settings.policy);
        assert_eq!(cleared.workspaces, settings.workspaces);
    }

    /// Only the call id and the tool are required; nothing is approved unless the UI says so.
    #[test]
    fn a_minimal_run_request_has_safe_defaults() {
        let r: ToolRunRequest =
            serde_json::from_str(r#"{"call_id":"x","tool":"read_file"}"#).unwrap();
        assert_eq!(r.call_id, "x");
        assert!(r.args.as_object().is_some_and(|o| o.is_empty()));
        assert!(!r.approved, "a call is never approved unless the UI says so");
        assert!(r.workdir.is_none() && r.conversation.is_none() && r.model.is_none());
    }

    #[test]
    fn labels_are_trimmed_bounded_and_never_empty() {
        assert_eq!(
            tool_label(Some("  chat-1 ".into())).as_deref(),
            Some("chat-1")
        );
        assert_eq!(tool_label(Some("   ".into())), None);
        assert_eq!(tool_label(None), None);
        assert_eq!(tool_label(Some("x".repeat(200))).unwrap().len(), 64);
    }

    /// The response the UI reads: the result's fields sit next to `id`.
    #[test]
    fn the_response_is_flat() {
        let resp = ToolRunResponse {
            id: "c1".into(),
            result: crate::tools::ToolResult::failure(
                crate::tools::ToolErrorCode::NoWorkspace,
                "no working directory",
                0,
            ),
        };
        let v = serde_json::to_value(resp).unwrap();
        assert_eq!(v["id"], "c1");
        assert_eq!(v["error"], "no_workspace");
        assert_eq!(v["content"], "no working directory");
        assert_eq!(v["truncated"], false);
    }

    /// An unknown tool has no decision; a dangerous one asks under the default policy.
    #[test]
    fn decisions_follow_the_core_table() {
        let s = crate::tools::ToolSettings::default();
        let service = crate::tools::ToolService::shared();
        assert!(service.decision("no_such_tool", &s).is_none());
        assert_eq!(
            service.decision("write_file", &s),
            Some(crate::tools::Decision::Ask)
        );
    }
}

#[tauri::command]
pub(crate) fn auto_tools_state() -> Result<crate::tools::automatic::RunState, String> {
    Ok(crate::tools::automatic::AutoGenerator::shared()?.snapshot())
}
#[tauri::command]
pub(crate) async fn tools_inventory() -> Result<Vec<crate::tools::automatic::InventoryTool>, String> {
    let service = crate::tools::ToolService::shared();
    service.sync_mcp_bounded(false, std::time::Duration::from_secs(8)).await;
    Ok(crate::tools::automatic::inventory(service, &crate::tools::installed::tools_root()))
}
#[tauri::command]
pub(crate) async fn auto_tools_start(app: tauri::AppHandle, endpoint: harness_core::compaction::Endpoint, count: usize, resume: bool) -> Result<crate::tools::automatic::RunState, String> {
    use tauri::Emitter;
    crate::tools::automatic::AutoGenerator::shared()?.launch(endpoint, count, resume, Arc::new(move |s| { let _ = app.emit("auto-tools-state", s); }))
}
#[tauri::command]
pub(crate) fn auto_tools_action(action: String, id: Option<String>, order: Option<Vec<String>>) -> Result<crate::tools::automatic::RunState, String> {
    let worker = crate::tools::automatic::AutoGenerator::shared()?;
    match action.as_str() {
        "clear_logs" => worker.clear_logs(),
        "pause" => worker.interrupt(false),
        "stop" => worker.interrupt(true),
        "reorder" => worker.reorder(&order.ok_or("Missing queue order")?),
        "remove" => worker.remove(&id.ok_or("Missing item id")?),
        "retry" => worker.retry(&id.ok_or("Missing item id")?),
        "retry_failed" => worker.retry_failed(),
        "approve" => worker.approve(&id.ok_or("Missing item id")?, crate::tools::ToolService::shared()),
        _ => Err("Unknown automatic tool action".into()),
    }
}

#[derive(serde::Serialize)]
pub(crate) struct InstalledToolFiles { manifest_json: String, code: String, test: String }
#[tauri::command]
pub(crate) async fn installed_tools_files(name: String) -> Result<InstalledToolFiles, String> {
    blocking(move || {
        if !crate::tools::package::valid_name(&name) { return Err("Invalid tool name".into()); }
        let pkg = crate::tools::package::load_dir(&crate::tools::installed::tools_root().join(name))?;
        Ok(InstalledToolFiles {
            manifest_json: serde_json::to_string_pretty(&pkg.manifest).map_err(|e|e.to_string())?,
            code: crate::tools::package::read_entry(&pkg)?,
            test: crate::tools::package::read_test(&pkg)?.unwrap_or_default(),
        })
    }).await?
}

#[tauri::command]
pub(crate) async fn generator_token_estimate(request: crate::tools::generator::GenerateRequest, count: usize, automatic: bool) -> Result<crate::tools::generator::TokenEstimate, String> {
    if !(1..=20).contains(&count) { return Err("Expected 1?20 tools".into()); }
    if automatic {
        crate::tools::automatic::AutoGenerator::shared()?.estimate(request.endpoint, count).await
    } else {
        crate::tools::generator::estimate(&request, 1, "", &[], &std::sync::atomic::AtomicBool::new(false)).await
    }
}
