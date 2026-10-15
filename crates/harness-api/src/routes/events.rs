//! `GET /events`: the core event bus as server-sent events.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use harness_core::events::{self, CoreEvent};
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventsQuery {
    /// Comma-separated event types to receive (`server_status,download,download_removed,
    /// benchmark_progress,benchmark_finished`); omitted = all.
    pub types: Option<String>,
}

/// Server status changes, download progress (at most ~2 per second per job) and benchmark
/// progress, as they happen. Each SSE `event:` is the type, `data:` the JSON event. A client
/// that falls behind gets an `event: lagged` with the number of skipped events.
#[utoipa::path(get, operation_id = "stream_events", path = "/events", tag = "events", params(EventsQuery),
    responses((status = 200, content_type = "text/event-stream", body = CoreEvent)))]
pub async fn events(
    crate::error::Q(q): crate::error::Q<EventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let filter: Option<Vec<String>> = q.types.map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });
    let rx = events::subscribe();
    let stream = futures_util::stream::unfold((rx, filter), |(mut rx, filter)| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if filter
                        .as_ref()
                        .is_some_and(|f| !f.iter().any(|t| t == event.kind()))
                    {
                        continue;
                    }
                    let data = serde_json::to_string(&event).unwrap_or_default();
                    let sse = Event::default().event(event.kind()).data(data);
                    return Some((Ok(sse), (rx, filter)));
                }
                Err(RecvError::Lagged(n)) => {
                    let sse = Event::default().event("lagged").data(n.to_string());
                    return Some((Ok(sse), (rx, filter)));
                }
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
