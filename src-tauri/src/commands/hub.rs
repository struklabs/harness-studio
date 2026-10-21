#[tauri::command]
pub(crate) async fn search_hf_models(
    query: Option<String>,
    options: Option<HfSearchOptions>,
) -> Result<Vec<HfModelSearchResult>, String> {
    let token = harness_core::hf_auth::current_token().await;
    let mut options = options.unwrap_or_default();
    if let Some(q) = query {
        options.query = q;
    }
    DownloadManager::search_models(&options, token.as_deref()).await
}

/// One page of a Hugging Face listing: an empty query lists by filters + sort alone, and
/// `next_cursor` (passed back as `options.cursor`) fetches the following page.
#[tauri::command]
pub(crate) async fn search_hf_models_page(options: HfSearchOptions) -> Result<HfSearchPage, String> {
    let token = harness_core::hf_auth::current_token().await;
    let mut page = DownloadManager::search_models_page(&options, token.as_deref()).await?;
    // Whether the runtime family a repo needs is installed is a local fact, so it is settled
    // here (not cached with the listing's own fields).
    let installed = blocking(harness_core::model_registry::installed_families).await?;
    for item in &mut page.items {
        item.resolve_installed(&installed);
    }
    Ok(page)
}

#[tauri::command]
pub(crate) async fn get_hf_model_card(repo_id: String) -> Result<HfModelCard, String> {
    let token = harness_core::hf_auth::current_token().await;
    DownloadManager::get_model_card(&repo_id, token.as_deref()).await
}

#[tauri::command]
pub(crate) async fn check_hf_access(repo_id: String) -> Result<HfAccess, String> {
    let token = harness_core::hf_auth::current_token().await;
    DownloadManager::check_access(&repo_id, token.as_deref()).await
}

#[tauri::command]
pub(crate) async fn get_hf_model_variants(repo_id: String) -> Result<Vec<HfModelVariant>, String> {
    let token = harness_core::hf_auth::current_token().await;
    DownloadManager::get_model_variants(&repo_id, token.as_deref()).await
}

/// Avatar URLs of Hugging Face publishers (cached; no token is sent). Unknown authors map to null.
#[tauri::command]
pub(crate) async fn get_hf_avatars(
    authors: Vec<String>,
) -> std::collections::HashMap<String, Option<String>> {
    crate::hf_avatar::get_avatars(authors).await
}
