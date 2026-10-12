//! Request analytics: summaries, the request log and the unified Prometheus
//! exposition. Read-only; everything comes from `analytics.db` (gateway traffic only).

use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use harness_core::analytics::{
    self, AnalyticsSummary, QueueHint, RequestDetail, RequestPage, RequestQuery,
};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::{blocking, ApiError, ApiResult, ErrorBody, P, Q};

#[derive(Deserialize, IntoParams, Default)]
#[serde(default)]
pub struct SummaryQuery {
    /// `24h`, `7d`, `30d`, `90d` or `all` (default `7d`); ignored when `from_ms` is given.
    pub range: Option<String>,
    /// Start of the range, Unix ms.
    pub from_ms: Option<u64>,
    /// End of the range (exclusive), Unix ms; default now.
    pub to_ms: Option<u64>,
}

/// `range` → start of the range (None = everything).
pub fn range_start(range: Option<&str>, now_ms: u64) -> Result<Option<u64>, ApiError> {
    let day = analytics::DAY_MS;
    let span = match range.unwrap_or("7d") {
        "all" => return Ok(None),
        "24h" | "1d" => day,
        "7d" => 7 * day,
        "30d" => 30 * day,
        "90d" => 90 * day,
        other => {
            return Err(ApiError::bad_request("api.invalid_range").with("range", other));
        }
    };
    Ok(Some(now_ms.saturating_sub(span)))
}

/// Per-model statistics (runs, tokens, average t/s, P50/P95 TTFT and latency, VRAM,
/// error and OOM rates, busy time, last use), client distribution, daily series and the
/// latest benchmark of each model. Covers gateway traffic only.
#[utoipa::path(get, path = "/analytics/summary", tag = "analytics",
    params(SummaryQuery),
    responses((status = 200, body = AnalyticsSummary), (status = 400, body = ErrorBody)))]
pub async fn summary(Q(q): Q<SummaryQuery>) -> ApiResult<Json<AnalyticsSummary>> {
    let now = analytics::now_ms();
    let from = match q.from_ms {
        Some(f) => Some(f),
        None => range_start(q.range.as_deref(), now)?,
    };
    let to = q.to_ms;
    let s = blocking(move || analytics::summary(from, to))
        .await?
        .map_err(ApiError::core)?;
    Ok(Json(s))
}

/// Recent gateway requests, newest first (metadata only). Page with `before_ms`.
#[utoipa::path(get, path = "/analytics/requests", tag = "analytics",
    params(RequestQuery),
    responses((status = 200, body = RequestPage)))]
pub async fn requests(Q(q): Q<RequestQuery>) -> ApiResult<Json<RequestPage>> {
    let page = blocking(move || analytics::requests(&q))
        .await?
        .map_err(ApiError::core)?;
    Ok(Json(page))
}

/// One request: all recorded fields, its timeline (load → queue → prompt → generation) and
/// captured content when its definition captures content.
#[utoipa::path(get, path = "/analytics/requests/{id}", tag = "analytics",
    params(("id" = String, Path, description = "Gateway request id (`X-Harness-Request-Id`)")),
    responses((status = 200, body = RequestDetail), (status = 404, body = ErrorBody)))]
pub async fn request(P(id): P<String>) -> ApiResult<Json<RequestDetail>> {
    let key = id.clone();
    match blocking(move || analytics::request(&key))
        .await?
        .map_err(ApiError::core)?
    {
        Some(d) => Ok(Json(d)),
        None => Err(ApiError::not_found("request", &id)),
    }
}

/// Queue history of the last 7 days per model definition: how many requests waited for a
/// free slot or for room in the shared KV pool, and, where slots keep making requests wait,
/// `suggested_slots` for the next load (the model's "Concurrent requests" setting).
#[utoipa::path(get, path = "/analytics/queue-hints", tag = "analytics",
    responses((status = 200, body = Vec<QueueHint>)))]
pub async fn queue_hints() -> ApiResult<Json<Vec<QueueHint>>> {
    let hints = blocking(analytics::queue_hints)
        .await?
        .map_err(ApiError::core)?;
    Ok(Json(hints))
}

/// Unified Prometheus exposition: gateway counters and per-model histograms, scheduler
/// gauges, analytics recorder counters, system memory and every llama-server's own
/// `llamacpp:*` metrics (label `server`).
#[utoipa::path(get, path = "/metrics", tag = "analytics",
    responses((status = 200, content_type = "text/plain", body = String)))]
pub async fn metrics() -> Response {
    (
        [(header::CONTENT_TYPE, analytics::prometheus::CONTENT_TYPE)],
        analytics::prometheus::render().await,
    )
        .into_response()
}
