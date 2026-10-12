// Settings > OpenTelemetry: OTLP export of gateway traces and metrics (off by default).

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct OtelOverview {
    settings: harness_core::otel::OtelSettings,
    status: harness_core::otel::OtelStatus,
}

async fn otel_overview() -> Result<OtelOverview, String> {
    let settings = blocking(|| AppSettings::load().otel).await?;
    Ok(OtelOverview {
        settings,
        status: harness_core::otel::status(),
    })
}

#[tauri::command]
pub(crate) async fn get_otel_settings() -> Result<OtelOverview, String> {
    otel_overview().await
}

/// Validates and saves the export settings (`headers`: `None` keeps the saved ones, `""`
/// removes them; values go to the OS keychain), then starts, restarts or stops the exporter.
#[tauri::command]
pub(crate) async fn save_otel_settings(
    settings: harness_core::otel::OtelSettings,
    headers: Option<String>,
) -> Result<OtelOverview, String> {
    let saved = blocking(move || harness_core::otel::save_settings(&settings, headers.as_deref()))
        .await??;
    harness_core::otel::apply(&saved).await?;
    otel_overview().await
}

/// Sends one real test span with these (possibly unsaved) settings and reports the answer.
#[tauri::command]
pub(crate) async fn test_otel_export(
    settings: harness_core::otel::OtelSettings,
    headers: Option<String>,
) -> Result<harness_core::otel::TestExportResult, String> {
    harness_core::otel::test_export(&settings, headers.as_deref()).await
}
