//! Harness Control API: a local HTTP management API over `harness-core`.
//!
//! A separate crate so `axum`/`tower` stay out of `harness-core` (the CLI's offline commands
//! and the core tests don't need an HTTP server). Every handler calls the same core services
//! as the GUI's Tauri commands; there is no API-only logic besides request/response shapes,
//! authentication and the event stream adapter.
//!
//! * Base path `/api/harness/v1`, default `127.0.0.1:11500`, off until the user enables it.
//! * Auth, scopes and Host check: [`auth`]. Errors: [`error`]. OpenAPI: `GET /openapi.json`.
//! * [`ApiServer`] starts/stops the listener; the owner of the data directory (see
//!   `harness_core::instance`) records its address and the session token file in
//!   `instance.json`.

pub mod auth;
pub mod browser;
pub mod error;
mod mcp;
mod routes;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use harness_core::benchmark::BenchmarkManager;
use harness_core::control_api::{self, ControlApiSettings, TokenStore};
use harness_core::downloader::DownloadManager;
use harness_core::instance::{ApiEndpoint, InstanceKind, InstanceLock};
use harness_core::operation_gate::OperationGate;
use harness_core::ports::PortState;
use harness_core::runtime::RuntimeManager;
use harness_core::servers::ServerRegistry;
use harness_core::telemetry::TelemetryPoller;
use serde::Serialize;
use tokio::sync::{oneshot, Mutex};

pub use routes::openapi_document;

/// The core services the API works on (shared with the GUI when it runs inside it).
#[derive(Clone)]
pub struct ApiContext {
    pub registry: Arc<ServerRegistry>,
    pub runtime: Arc<RuntimeManager>,
    pub downloads: Arc<DownloadManager>,
    pub benchmarks: Arc<BenchmarkManager>,
    pub gate: Arc<OperationGate>,
    pub telemetry: Arc<tokio::sync::Mutex<TelemetryPoller>>,
    /// Which program serves the API (`/status`).
    pub kind: InstanceKind,
    /// Version of that program.
    pub version: String,
    /// Data directory (session token file).
    pub data_dir: PathBuf,
}

/// Per-listener state handed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub browser: Option<Arc<browser::BrowserAuth>>,
    pub ctx: ApiContext,
    pub session_token: Arc<str>,
    pub tokens: TokenStore,
    pub addr: SocketAddr,
    pub loopback: bool,
    /// Set by a headless owner (`harness serve`): `POST /shutdown` notifies it.
    pub shutdown: Option<Arc<tokio::sync::Notify>>,
    /// Allowed browser origins (CORS; also the MCP endpoint's `Origin` check).
    pub cors_origins: Arc<[String]>,
}

/// What Settings shows about the API.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApiStatus {
    pub running: bool,
    /// `http://host:port/api/harness/v1` while running.
    pub base_url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub token_path: Option<PathBuf>,
    /// Why the last start failed (i18n key or text).
    pub error: Option<String>,
    /// The configured port was held by another program: the API runs on `port` instead
    /// (remembered in `config/ports.json`, recorded in `instance.json` for the CLI).
    #[serde(default)]
    pub port_fallback: Option<harness_core::ports::PortFallback>,
}

struct Running {
    tls: bool,
    addr: SocketAddr,
    session_token: String,
    token_path: PathBuf,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
    fallback: Option<harness_core::ports::PortFallback>,
}

/// Set by the app and `harness serve` only: the tools' MCP configs live in the user's real home
/// folder, so a Control API started by a test must never rewrite them.
static MCP_FOLLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Lets the Control API move the Harness MCP HTTP entries Harness wrote for an earlier port to
/// its current one (see [`follow_port`]). Off by default; only the real app and `harness serve`
/// turn it on.
pub fn enable_mcp_follow() {
    MCP_FOLLOW.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Moves the Harness MCP HTTP entries written for an earlier Control API port to `port` (in the
/// background; only when [`enable_mcp_follow`] was called and for the app's own data directory,
/// never a test's).
fn follow_port(data_dir: &std::path::Path, port: u16, history: Vec<u16>) {
    if !MCP_FOLLOW.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if harness_core::paths::normalize(data_dir)
        != harness_core::paths::normalize(&harness_core::paths::data_root())
    {
        return;
    }
    tokio::task::spawn_blocking(move || {
        let moved = harness_core::integrations::harness_mcp::follow_api_port(port, &history);
        if !moved.is_empty() {
            eprintln!(
                "[Harness Studio] Harness MCP ayarları yeni Control API portuna taşındı ({}): {}",
                port,
                moved.join(", ")
            );
        }
    });
}

/// Starts and stops the API listener. At most one listener at a time.
#[derive(Default)]
pub struct ApiServer {
    browser: std::sync::Mutex<Option<Arc<browser::BrowserAuth>>>,
    running: Mutex<Option<Running>>,
    last_error: std::sync::Mutex<Option<String>>,
    instance: std::sync::Mutex<Option<Arc<InstanceLock>>>,
    shutdown: std::sync::Mutex<Option<Arc<tokio::sync::Notify>>>,
    /// The port was given explicitly (`harness serve --port`): no fallback port when taken.
    strict_port: std::sync::atomic::AtomicBool,
}

fn base_url(addr: &SocketAddr, tls: bool) -> String {
    let scheme = if tls { "https" } else { "http" };
    format!("{scheme}://{addr}{}", control_api::BASE_PATH)
}

impl ApiServer {
    pub fn set_browser(&self, auth: Arc<browser::BrowserAuth>) {
        *self.browser.lock().unwrap() = Some(auth);
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// The instance lock whose `instance.json` records the API address (owner only).
    pub fn set_instance(&self, lock: Option<Arc<InstanceLock>>) {
        *self.instance.lock().unwrap() = lock;
    }

    /// Lets API clients ask the process to exit (`POST /shutdown`, admin). Only a headless
    /// owner sets it; without it the endpoint answers 409 `api.shutdown_unsupported` (the GUI
    /// is closed by its user). Takes effect on the next [`Self::start`].
    pub fn set_shutdown_hook(&self, hook: Option<Arc<tokio::sync::Notify>>) {
        *self.shutdown.lock().unwrap() = hook;
    }

    /// `true`: the configured port was given explicitly; when another program holds it the
    /// start fails (`api.bind_failed`, naming the program) instead of using a fallback port.
    pub fn set_strict_port(&self, strict: bool) {
        self.strict_port
            .store(strict, std::sync::atomic::Ordering::SeqCst);
    }

    /// (Re)starts the listener with `settings` (validated here; a network bind needs
    /// `allow_network` and a named token). A new session token is created on every start.
    pub async fn start(
        &self,
        ctx: ApiContext,
        settings: &ControlApiSettings,
    ) -> Result<ApiStatus, String> {
        self.stop().await;
        let result = self.start_inner(ctx, settings, None).await;
        *self.last_error.lock().unwrap() = result.as_ref().err().cloned();
        result
    }

    /// [`Self::start`] on a listener the caller already bound (`settings.host`/`port` are
    /// still validated). Tests use it: binding port 0 and handing the listener over leaves no
    /// gap in which another test could take the port.
    pub async fn start_on(
        &self,
        ctx: ApiContext,
        settings: &ControlApiSettings,
        listener: tokio::net::TcpListener,
    ) -> Result<ApiStatus, String> {
        self.stop().await;
        let result = self.start_inner(ctx, settings, Some(listener)).await;
        *self.last_error.lock().unwrap() = result.as_ref().err().cloned();
        result
    }

    async fn start_inner(
        &self,
        ctx: ApiContext,
        settings: &ControlApiSettings,
        bound: Option<tokio::net::TcpListener>,
    ) -> Result<ApiStatus, String> {
        let tokens = TokenStore::at(&ctx.data_dir.join("config"));
        let settings = control_api::validate_settings(settings, tokens.count())?;
        let (listener, fallback) = match bound {
            Some(listener) => (listener, None),
            None => {
                // Another program on the configured port: a fallback port (the last one
                // first), never the gateway's.
                let gw = harness_core::gateway::status();
                let mut avoid = vec![harness_core::config::AppSettings::load().gateway.port];
                avoid.extend(gw.port.filter(|_| gw.running));
                let avoid =
                    (!self.strict_port.load(std::sync::atomic::Ordering::SeqCst)).then_some(avoid);
                let remembered = PortState::load(&ctx.data_dir).control_api;
                harness_core::ports::bind_with_fallback(
                    &settings.host,
                    settings.port,
                    remembered,
                    avoid,
                )
                .await
                .map_err(|e| format!("api.bind_failed: {}", e))?
            }
        };
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let browser = self.browser.lock().unwrap().clone();
        let tls = browser.as_ref().is_some_and(|b| b.tls);
        if let Some(b) = &browser {
            if let Some(origin) = &b.config.public_origin {
                let normalized = harness_core::network::normalize_origin(origin)?;
                let scheme = if tls { "https://" } else { "http://" };
                if normalized != *origin || !origin.starts_with(scheme) {
                    return Err("browser.origin".into());
                }
            } else if !harness_core::network::is_loopback_host(&settings.host) {
                return Err("browser.public_origin_required".into());
            }
        }
        let tls_config = if let Some(b) = browser.as_ref().filter(|b| b.tls) {
            let _ = rustls::crypto::ring::default_provider().install_default();
            Some(
                axum_server::tls_rustls::RustlsConfig::from_pem_file(
                    b.config.tls_cert.as_ref().unwrap(),
                    b.config.tls_key.as_ref().unwrap(),
                )
                .await
                .map_err(|e| format!("browser.tls: {e}"))?,
            )
        } else {
            None
        };
        let session_token = control_api::new_session_token()?;
        let token_path = control_api::write_session_token(&ctx.data_dir, &session_token)?;
        if fallback.is_some() {
            PortState::update(&ctx.data_dir, |s| s.control_api = Some(addr.port()));
        }
        let history = PortState::update(&ctx.data_dir, |s| s.record_api_port(addr.port()))
            .control_api_history;
        follow_port(&ctx.data_dir, addr.port(), history);
        harness_core::ports::set_live_api_port(Some(addr.port()));
        let state = AppState {
            browser,
            session_token: session_token.clone().into(),
            tokens,
            addr,
            loopback: harness_core::network::is_loopback_host(&settings.host),
            shutdown: self.shutdown.lock().unwrap().clone(),
            cors_origins: settings.cors_origins.clone().into(),
            ctx,
        };
        let app = routes::router(state, &settings.cors_origins);
        let (tx, rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            if let Some(config) = tls_config {
                let handle = axum_server::Handle::new();
                let shutdown = handle.clone();
                let watcher = tokio::spawn(async move {
                    let _ = rx.await;
                    shutdown.graceful_shutdown(Some(std::time::Duration::from_secs(1)));
                });
                if let Ok(listener) = listener.into_std() {
                    if let Ok(server) = axum_server::from_tcp_rustls(listener, config) {
                        let _ = server.handle(handle).serve(app.into_make_service()).await;
                    }
                }
                watcher.abort();
            } else {
                let _ = axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = rx.await;
                    })
                    .await;
            }
        });
        if let Some(lock) = self.instance.lock().unwrap().as_ref() {
            let endpoint = ApiEndpoint {
                host: addr.ip().to_string(),
                port: addr.port(),
                tls,
            };
            if let Err(e) = lock.set_api(Some(endpoint), Some(token_path.clone())) {
                eprintln!("[Harness Studio] instance.json yazılamadı: {}", e);
            }
        }
        *self.running.lock().await = Some(Running {
            tls,
            addr,
            session_token,
            token_path,
            shutdown: tx,
            task,
            fallback,
        });
        Ok(self.status().await)
    }

    /// Stops the listener (open SSE streams end), removes the session token file and clears
    /// the address in `instance.json`.
    pub async fn stop(&self) {
        let Some(running) = self.running.lock().await.take() else {
            return;
        };
        if let Some(browser) = self.browser.lock().unwrap().as_ref() {
            browser.revoke_all();
        }
        let _ = running.shutdown.send(());
        // Long-lived SSE connections would keep a graceful shutdown waiting: give it a moment,
        // then abort.
        let mut task = running.task;
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
        let _ = std::fs::remove_file(&running.token_path);
        harness_core::ports::set_live_api_port(None);
        if let Some(lock) = self.instance.lock().unwrap().as_ref() {
            let _ = lock.set_api(None, None);
        }
    }

    pub async fn status(&self) -> ApiStatus {
        let error = self.last_error.lock().unwrap().clone();
        match self.running.lock().await.as_ref() {
            Some(r) => ApiStatus {
                running: true,
                base_url: Some(base_url(&r.addr, r.tls)),
                host: Some(r.addr.ip().to_string()),
                port: Some(r.addr.port()),
                token_path: Some(r.token_path.clone()),
                error: None,
                port_fallback: r.fallback.clone(),
            },
            None => ApiStatus {
                error,
                ..Default::default()
            },
        }
    }

    /// The current session token (GUI "copy token").
    pub async fn session_token(&self) -> Option<String> {
        self.running
            .lock()
            .await
            .as_ref()
            .map(|r| r.session_token.clone())
    }
}
