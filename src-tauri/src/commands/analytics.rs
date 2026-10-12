// Analytics screen and Request inspector: gateway request log in analytics.db.

#[tauri::command]
pub(crate) async fn get_analytics_summary(
    from_ms: Option<u64>,
    to_ms: Option<u64>,
) -> Result<crate::analytics::AnalyticsSummary, String> {
    blocking(move || crate::analytics::summary(from_ms, to_ms)).await?
}

#[tauri::command]
pub(crate) async fn list_analytics_requests(
    query: crate::analytics::RequestQuery,
) -> Result<crate::analytics::RequestPage, String> {
    blocking(move || crate::analytics::requests(&query)).await?
}

#[tauri::command]
pub(crate) async fn get_analytics_request(
    id: String,
) -> Result<Option<crate::analytics::RequestDetail>, String> {
    blocking(move || crate::analytics::request(&id)).await?
}

#[tauri::command]
pub(crate) async fn get_analytics_info() -> Result<crate::analytics::AnalyticsInfo, String> {
    blocking(crate::analytics::info).await
}

#[tauri::command]
pub(crate) async fn set_analytics_settings(
    settings: crate::analytics::AnalyticsSettings,
) -> Result<crate::analytics::AnalyticsInfo, String> {
    blocking(move || {
        crate::analytics::save_settings(settings)?;
        // A shorter retention applies at once.
        crate::analytics::maintain_now()?;
        Ok(crate::analytics::info())
    })
    .await?
}

/// Deletes every stored request, rollup and captured content.
#[tauri::command]
pub(crate) async fn clear_analytics() -> Result<crate::analytics::AnalyticsInfo, String> {
    blocking(|| {
        crate::analytics::clear()?;
        Ok(crate::analytics::info())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn record_direct_usage(record: crate::analytics::direct::DirectRecord) -> Result<(), String> {
    blocking(move || crate::analytics::direct::record_client(&record)).await?
}
