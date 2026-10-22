// Connections > Tools and clients: config integrations (preview / apply / rollback), the
// connection test and "Başlat" (launch a coding agent with process-only settings).

#[tauri::command]
pub(crate) async fn detect_installed_clients() -> Result<Vec<ClientStatus>, String> {
    blocking(detect_all_clients).await
}

#[tauri::command]
pub(crate) async fn list_integrations(
    base_url: String,
    model_id: String,
    context_size: u32,
) -> Result<Vec<IntegrationInfo>, String> {
    blocking(move || run_list_integrations(&base_url, &model_id, context_size)).await
}

#[allow(clippy::too_many_arguments)]
fn integration_request(
    base_url: String,
    api_key: Option<String>,
    model_id: String,
    model_name: String,
    context_size: u32,
    target_path: Option<String>,
    models: Option<Vec<crate::integrations::ProviderModel>>,
    set_default_model: Option<bool>,
) -> ApplyRequest {
    ApplyRequest {
        base_url,
        api_key,
        model_id,
        model_name,
        context_size,
        target_path,
        models: models.unwrap_or_default(),
        set_default_model,
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn preview_integration(
    id: String,
    base_url: String,
    api_key: Option<String>,
    model_id: String,
    model_name: String,
    context_size: u32,
    target_path: Option<String>,
    models: Option<Vec<crate::integrations::ProviderModel>>,
    set_default_model: Option<bool>,
) -> Result<Vec<FileChange>, String> {
    let request = integration_request(
        base_url,
        api_key,
        model_id,
        model_name,
        context_size,
        target_path,
        models,
        set_default_model,
    );
    blocking(move || run_preview_integration(&id, &request)).await?
}

/// Writes the tool's config (timestamped backup, ledger entry); returns the message, the files
/// written, the backups and warnings (file changed since Harness last wrote it...).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn apply_integration(
    id: String,
    base_url: String,
    api_key: Option<String>,
    model_id: String,
    model_name: String,
    context_size: u32,
    target_path: Option<String>,
    models: Option<Vec<crate::integrations::ProviderModel>>,
    set_default_model: Option<bool>,
) -> Result<crate::integrations::ApplyResult, String> {
    let request = integration_request(
        base_url,
        api_key,
        model_id,
        model_name,
        context_size,
        target_path,
        models,
        set_default_model,
    );
    blocking(move || run_registry_apply(&id, &request)).await?
}

/// Removes (or restores) only the keys Harness set; keys the user changed stay (warnings).
#[tauri::command]
pub(crate) async fn revert_integration(
    id: String,
) -> Result<crate::integrations::RevertOutcome, String> {
    blocking(move || run_revert_integration(&id)).await?
}

/// One real small request with a tool, in the API the integration speaks (`anthropic`,
/// `responses` or `chat`), against `base_url` (`http://host:port/v1`).
#[tauri::command]
pub(crate) async fn test_integration_connection(
    api: String,
    base_url: String,
    api_key: Option<String>,
    model: String,
) -> Result<crate::integrations::conntest::ConnectionTest, String> {
    use crate::integrations::launch::WireApi;
    let api = match api.as_str() {
        "anthropic" => WireApi::Anthropic,
        "responses" => WireApi::Responses,
        "chat" => WireApi::Chat,
        other => return Err(harness_core::error_key::err("conntest.unknown_api").with("api", other).into()),
    };
    Ok(crate::integrations::conntest::test_connection(
        api,
        &base_url,
        api_key.as_deref(),
        &model,
        "harness-studio-test",
    )
    .await)
}

/// Where each launchable tool's executable is (None = not on PATH; OpenCode also finds the CLI
/// a desktop build ships).
#[tauri::command]
pub(crate) async fn launch_tools() -> Result<Vec<(String, Option<String>)>, String> {
    use crate::integrations::launch::LaunchTool;
    blocking(|| {
        LaunchTool::ALL
            .iter()
            .map(|t| {
                (
                    t.id().to_string(),
                    t.find_executable()
                        .map(|p| p.to_string_lossy().to_string()),
                )
            })
            .collect()
    })
    .await
}

/// OpenCode desktop apps on this machine (known locations and the uninstall registry).
#[tauri::command]
pub(crate) async fn opencode_desktop_apps(
) -> Result<Vec<crate::integrations::opencode_desktop::DesktopApp>, String> {
    use crate::integrations::{opencode_desktop, Roots};
    blocking(|| opencode_desktop::find(&Roots::from_env())).await
}

/// Opens an OpenCode desktop app [`opencode_desktop_apps`] found (it reads the persistent
/// config Harness wrote). Returns the pid.
#[tauri::command]
pub(crate) async fn open_opencode_desktop(path: String, hide_external_skills: Option<bool>) -> Result<u32, String> {
    use crate::integrations::{opencode_desktop, Roots};
    blocking(move || opencode_desktop::open(&Roots::from_env(), Path::new(&path), hide_external_skills.unwrap_or(true))).await?
}

/// Chat models of the user's definitions with their tool-calling verdict and prompt speed.
#[tauri::command]
pub(crate) async fn list_agent_models(
) -> Result<Vec<crate::integrations::agent_models::AgentModel>, String> {
    blocking(crate::integrations::agent_models::list).await?
}

/// One real request with a tool to `alias` at `base_url` (`http://host:port/v1`); the result
/// is stored for the definition's current model file.
#[tauri::command]
pub(crate) async fn check_model_tool_calling(
    base_url: String,
    api_key: Option<String>,
    alias: String,
) -> Result<crate::integrations::agent_models::ToolCheck, String> {
    use crate::integrations::agent_models;
    let def = blocking({
        let alias = alias.clone();
        move || {
            harness_core::domain::store::Domain::global()
                .definitions()
                .ok()
                .and_then(|defs| {
                    defs.into_iter()
                        .find(|d| d.alias.eq_ignore_ascii_case(&alias))
                })
        }
    })
    .await?;
    agent_models::check(
        &base_url,
        api_key.as_deref().filter(|k| !k.trim().is_empty()),
        &alias,
        def.as_ref(),
        &agent_models::checks_file(),
    )
    .await
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct LaunchStarted {
    pid: u32,
    program: String,
    /// The plan with secrets redacted.
    plan: serde_json::Value,
}

/// "Başlat": starts the tool in a new terminal window with process-only settings (nothing in
/// its global config is written). The temp files are removed when the tool exits. Windows
/// (new console), Linux (a terminal emulator) and macOS (Terminal.app / iTerm2 through
/// `osascript`) run a private launcher script, see `integrations::terminal`; elsewhere, or
/// without a terminal, the UI offers the `harness launch` command.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) async fn launch_integration_tool(
    tool: String,
    alias: String,
    base_url: String,
    api_key: Option<String>,
    context_size: Option<u32>,
    managed: bool,
    cwd: Option<String>,
    hide_external_skills: Option<bool>,
) -> Result<LaunchStarted, String> {
    use crate::integrations::launch::{self, LaunchInput, LaunchTool};
    blocking(move || {
        let tool = LaunchTool::parse(&tool)
            .ok_or_else(|| harness_core::error_key::err("launch.unknown_tool").with("tool", &tool))?;
        if !cfg!(any(windows, target_os = "linux", target_os = "macos")) {
            return Err("launch.terminal_unsupported".to_string());
        }
        let program = tool
            .find_executable()
            .ok_or_else(|| format!("launch.not_installed: {}", tool.program()))?;
        let host = base_url.trim_end_matches('/');
        let host_url = host.strip_suffix("/v1").unwrap_or(host).to_string();
        let temp_dir = launch::new_temp_dir()?;
        let plan = launch::plan(&LaunchInput {
            tool,
            hide_external_skills: hide_external_skills.unwrap_or(true),
            host_url,
            alias,
            token: api_key.filter(|k| !k.trim().is_empty()),
            context: context_size.filter(|c| *c > 0),
            managed,
            extra_args: Vec::new(),
            temp_dir: temp_dir.clone(),
        });
        plan.write_files()?;
        let dir = cwd
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .or_else(|| std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from));
        // macOS: a Terminal.app / iTerm2 window through `osascript`. The launcher script is the
        // only thing in the AppleScript and removes the temp folder itself.
        #[cfg(target_os = "macos")]
        let pid = {
            use crate::integrations::terminal;
            let script = terminal::write_launcher(&plan, &program, dir.as_deref(), &temp_dir)
                .inspect_err(|_| {
                    let _ = std::fs::remove_dir_all(&temp_dir);
                })?;
            terminal::open_in_macos_terminal(&script).inspect_err(|_| {
                let _ = std::fs::remove_dir_all(&temp_dir);
            })?
        };
        #[cfg(not(target_os = "macos"))]
        let pid = {
        #[cfg(target_os = "linux")]
        let mut cmd = {
            use crate::integrations::terminal;
            let script = terminal::write_launcher(&plan, &program, dir.as_deref(), &temp_dir)
                .inspect_err(|_| {
                    let _ = std::fs::remove_dir_all(&temp_dir);
                })?;
            let term = terminal::pick_terminal(
                &script,
                std::env::var("TERMINAL").ok().as_deref(),
                crate::integrations::launch::find_program,
            )
            .ok_or_else(|| {
                let _ = std::fs::remove_dir_all(&temp_dir);
                "launch.terminal_unsupported".to_string()
            })?;
            let mut c = harness_core::process::hidden_command(&term.program);
            c.args(&term.args);
            c
        };
        #[cfg(windows)]
        let mut cmd = crate::integrations::terminal::windows_console_command(
            &plan, &program, dir.as_deref(), &temp_dir,
        ).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&temp_dir);
        })?;
        #[cfg(not(any(target_os = "linux", windows)))]
        let mut cmd = plan.command();
        if let Some(d) = dir {
            cmd.current_dir(d);
        }
        let mut child = cmd.spawn().map_err(|e| {
            let _ = std::fs::remove_dir_all(&temp_dir);
            String::from(
                harness_core::error_key::err("process.spawn_failed")
                    .with("program", program.display())
                    .with("error", e),
            )
        })?;
        let pid = child.id();
        // The settings file holds the token: remove it once the tool exits. On Linux the child
        // is the terminal, which may return at once (gnome-terminal, x-terminal-emulator) while
        // the tool still runs: the launcher script removes the folder itself.
        std::thread::spawn(move || {
            let _ = child.wait();
            if !cfg!(target_os = "linux") {
                let _ = std::fs::remove_dir_all(&temp_dir);
            }
        });
        pid
        };
        Ok(LaunchStarted {
            pid,
            program: program.to_string_lossy().to_string(),
            plan: plan.redacted(),
        })
    })
    .await?
}

/// Client configs Harness wrote whose model names no longer exist (renamed or deleted aliases).
/// `served` lists names a running server or the gateway answers to besides the definitions.
/// Read-only; the fix is the normal preview/apply of that integration.
#[tauri::command]
pub(crate) async fn check_integration_alias_drift(
    served: Vec<String>,
) -> Result<Vec<harness_core::integrations::drift::AliasDrift>, String> {
    blocking(move || harness_core::integrations::drift::detect_alias_drift(&served)).await
}

/// Client configs Harness wrote for another address than the gateway's current `base_url` (the
/// gateway moved to a fallback port, or the tool was set up for a server's own port).
/// Read-only; the fix is the normal preview/apply of that integration.
#[tauri::command]
pub(crate) async fn check_integration_endpoint_drift(
    base_url: String,
) -> Result<Vec<harness_core::integrations::drift::EndpointDrift>, String> {
    blocking(move || harness_core::integrations::drift::detect_endpoint_drift(&base_url)).await
}
