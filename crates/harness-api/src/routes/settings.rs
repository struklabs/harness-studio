//! App settings other clients may change (`harness_core::remote_settings`): an allow-list of
//! preferences, never secrets, bind addresses, the tool policy or folders. Reading needs `read`,
//! changing needs `admin`.

use axum::Json;
use harness_core::remote_settings::{self, RemoteSettings, RemoteSettingsPatch};

use crate::error::{blocking, ApiResult, Body, ErrorBody};

/// The allow-listed settings.
#[utoipa::path(get, operation_id = "get_settings", path = "/settings", tag = "system",
    responses((status = 200, body = RemoteSettings)))]
pub async fn get() -> ApiResult<Json<RemoteSettings>> {
    Ok(Json(
        blocking(|| remote_settings::read(&harness_core::config::AppSettings::load())).await?,
    ))
}

/// Changes the given fields (all or nothing). An open desktop window reloads its settings.
#[utoipa::path(patch, operation_id = "update_settings", path = "/settings", tag = "system",
    request_body = RemoteSettingsPatch,
    responses((status = 200, body = RemoteSettings),
        (status = 400, description = "A value outside its allowed set", body = ErrorBody),
        (status = 422, description = "A field outside the allow-list", body = ErrorBody)))]
pub async fn update(Body(patch): Body<RemoteSettingsPatch>) -> ApiResult<Json<RemoteSettings>> {
    Ok(Json(
        blocking(move || remote_settings::update(&patch)).await??,
    ))
}
