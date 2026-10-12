//! The gateway as seen from the Control API: status, settings and the scheduler's
//! models. The gateway itself serves inference on its own port (`harness-gateway`).

use axum::Json;
use harness_core::gateway::scheduler::ModelSlot;
use harness_core::gateway::{GatewaySettings, GatewayStatus};
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{ApiResult, ErrorBody};

#[derive(Serialize, ToSchema)]
pub struct GatewayView {
    pub status: GatewayStatus,
    pub settings: GatewaySettings,
    /// Definitions the scheduler knows (loaded, loading or used since the gateway started).
    pub models: Vec<ModelSlot>,
}

/// Gateway status, settings and the models it serves (JIT/TTL/queue state).
#[utoipa::path(get, path = "/gateway", tag = "system",
    responses((status = 200, body = GatewayView), (status = 401, body = ErrorBody)))]
pub async fn gateway() -> ApiResult<Json<GatewayView>> {
    let status = harness_core::gateway::status();
    let (settings, models) = match harness_core::gateway::scheduler() {
        Some(s) => (s.settings(), s.snapshot().await),
        None => (
            tokio::task::spawn_blocking(|| harness_core::config::AppSettings::load().gateway)
                .await
                .unwrap_or_default(),
            Vec::new(),
        ),
    };
    Ok(Json(GatewayView {
        status,
        settings,
        models,
    }))
}
