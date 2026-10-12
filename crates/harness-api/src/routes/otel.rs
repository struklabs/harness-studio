//! OpenTelemetry (OTLP) export settings, status and "Test export". Reading needs `read`,
//! changing and testing `admin` (the test sends data to an address of the caller's choice).
//! Header values are never returned: they live in the OS keychain.

use axum::Json;
use harness_core::otel::{self, OtelSettings, OtelStatus, TestExportResult};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{blocking, ApiResult, Body, ErrorBody};

#[derive(Serialize, ToSchema)]
pub struct OtelView {
    pub settings: OtelSettings,
    pub status: OtelStatus,
}

async fn view() -> ApiResult<Json<OtelView>> {
    let settings = blocking(|| harness_core::config::AppSettings::load().otel).await?;
    Ok(Json(OtelView {
        settings,
        status: otel::status(),
    }))
}

/// Export settings (header names only) and exporter status.
#[utoipa::path(get, path = "/otel", tag = "system",
    responses((status = 200, body = OtelView), (status = 401, body = ErrorBody)))]
pub async fn get_otel() -> ApiResult<Json<OtelView>> {
    view().await
}

#[derive(Deserialize, ToSchema)]
pub struct SaveOtelRequest {
    pub settings: OtelSettings,
    /// Export headers, one `name=value` (or `name: value`) per line. Omitted/null = keep the
    /// saved ones, `""` = remove them.
    #[serde(default)]
    pub headers: Option<String>,
}

/// Saves the export settings and starts, restarts or stops the exporter.
#[utoipa::path(put, path = "/otel", tag = "system", request_body = SaveOtelRequest,
    responses((status = 200, body = OtelView), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn put_otel(Body(req): Body<SaveOtelRequest>) -> ApiResult<Json<OtelView>> {
    let saved =
        blocking(move || otel::save_settings(&req.settings, req.headers.as_deref())).await??;
    otel::apply(&saved).await?;
    view().await
}

#[derive(Deserialize, ToSchema)]
pub struct TestOtelRequest {
    /// Settings to test (may be unsaved); omitted = the saved ones.
    #[serde(default)]
    pub settings: Option<OtelSettings>,
    /// Headers to test with (unsaved); omitted = the saved ones.
    #[serde(default)]
    pub headers: Option<String>,
}

/// Sends one real test span to the endpoint and reports the collector's answer.
#[utoipa::path(post, path = "/otel/test", tag = "system", request_body = TestOtelRequest,
    responses((status = 200, body = TestExportResult), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
pub async fn test_otel(Body(req): Body<TestOtelRequest>) -> ApiResult<Json<TestExportResult>> {
    let settings = match req.settings {
        Some(s) => s,
        None => blocking(|| harness_core::config::AppSettings::load().otel).await?,
    };
    Ok(Json(
        otel::test_export(&settings, req.headers.as_deref()).await?,
    ))
}
