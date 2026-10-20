#[tauri::command]
pub(crate) async fn github_discovery_search(query: String) -> Result<Vec<serde_json::Value>, String> {
    harness_core::github_discovery::search(&query).await
}
#[tauri::command]
pub(crate) async fn github_discovery_inspect(repo: String) -> Result<harness_core::github_discovery::Review, String> {
    harness_core::github_discovery::inspect(&repo).await
}
#[tauri::command]
pub(crate) async fn github_discovery_token(token: String) -> Result<(), String> {
    blocking(move || harness_core::github_discovery::token_set(&token)).await?
}
#[tauri::command]
pub(crate) async fn github_discovery_install(repo: String, sha: String, path: String, scripts: Vec<String>, confirmed: bool) -> Result<(), String> {
    harness_core::github_discovery::install(&repo, &sha, &path, &scripts, confirmed, &harness_core::skills::root()).await
}
