//! llama-bench runs (one at a time; history in `benchmarks.json`).

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use harness_core::benchmark::{self, BenchmarkRequest, BenchmarkRun, BenchmarkStatus};
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{blocking, ApiError, ApiResult, Body, ErrorBody, P};
use crate::AppState;

#[derive(Serialize, ToSchema)]
pub struct BenchmarkList {
    /// The current (or latest) run's live state.
    pub current: BenchmarkStatus,
    /// Saved runs, newest first.
    pub runs: Vec<BenchmarkRun>,
}

#[utoipa::path(get, path = "/benchmarks", tag = "benchmarks",
    responses((status = 200, body = BenchmarkList)))]
pub async fn list_benchmarks(State(s): State<AppState>) -> ApiResult<Json<BenchmarkList>> {
    let runs = blocking(benchmark::load_history).await?;
    Ok(Json(BenchmarkList {
        current: s.ctx.benchmarks.status(),
        runs,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct BenchmarkStarted {
    pub run_id: String,
}

/// Starts a llama-bench sweep with the `llama-bench` next to the configured llama-server.
/// Progress: `GET /benchmarks/{id}` or `benchmark_progress` / `benchmark_finished` events.
#[utoipa::path(post, path = "/benchmarks", tag = "benchmarks", request_body = BenchmarkRequest,
    responses((status = 202, body = BenchmarkStarted), (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn start_benchmark(
    State(s): State<AppState>,
    Body(request): Body<BenchmarkRequest>,
) -> ApiResult<(StatusCode, Json<BenchmarkStarted>)> {
    let run_id =
        harness_core::orchestrator::start_benchmark(&s.ctx.benchmarks, &s.ctx.runtime, request)
            .await?;
    Ok((StatusCode::ACCEPTED, Json(BenchmarkStarted { run_id })))
}

/// A run: live while it runs, from the history afterwards.
#[derive(Serialize, ToSchema)]
pub struct BenchmarkView {
    pub id: String,
    pub running: bool,
    /// Live state (the current run only).
    pub status: Option<BenchmarkStatus>,
    /// Saved result (finished runs).
    pub run: Option<BenchmarkRun>,
}

#[utoipa::path(get, path = "/benchmarks/{id}", tag = "benchmarks",
    params(("id" = String, Path)),
    responses((status = 200, body = BenchmarkView), (status = 404, body = ErrorBody)))]
pub async fn get_benchmark(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<BenchmarkView>> {
    let status = s.ctx.benchmarks.status();
    let current = status.run_id.as_deref() == Some(id.as_str());
    let saved = blocking({
        let id = id.clone();
        move || benchmark::load_history().into_iter().find(|r| r.id == id)
    })
    .await?;
    let run = saved.or_else(|| status.last_run.clone().filter(|r| current && r.id == id));
    if !current && run.is_none() {
        return Err(ApiError::not_found("benchmark", &id));
    }
    Ok(Json(BenchmarkView {
        running: current && status.running,
        status: current.then_some(status),
        run,
        id,
    }))
}

#[utoipa::path(post, path = "/benchmarks/{id}/cancel", tag = "benchmarks",
    params(("id" = String, Path)),
    responses((status = 202), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn cancel_benchmark(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<StatusCode> {
    let status = s.ctx.benchmarks.status();
    if status.run_id.as_deref() != Some(id.as_str()) {
        return Err(ApiError::not_found("benchmark", &id));
    }
    if !status.running {
        return Err(ApiError::conflict("benchmark.not_running"));
    }
    s.ctx.benchmarks.cancel();
    Ok(StatusCode::ACCEPTED)
}
