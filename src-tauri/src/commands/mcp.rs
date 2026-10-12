#[tauri::command]
pub(crate) fn get_mcp_config() -> Result<mcp::McpConfig, String> {
    mcp::load()
}

#[tauri::command]
pub(crate) fn save_mcp_config(config: mcp::McpConfig) -> Result<(), String> {
    mcp::save(&config)?;
    // Harness's own MCP client starts again from the saved list on its next use.
    crate::tools::ToolService::shared().mcp().reset();
    Ok(())
}

/// Reads MCP servers configured in Claude Desktop / Claude Code / Codex / Cursor / LM Studio
/// (not saved).
#[tauri::command]
pub(crate) fn read_mcp_import_source(source: String) -> Result<mcp::McpConfig, String> {
    mcp::read_import_source(&source)
}

#[tauri::command]
pub(crate) fn get_mcp_import_sources() -> Vec<(String, Option<String>, bool)> {
    ["claude", "claude-code", "codex", "cursor", "lmstudio"]
        .iter()
        .map(|s| {
            let path = mcp::import_source_path(s);
            let exists = path.as_ref().map(|p| p.is_file()).unwrap_or(false);
            (
                s.to_string(),
                path.map(|p| p.to_string_lossy().to_string()),
                exists,
            )
        })
        .collect()
}
