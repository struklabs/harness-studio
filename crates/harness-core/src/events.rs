//! Process-wide event bus: the services publish state changes here (server status, download
//! progress, benchmark progress) and any front end can subscribe (Control API `GET /events`,
//! future CLI `--follow`). Publishing never blocks and costs nothing without subscribers;
//! slow subscribers skip events (`RecvError::Lagged`) instead of holding anyone up.

use serde::Serialize;
use std::sync::OnceLock;
use tokio::sync::broadcast;

use crate::benchmark::BenchmarkRun;
use crate::downloader::DownloadProgress;
use crate::failure::ServerFailure;
use crate::process::{RestartState, ServerStatus};

/// Events kept for a subscriber that has not read them yet.
const CAPACITY: usize = 256;

/// One state change. Serialized with a `type` tag (`server_status`, `download`, ...).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent {
    /// Model definitions were successfully persisted by any client.
    DefinitionsChanged,
    /// The tool registry changed (generated, installed or MCP tools).
    ToolsChanged,
    /// A managed engine library was installed or removed (an ONNX Runtime build, also the
    /// automatic install of a first ONNX launch); install-state views reload on this.
    EnginesChanged,
    /// A server instance changed status (starting, running, stopped, error...).
    ServerStatus {
        server_id: String,
        status: ServerStatus,
    },
    /// A download job was queued, progressed (at most ~2/s per job) or changed status.
    Download { progress: DownloadProgress },
    /// A download job was removed from the queue.
    DownloadRemoved { id: u64 },
    /// A safetensors to GGUF conversion job was queued, changed stage or progressed.
    Convert {
        job: Box<crate::convert::ConvertJob>,
    },
    /// A conversion job was removed from the list.
    ConvertRemoved { id: u64 },
    /// A training job was created, changed status or progressed.
    Training {
        job: Box<crate::training::TrainingJob>,
    },
    /// A training job was deleted.
    TrainingRemoved { id: u64 },
    /// A llama-bench run progressed.
    BenchmarkProgress {
        run_id: String,
        current: u32,
        total: u32,
        phase: String,
    },
    /// A llama-bench run finished (completed, cancelled or failed).
    BenchmarkFinished { run: Box<BenchmarkRun> },
    /// A server run failed; the classified cause (`servers.out_of_memory`...).
    ServerFailed {
        server_id: String,
        failure: Box<ServerFailure>,
    },
    /// An automatic restart after a crash was scheduled (attempt k of N, in `delay_secs`).
    ServerRestart {
        server_id: String,
        restart: RestartState,
    },
    /// A request passed the gateway (metadata only, never content; analytics stores these).
    GatewayRequest {
        record: Box<crate::gateway::record::RequestRecord>,
    },
    /// The gateway scheduler loaded, unloaded or evicted a model.
    GatewayModel {
        definition_id: String,
        alias: String,
        server_id: String,
        /// `loaded`, `unloaded_ttl`, `evicted`, `unloaded`, `load_failed`.
        action: String,
        /// Why (the alias that needed the room, the failure code...).
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The Dashboard server's launch was saved as the last session (after it reached
    /// Running); views showing the saved launch reload on this.
    LastSessionSaved { server_id: String },
    /// Gateway request records were written to `analytics.db` (they wait
    /// [`crate::analytics::ENRICH_DELAY`] after `gateway_request`); views reading the request
    /// log reload on this, not on `gateway_request`.
    AnalyticsWritten {
        /// Records in the batch.
        count: usize,
    },
}

impl CoreEvent {
    /// SSE event name (`event:` field): the serialized `type`.
    pub fn kind(&self) -> &'static str {
        match self {
            CoreEvent::DefinitionsChanged => "definitions_changed",
            CoreEvent::ToolsChanged => "tools_changed",
            CoreEvent::EnginesChanged => "engines_changed",
            CoreEvent::ServerStatus { .. } => "server_status",
            CoreEvent::Download { .. } => "download",
            CoreEvent::DownloadRemoved { .. } => "download_removed",
            CoreEvent::Convert { .. } => "convert",
            CoreEvent::ConvertRemoved { .. } => "convert_removed",
            CoreEvent::Training { .. } => "training",
            CoreEvent::TrainingRemoved { .. } => "training_removed",
            CoreEvent::BenchmarkProgress { .. } => "benchmark_progress",
            CoreEvent::BenchmarkFinished { .. } => "benchmark_finished",
            CoreEvent::ServerFailed { .. } => "server_failed",
            CoreEvent::ServerRestart { .. } => "server_restart",
            CoreEvent::GatewayRequest { .. } => "gateway_request",
            CoreEvent::GatewayModel { .. } => "gateway_model",
            CoreEvent::LastSessionSaved { .. } => "last_session_saved",
            CoreEvent::AnalyticsWritten { .. } => "analytics_written",
        }
    }
}

fn bus() -> &'static broadcast::Sender<CoreEvent> {
    static BUS: OnceLock<broadcast::Sender<CoreEvent>> = OnceLock::new();
    BUS.get_or_init(|| broadcast::channel(CAPACITY).0)
}

/// Publishes an event (dropped when nobody listens).
pub fn publish(event: CoreEvent) {
    let _ = bus().send(event);
}

/// A new subscription; it sees events published from now on.
pub fn subscribe() -> broadcast::Receiver<CoreEvent> {
    bus().subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_receive_published_events_in_order() {
        let mut rx = subscribe();
        publish(CoreEvent::DownloadRemoved { id: 7 });
        publish(CoreEvent::ServerStatus {
            server_id: "ev-test".into(),
            status: ServerStatus::Running,
        });
        // Other tests may publish concurrently; look for ours in order.
        let mut seen = Vec::new();
        while seen.len() < 2 {
            match rx.recv().await.unwrap() {
                CoreEvent::DownloadRemoved { id: 7 } => seen.push("removed"),
                CoreEvent::ServerStatus { server_id, .. } if server_id == "ev-test" => {
                    seen.push("status")
                }
                _ => {}
            }
        }
        assert_eq!(seen, ["removed", "status"]);
        let json = serde_json::to_value(CoreEvent::DownloadRemoved { id: 1 }).unwrap();
        assert_eq!(json["type"], "download_removed");
        assert_eq!(
            CoreEvent::DownloadRemoved { id: 1 }.kind(),
            "download_removed"
        );
    }
}
