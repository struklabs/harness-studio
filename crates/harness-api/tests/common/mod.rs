//! Shared setup for the Control API tests: a temporary data root (never the user's), an
//! in-memory secret store (never the user's keychain) and a real API listener on an ephemeral
//! loopback port.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use harness_api::{ApiContext, ApiServer};
use harness_core::benchmark::BenchmarkManager;
use harness_core::control_api::ControlApiSettings;
use harness_core::downloader::DownloadManager;
use harness_core::instance::InstanceKind;
use harness_core::operation_gate::OperationGate;
use harness_core::runtime::RuntimeManager;
use harness_core::servers::ServerRegistry;
use harness_core::telemetry::TelemetryPoller;

/// The process-wide data root (settings, definitions, profiles): a temp dir.
pub fn data_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("api-test"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        // A real llama-server when available (tests that start processes need it).
        if let Some(bin) = llama_server() {
            let mut settings = harness_core::config::AppSettings::load();
            settings.runtime_mode = harness_core::config::RuntimeMode::Custom;
            settings.custom_binary.binary_path = Some(bin);
            settings.custom_binary.is_valid = true;
            settings.save().unwrap();
        }
        dir
    })
}

pub fn llama_server() -> Option<PathBuf> {
    std::env::var_os("HS_LLAMA_SERVER")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

pub fn test_model() -> Option<PathBuf> {
    std::env::var_os("HS_TEST_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

pub struct Api {
    pub server: ApiServer,
    pub ctx: ApiContext,
    pub base: String,
    pub token: String,
    pub http: reqwest::Client,
    /// Per-test data dir (session token file, named tokens).
    pub dir: tempfile::TempDir,
}

pub fn context(dir: &Path) -> ApiContext {
    data_root();
    ApiContext {
        registry: Arc::new(ServerRegistry::default()),
        runtime: Arc::new(RuntimeManager::default()),
        downloads: Arc::new(DownloadManager::default()),
        benchmarks: Arc::new(BenchmarkManager::default()),
        gate: Arc::new(OperationGate::default()),
        telemetry: Arc::new(tokio::sync::Mutex::new(TelemetryPoller::new(None))),
        kind: InstanceKind::Headless,
        version: "test".into(),
        data_dir: dir.to_path_buf(),
    }
}

/// A loopback listener on an ephemeral port (≥ 1024) and its port. Hand it to
/// `ApiServer::start_on`: a port only looked up and bound later (`free_port`) can be taken by
/// a parallel test in between.
pub async fn listener() -> (tokio::net::TcpListener, u16) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

/// A free loopback port (≥ 1024; ephemeral ports always are) for settings that are never
/// bound (refused starts, server profiles that do not start).
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub fn settings(port: u16) -> ControlApiSettings {
    ControlApiSettings {
        enabled: true,
        port,
        ..Default::default()
    }
}

pub async fn start() -> Api {
    start_with(|_| {}).await
}

pub async fn start_with(configure: impl FnOnce(&mut ControlApiSettings)) -> Api {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let server = ApiServer::new();
    let (listener, port) = listener().await;
    let mut s = settings(port);
    configure(&mut s);
    let status = server.start_on(ctx.clone(), &s, listener).await.unwrap();
    let token = server.session_token().await.unwrap();
    Api {
        base: status.base_url.unwrap(),
        server,
        ctx,
        token,
        http: reqwest::Client::new(),
        dir,
    }
}

impl Api {
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.http
            .get(self.url(path))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
    }

    pub async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        token: &str,
        body: Option<serde_json::Value>,
    ) -> reqwest::Response {
        let mut req = self.http.request(method, self.url(path)).bearer_auth(token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        req.send().await.unwrap()
    }

    pub async fn post(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.send(reqwest::Method::POST, path, &self.token.clone(), Some(body))
            .await
    }
}

/// `(status, json)` of a response.
pub async fn json(resp: reqwest::Response) -> (u16, serde_json::Value) {
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    let value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    (status, value)
}

/// Reads SSE events (`(event, data)`) from a streaming response until `done` says stop or
/// the timeout passes.
pub async fn read_sse(
    resp: reqwest::Response,
    timeout: std::time::Duration,
    mut done: impl FnMut(&str, &serde_json::Value) -> bool,
) -> Vec<(String, serde_json::Value)> {
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let chunk = match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(c))) => c,
            _ => return out,
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            let mut event = String::from("message");
            let mut data = String::new();
            for line in block.lines() {
                if let Some(e) = line.strip_prefix("event:") {
                    event = e.trim().to_string();
                } else if let Some(d) = line.strip_prefix("data:") {
                    data.push_str(d.trim_start());
                }
            }
            if data.is_empty() {
                continue;
            }
            let value = serde_json::from_str(&data).unwrap_or(serde_json::Value::String(data));
            let stop = done(&event, &value);
            out.push((event, value));
            if stop {
                return out;
            }
        }
    }
}
