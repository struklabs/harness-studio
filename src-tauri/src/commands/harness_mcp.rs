// Connections > "Harness MCP'yi ekle": registers the Harness MCP server in Claude Code,
// Codex, OpenCode and Cursor. The core writes the files (preview / backup / marker removal).

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct HarnessMcpOverview {
    targets: Vec<crate::integrations::harness_mcp::McpTargetInfo>,
    /// `harness` CLI found next to the app or on PATH (stdio command).
    cli_path: Option<String>,
}

#[tauri::command]
pub(crate) async fn harness_mcp_overview() -> Result<HarnessMcpOverview, String> {
    blocking(|| HarnessMcpOverview {
        targets: crate::integrations::harness_mcp::list_targets(),
        cli_path: crate::integrations::harness_mcp::find_harness_cli()
            .map(|p| p.to_string_lossy().to_string()),
    })
    .await
}

/// `mode`: `stdio` (command = the harness CLI) or `http` (url + a named token).
fn harness_mcp_transport(
    mode: &str,
    command: Option<String>,
    allow_models_write: bool,
    url: Option<String>,
    token: Option<String>,
) -> Result<crate::integrations::harness_mcp::McpTransport, String> {
    use crate::integrations::harness_mcp::{stdio_transport, McpTransport};
    match mode {
        "stdio" => Ok(stdio_transport(
            command.as_deref().unwrap_or_default().trim(),
            allow_models_write,
            &paths::data_root(),
        )),
        "http" => Ok(McpTransport::Http {
            url: url.unwrap_or_default().trim().to_string(),
            token,
        }),
        other => Err(harness_core::error_key::err("harness_mcp.unknown_mode").with("mode", other).into()),
    }
}

#[tauri::command]
pub(crate) async fn preview_harness_mcp(
    id: String,
    mode: String,
    command: Option<String>,
    allow_models_write: bool,
    url: Option<String>,
    token: Option<String>,
) -> Result<Vec<FileChange>, String> {
    let transport = harness_mcp_transport(&mode, command, allow_models_write, url, token)?;
    blocking(move || crate::integrations::harness_mcp::preview(&id, &transport)).await?
}

#[tauri::command]
pub(crate) async fn apply_harness_mcp(
    id: String,
    mode: String,
    command: Option<String>,
    allow_models_write: bool,
    url: Option<String>,
    token: Option<String>,
) -> Result<String, String> {
    let transport = harness_mcp_transport(&mode, command, allow_models_write, url, token)?;
    blocking(move || crate::integrations::harness_mcp::apply(&id, &transport)).await?
}

#[tauri::command]
pub(crate) async fn remove_harness_mcp(id: String) -> Result<String, String> {
    blocking(move || crate::integrations::harness_mcp::remove(&id)).await?
}

/// Settings > "Terminalde kullan": where the bundled `harness` CLI is, its version and whether
/// it is on the user's PATH. `user_path` is a path the user typed (last lookup).
#[tauri::command]
pub(crate) async fn cli_status(
    user_path: Option<String>,
) -> Result<crate::integrations::cli_install::CliStatus, String> {
    blocking(move || {
        let loc = crate::integrations::cli_install::locate(user_path.as_deref());
        crate::integrations::cli_install::status(loc.as_ref())
    })
    .await
}

/// "PATH'e ekle": only ever called from an explicit click.
#[tauri::command]
pub(crate) async fn cli_path_install(user_path: Option<String>) -> Result<(), String> {
    blocking(move || {
        let loc = crate::integrations::cli_install::locate(user_path.as_deref())
            .ok_or_else(|| harness_core::error_key::err("cli_install.cli_not_found"))?;
        crate::integrations::cli_install::install(&loc.path)
    })
    .await?
}

/// "Kaldır": undoes [`cli_path_install`].
#[tauri::command]
pub(crate) async fn cli_path_uninstall(user_path: Option<String>) -> Result<(), String> {
    blocking(move || {
        let loc = crate::integrations::cli_install::locate(user_path.as_deref())
            .ok_or_else(|| harness_core::error_key::err("cli_install.cli_not_found"))?;
        crate::integrations::cli_install::uninstall(&loc.path)
    })
    .await?
}
