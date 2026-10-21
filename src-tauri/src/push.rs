//! Pushes backend state changes to the window instead of the UI polling for them: server list
//! (`servers-changed`, with the new list), new log lines (`server-logs`, ids of the servers),
//! download queue (`downloads-changed`), gateway models (`gateway-changed`), finished gateway
//! requests once written to analytics.db (`gateway-requests-changed`) and telemetry of
//! the server a view watches (`telemetry`). Every stream is coalesced, so a burst of changes
//! (a model loading writes hundreds of log lines) becomes a few events. With nothing
//! changing, nothing runs: the tasks sleep on their channels.

use harness_core::events::CoreEvent;
use harness_core::orchestrator::SamplerWatch;
use harness_core::servers::ServerRegistry;
use harness_core::telemetry::{material_change, TelemetryPoller, TelemetrySnapshot};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::{broadcast::error::RecvError, watch};

/// Server status changes come in bursts (starting -> running, failure + status).
const SERVERS_COALESCE: Duration = Duration::from_millis(60);
/// A loading model writes many lines; the log view refreshes at most ~4 times a second.
const LOGS_COALESCE: Duration = Duration::from_millis(250);
/// Download progress is published about twice a second per job.
const EVENTS_COALESCE: Duration = Duration::from_millis(250);

/// Starts the push tasks. They live as long as the app.
pub fn spawn(app: &AppHandle, registry: Arc<ServerRegistry>) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut rx = harness_core::changes::subscribe_servers();
        while rx.changed().await.is_ok() {
            tokio::time::sleep(SERVERS_COALESCE).await;
            rx.borrow_and_update();
            let _ = handle.emit("servers-changed", registry.summaries());
        }
    });

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut rx = harness_core::changes::subscribe_logs();
        while rx.changed().await.is_ok() {
            tokio::time::sleep(LOGS_COALESCE).await;
            rx.borrow_and_update();
            let ids = harness_core::changes::take_log_changes();
            if !ids.is_empty() {
                let _ = handle.emit("server-logs", ids);
            }
        }
    });

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut rx = harness_core::events::subscribe();
        loop {
            let mut changed = Changed::default();
            match rx.recv().await {
                Ok(e) => changed.classify(&e),
                Err(RecvError::Lagged(_)) => changed = Changed::all(),
                Err(RecvError::Closed) => return,
            }
            if !changed.any() {
                continue;
            }
            // Collect what else arrives in the window, then emit once per kind.
            let deadline = tokio::time::Instant::now() + EVENTS_COALESCE;
            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Ok(e)) => changed.classify(&e),
                    Ok(Err(RecvError::Lagged(_))) => changed = Changed::all(),
                    Ok(Err(RecvError::Closed)) => return,
                    Err(_) => break,
                }
            }
            if changed.downloads {
                let _ = handle.emit("downloads-changed", ());
            }
            if changed.converts {
                let _ = handle.emit("converts-changed", ());
            }
            if changed.training {
                let _ = handle.emit("training-changed", ());
            }
            if changed.tools {
                let _ = handle.emit("tools-changed", ());
            }
            if changed.engines {
                let _ = handle.emit("engines-changed", ());
            }
            if changed.definitions {
                let _ = handle.emit("definitions-changed", ());
            }
            if changed.gateway {
                let _ = handle.emit("gateway-changed", ());
            }
            if changed.requests {
                let _ = handle.emit("gateway-requests-changed", ());
            }
            if changed.last_session {
                let _ = handle.emit("last-session-changed", ());
            }
        }
    });
}

/// Which kinds of event arrived in one coalescing window.
#[derive(Default)]
struct Changed {
    tools: bool,
    /// A managed engine library was installed or removed.
    engines: bool,
    definitions: bool,
    downloads: bool,
    /// A safetensors to GGUF conversion job changed.
    converts: bool,
    /// A training job changed.
    training: bool,
    gateway: bool,
    /// A gateway request finished (the analytics views list them).
    requests: bool,
    /// The Dashboard server's launch was saved (the form's "saved" launch changed).
    last_session: bool,
}

impl Changed {
    fn all() -> Self {
        Self {
            tools: true,
            engines: true,
            definitions: true,
            downloads: true,
            converts: true,
            training: true,
            gateway: true,
            requests: true,
            last_session: true,
        }
    }

    fn any(&self) -> bool {
        self.tools
            || self.engines
            || self.definitions
            || self.downloads
            || self.converts
            || self.training
            || self.gateway
            || self.requests
            || self.last_session
    }

    fn classify(&mut self, e: &CoreEvent) {
        match e {
            CoreEvent::ToolsChanged => self.tools = true,
            CoreEvent::EnginesChanged => self.engines = true,
            CoreEvent::DefinitionsChanged => self.definitions = true,
            CoreEvent::Download { .. } | CoreEvent::DownloadRemoved { .. } => self.downloads = true,
            CoreEvent::Convert { .. } | CoreEvent::ConvertRemoved { .. } => self.converts = true,
            CoreEvent::Training { .. } | CoreEvent::TrainingRemoved { .. } => self.training = true,
            CoreEvent::GatewayModel { .. } => self.gateway = true,
            // The record lands in analytics.db only after ENRICH_DELAY: reload then.
            CoreEvent::AnalyticsWritten { .. } => self.requests = true,
            CoreEvent::LastSessionSaved { .. } => self.last_session = true,
            _ => {}
        }
    }
}

/// What the window's live views watch, set by the UI (`watch_telemetry`). Nothing is watched
/// while the window is hidden or minimized; the sampler then only runs for active servers.
pub struct TelemetryWatch {
    state: Mutex<WatchState>,
    /// What the sampler is asked for (see [`SamplerWatch`]).
    demand: watch::Sender<SamplerWatch>,
}

#[derive(Default)]
struct WatchState {
    /// The controlled server (title bar, Dashboard form).
    server_id: Option<String>,
    /// A view shows every running server (Dashboard telemetry cards): all of them are pushed.
    all: bool,
    /// Last snapshot sent per server (changes are measured against it).
    sent: HashMap<String, TelemetrySnapshot>,
    /// Order number of the UI call applied last. The UI can send two calls at once (a view
    /// asking for memory, then one asking for every server); they run concurrently here, so
    /// an older one finishing later must not undo the newer one.
    seq: u64,
}

#[derive(Serialize, Clone)]
struct TelemetryEvent<'a> {
    server_id: &'a str,
    snapshot: &'a TelemetrySnapshot,
    /// Milliseconds since the Unix epoch (the UI fills idle gaps of its sparkline with it).
    at_ms: u64,
}

impl Default for TelemetryWatch {
    fn default() -> Self {
        Self {
            state: Mutex::new(WatchState::default()),
            demand: watch::channel(SamplerWatch::default()).0,
        }
    }
}

impl TelemetryWatch {
    /// For [`harness_core::orchestrator::run_telemetry_sampler_watched`].
    pub fn demand(&self) -> watch::Receiver<SamplerWatch> {
        self.demand.subscribe()
    }

    /// Watches `server_id` (None = nothing); `memory` asks for system memory samples even
    /// while no server runs; `all` also pushes every running server. `current` is what the
    /// view shows now for `server_id`, `others` for the rest (changes are measured against it).
    /// `seq`: the UI's order number of this call; a call older than the one applied last is
    /// ignored (returns false). None always applies.
    pub fn set(
        &self,
        seq: Option<u64>,
        server_id: Option<String>,
        memory: bool,
        all: bool,
        current: Option<TelemetrySnapshot>,
        others: Vec<(String, TelemetrySnapshot)>,
    ) -> bool {
        let wants = SamplerWatch {
            live: server_id.is_some(),
            memory: server_id.is_some() && memory,
        };
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(seq) = seq {
            if seq < st.seq {
                return false;
            }
            st.seq = seq;
        }
        st.all = all && server_id.is_some();
        st.sent = others.into_iter().collect();
        if let (Some(id), Some(current)) = (&server_id, current) {
            st.sent.insert(id.clone(), current);
        }
        st.server_id = server_id;
        // Under the lock, so the sampler's demand follows the same order.
        self.demand
            .send_if_modified(|d| std::mem::replace(d, wants) != wants);
        true
    }

    /// After a sampler round: sends the telemetry of the watched server (and, when a view
    /// shows them all, of every running one) that changed.
    pub fn after_round(&self, app: &AppHandle, poller: &TelemetryPoller) {
        if !crate::desktop::window_visible() {
            return;
        }
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(selected) = st.server_id.clone() else {
            return;
        };
        let mut ids = vec![selected];
        if st.all {
            for id in poller.live_server_ids() {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        st.sent.retain(|id, _| ids.contains(id));
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        for id in ids {
            let next = poller.last(&id);
            if st
                .sent
                .get(&id)
                .is_some_and(|prev| !material_change(prev, &next))
            {
                continue;
            }
            let _ = app.emit(
                "telemetry",
                TelemetryEvent {
                    server_id: &id,
                    snapshot: &next,
                    at_ms,
                },
            );
            st.sent.insert(id, next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_events_and_lag_refresh_definitions() {
        let mut changed = Changed::default();
        assert!(!changed.any());
        changed.classify(&CoreEvent::DefinitionsChanged);
        assert!(changed.definitions);
        assert!(changed.any());
        assert!(!changed.gateway);
        assert!(Changed::all().definitions);
    }

    #[test]
    fn an_older_call_finishing_later_does_not_undo_the_newer_one() {
        let w = TelemetryWatch::default();
        let id = || Some("default".to_string());
        // The view's second call (every server) is applied first, its first call (memory
        // only) arrives afterwards: the cards must stay watched.
        assert!(w.set(Some(2), id(), true, true, None, Vec::new()));
        assert!(!w.set(Some(1), id(), true, false, None, Vec::new()));
        assert!(w.state.lock().unwrap().all);
        // A newer call still applies, and None (window closing) always does.
        assert!(w.set(Some(3), id(), true, false, None, Vec::new()));
        assert!(!w.state.lock().unwrap().all);
        assert!(w.set(None, None, false, false, None, Vec::new()));
        assert!(w.state.lock().unwrap().server_id.is_none());
    }
}
