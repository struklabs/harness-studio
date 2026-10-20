//! Harness gateway: one OpenAI/Anthropic-compatible endpoint for every model
//! definition, default `127.0.0.1:11435`.
//!
//! Optional and in-process: the GUI or `harness serve` runs it ([`GatewayServer`]); there is
//! no daemon, and the direct llama-server ports keep working unchanged. Routing, loading and
//! limits are `harness_core::gateway::scheduler`; this crate is the HTTP side:
//!
//! * `GET /v1/models`, `GET /v1/models/{model}`
//! * `POST /v1/chat/completions`, `/v1/completions`, `/v1/responses`, `/v1/embeddings`,
//!   `/v1/rerank`, `/v1/messages`, `/v1/messages/count_tokens` ([`proxy`])
//! * Stateful Responses: `store` / `previous_response_id` on `POST /v1/responses`, and
//!   `GET`/`DELETE /v1/responses/{id}`, `GET /v1/responses/{id}/input_items` ([`responses`])
//! * Decisions: `POST /v1/systemone` (Ollama / LM Studio) and `/v1/decisions` (OpenAI / LM
//!   Studio) ([`decisions`])
//! * Ollama-compatible `/api/*` ([`ollama`]; also on the Ollama port 11434 when turned on)
//! * `GET /health` (no token)
//! * `GET /metrics`: unified Prometheus exposition (gateway counters and histograms per
//!   model, scheduler gauges, analytics recorder, system memory, every llama-server's own
//!   metrics); same token rule as `/v1/models`
//!
//! Auth: [`auth`]. Tool compatibility (Codex/Responses): [`compat`]. Errors: [`errors`].

pub mod audio;
pub mod audio_llm;
pub mod audio_process;
pub mod auth;
pub mod compat;
pub mod crispasr_tts;
pub mod decisions;
pub mod errors;
pub mod hf;
pub mod images;
pub mod llama_tts;
mod model_name;
pub mod ollama;
pub mod proxy;
pub mod pyhf;
mod reasoning;
pub mod responses;
mod server_tools;
pub mod sherpa_asr;
pub mod speech;
mod text_tool_calls;
mod tool_loop_guard;
pub mod transcribe_cpp;
pub mod videos;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use bytes::Bytes;
use harness_core::control_api::TokenStore;
use harness_core::gateway::limits::RateLimiter;
use harness_core::gateway::scheduler::{Scheduler, SchedulerContext};
use harness_core::gateway::{self, GatewaySettings, GatewayStatus};
use harness_core::operation_gate::OperationGate;
use harness_core::ports::PortState;
use harness_core::runtime::RuntimeManager;
use harness_core::servers::ServerRegistry;
use tokio::sync::{oneshot, Mutex};
use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};

use compat::Api;

/// Largest request body (images as base64 can be large).
pub const BODY_LIMIT: usize = 64 * 1024 * 1024;

/// Origins of the Harness window (Tauri on Windows / macOS / Linux, and the dev server).
pub use harness_core::network::APP_ORIGINS;

/// The core services the gateway works on (same instances as the GUI's / `harness serve`).
#[derive(Clone)]
pub struct GatewayContext {
    pub registry: Arc<ServerRegistry>,
    pub runtime: Arc<RuntimeManager>,
    pub gate: Arc<OperationGate>,
    /// Data directory (client token file, named tokens in `config/`).
    pub data_dir: PathBuf,
    /// Download queue for the Ollama `/api/pull` (Hugging Face references only; `None` =
    /// pulling is unavailable).
    pub downloads: Option<Arc<harness_core::downloader::DownloadManager>>,
}

/// Per-listener state of the handlers.
#[derive(Clone)]
pub struct GwState {
    pub scheduler: Arc<Scheduler>,
    pub client_token: Arc<str>,
    pub tokens: TokenStore,
    pub addr: SocketAddr,
    pub loopback: bool,
    pub http: reqwest::Client,
    /// Download queue of `/api/pull`.
    pub downloads: Option<Arc<harness_core::downloader::DownloadManager>>,
    /// Per-token rate-limit counters (shared by the gateway and the Ollama port listener).
    pub limiter: RateLimiter,
    /// Video jobs (`/v1/videos`).
    pub videos: Arc<videos::VideoStore>,
}

struct Running {
    settings: GatewaySettings,
    client_token: String,
    scheduler: Arc<Scheduler>,
    data_dir: PathBuf,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
    maintenance: tokio::task::JoinHandle<()>,
    links: tokio::task::JoinHandle<()>,
    /// The listener on the Ollama port, when bound.
    ollama: Option<(oneshot::Sender<()>, tokio::task::JoinHandle<()>)>,
}

/// Set by the app and `harness serve` only: client configs live in the user's real home
/// folder, so a gateway started by a test (on a random port) must never rewrite them.
static CLIENT_FOLLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Lets the gateway move the client configs Harness wrote to its current port (see
/// [`follow_port`]). Off by default; only the real app and `harness serve` turn it on.
pub fn enable_client_follow() {
    CLIENT_FOLLOW.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Moves the client configs Harness wrote for an earlier gateway port to the current one (in
/// the background; only when [`enable_client_follow`] was called and for the app's own data
/// directory, never a test's).
fn follow_port(data_dir: &std::path::Path, history: Vec<u16>) {
    if !CLIENT_FOLLOW.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if harness_core::paths::normalize(data_dir)
        != harness_core::paths::normalize(&harness_core::paths::data_root())
    {
        return;
    }
    if gateway::status().endpoint.is_none() {
        return;
    }
    tokio::task::spawn_blocking(move || {
        // One run at a time, each with the address the gateway has when it runs: a run queued
        // for an earlier port must not write that port back after a later run.
        static FOLLOW: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = FOLLOW.lock().unwrap_or_else(|e| e.into_inner());
        let Some(base) = gateway::status().endpoint.map(|e| e.base_url) else {
            return;
        };
        let done = harness_core::integrations::drift::follow_gateway_port(&base, &history);
        let names = |ids: &[String]| -> Vec<String> {
            ids.iter()
                .map(|id| harness_core::integrations::drift::client_name(id))
                .collect()
        };
        let (moved, pending) = (names(&done.moved), names(&done.pending));
        if !moved.is_empty() {
            eprintln!(
                "[Harness Gateway] Client settings moved to the new address ({}): {}",
                base,
                moved.join(", ")
            );
        }
        if !pending.is_empty() {
            eprintln!(
                "[Harness Gateway] Client settings could not be moved automatically (Connections > Fix): {}",
                pending.join(", ")
            );
        }
        gateway::set_client_follow(moved, pending);
    });
}

/// Starts and stops the gateway listener (at most one).
#[derive(Default)]
pub struct GatewayServer {
    running: Mutex<Option<Running>>,
    /// The port was given explicitly (`harness serve --gateway-port`): no fallback port.
    strict_port: std::sync::atomic::AtomicBool,
}

fn upstream_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        // No pooled connections. llama-server closes the connection after a streamed answer
        // while its headers still say keep-alive (`timeout=5, max=100`); a request sent on it
        // right after the stream ends fails with "error sending request" (502
        // `upstream_unreachable`). On a one-slot server that is the queued request released
        // by that very stream: one of four parallel streams failed every few rounds. Upstreams
        // are on loopback, so a new connection per request costs next to nothing.
        .pool_max_idle_per_host(0)
        // Loopback upstreams only: never through a system proxy.
        .no_proxy()
        .build()
        .unwrap_or_default()
}

impl GatewayServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true`: the configured port was given explicitly; when another program holds it the
    /// start fails (`gateway.bind_failed`, naming the program) instead of using a fallback.
    pub fn set_strict_port(&self, strict: bool) {
        self.strict_port
            .store(strict, std::sync::atomic::Ordering::SeqCst);
    }

    /// (Re)starts the listener with `settings` (validated here; a network bind needs
    /// `allow_network` and a named token with the `inference` scope). Limits-only changes of a
    /// running gateway apply without a restart (streams keep going).
    pub async fn start(
        &self,
        ctx: GatewayContext,
        settings: &GatewaySettings,
    ) -> Result<GatewayStatus, String> {
        self.start_bound(ctx, settings, None).await
    }

    /// Starts on an OS-assigned loopback port, retaining the listener through startup.
    /// This explicit entry point requires `port: 0`; user settings validation and
    /// [`Self::start`] continue to reject port 0.
    pub async fn start_ephemeral(
        &self,
        ctx: GatewayContext,
        settings: &GatewaySettings,
    ) -> Result<GatewayStatus, String> {
        if settings.port != 0 || !settings.is_loopback() {
            return Err(gateway::ERR_INVALID_PORT.to_string());
        }
        let host = harness_core::network::normalize_bind_host(&settings.host)?;
        let control_port = harness_core::config::AppSettings::load().control_api.port;
        // The unbound control/Ollama settings may happen to name an ephemeral port.
        for _ in 0..5 {
            let listener = tokio::net::TcpListener::bind((host.as_str(), 0))
                .await
                .map_err(|e| format!("gateway.bind_failed: {e}"))?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            if port < 1024
                || [11435, 11440, 11500].contains(&port)
                || port == control_port
                || (settings.ollama_port_enabled && port == settings.ollama_port)
            {
                continue;
            }
            let mut settings = settings.clone();
            settings.port = port;
            return self.start_bound(ctx, &settings, Some(listener)).await;
        }
        Err(gateway::ERR_PORT_CONFLICT.to_string())
    }

    async fn start_bound(
        &self,
        ctx: GatewayContext,
        settings: &GatewaySettings,
        bound: Option<tokio::net::TcpListener>,
    ) -> Result<GatewayStatus, String> {
        let tokens = TokenStore::at(&ctx.data_dir.join("config"));
        let control_port = Some(harness_core::config::AppSettings::load().control_api.port);
        let settings = gateway::validate_settings(
            settings,
            gateway::inference_token_count(&tokens),
            control_port,
        )?;
        {
            let running = self.running.lock().await;
            if let Some(r) = running.as_ref() {
                let same_listener = r.settings.host == settings.host
                    && r.settings.port == settings.port
                    && r.settings.cors_origins == settings.cors_origins
                    && r.settings.ollama_port_enabled == settings.ollama_port_enabled
                    && r.settings.ollama_port == settings.ollama_port;
                if same_listener && bound.is_none() {
                    r.scheduler.set_settings(settings.clone());
                    drop(running);
                    self.update_running_settings(&settings).await;
                    return Ok(gateway::status());
                }
            }
        }
        self.stop().await;
        let result = self.start_inner(ctx, settings, tokens, bound).await;
        if let Err(e) = &result {
            gateway::set_stopped(Some(e.clone()));
        }
        result
    }

    async fn update_running_settings(&self, settings: &GatewaySettings) {
        if let Some(r) = self.running.lock().await.as_mut() {
            r.settings = settings.clone();
            if let (Some(host), Some(port)) = (gateway::status().host, gateway::status().port) {
                gateway::set_running(&host, port, settings.token_required(), r.scheduler.clone());
            }
        }
    }

    async fn start_inner(
        &self,
        ctx: GatewayContext,
        settings: GatewaySettings,
        tokens: TokenStore,
        bound: Option<tokio::net::TcpListener>,
    ) -> Result<GatewayStatus, String> {
        // Another program on the configured port: a fallback port (the last one first), never
        // the Control API's or the Ollama port.
        let mut avoid = vec![
            harness_core::config::AppSettings::load().control_api.port,
            settings.ollama_port,
        ];
        avoid.extend(harness_core::ports::live_api_port());
        let avoid = (!self.strict_port.load(std::sync::atomic::Ordering::SeqCst)).then_some(avoid);
        let remembered = PortState::load(&ctx.data_dir).gateway;
        let (listener, fallback) = match bound {
            Some(listener) => (listener, None),
            None => harness_core::ports::bind_with_fallback(
                &settings.host,
                settings.port,
                remembered,
                avoid,
            )
            .await
            .map_err(|e| format!("gateway.bind_failed: {}", e))?,
        };
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let ports = PortState::update(&ctx.data_dir, |s| {
            // On the configured port again: the next start does not look for another one.
            s.gateway = fallback.is_some().then(|| addr.port());
            s.record_gateway_port(addr.port());
        });
        if let Some(fb) = &fallback {
            eprintln!(
                "[Harness Gateway] Port {} is in use ({}); gateway is running on port {}.",
                fb.configured,
                fb.holder
                    .as_ref()
                    .map(|h| h.label())
                    .unwrap_or_else(|| "?".into()),
                addr.port()
            );
        }
        let client_token = gateway::new_client_token()?;
        gateway::write_client_token(&ctx.data_dir, &client_token)?;
        let scheduler = Scheduler::new(
            SchedulerContext {
                registry: ctx.registry.clone(),
                runtime: ctx.runtime.clone(),
                gate: ctx.gate.clone(),
            },
            settings.clone(),
        );
        let state = GwState {
            scheduler: scheduler.clone(),
            client_token: client_token.clone().into(),
            tokens,
            addr,
            loopback: harness_core::network::is_loopback_host(&settings.host),
            http: upstream_client(),
            downloads: ctx.downloads.clone(),
            videos: Arc::new(videos::VideoStore::open(ctx.data_dir.join("videos"))),
            // Budgets survive a restart (and are shared with other Harness processes on this
            // data directory); without a readable analytics.db they are kept in memory.
            limiter: match harness_core::analytics::shared() {
                Ok(db) => RateLimiter::persistent(db),
                Err(e) => {
                    eprintln!("[Harness Limits] {e}");
                    RateLimiter::new()
                }
            },
        };
        let ollama = if settings.ollama_port_enabled {
            ollama_listener(&state, &settings).await
        } else {
            Ok(None)
        };
        let app = router(state, &settings.cors_origins);
        let (tx, rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        let maintenance = tokio::spawn(scheduler.clone().maintenance_loop());
        let links = tokio::spawn(harness_core::link::refresh_loop());
        tokio::spawn(scheduler.clone().warm_pinned());
        gateway::set_running(
            &addr.ip().to_string(),
            addr.port(),
            settings.token_required(),
            scheduler.clone(),
        );
        gateway::set_port_fallback(fallback);
        eprintln!("[Harness Gateway] Listening on http://{}.", addr);
        follow_port(&ctx.data_dir, ports.gateway_history);
        let ollama = match ollama {
            Ok(Some((port, stop, task))) => {
                eprintln!(
                    "[Harness Gateway] Ollama-compatible API also listening on {}:{}.",
                    settings.host, port
                );
                gateway::set_ollama_listener(Some(port), None);
                Some((stop, task))
            }
            Ok(None) => {
                gateway::set_ollama_listener(None, None);
                None
            }
            Err(warning) => {
                eprintln!(
                    "[Harness Gateway] Ollama-compatible API not started: {}",
                    warning
                );
                gateway::set_ollama_listener(None, Some(warning));
                None
            }
        };
        *self.running.lock().await = Some(Running {
            settings,
            client_token,
            scheduler,
            data_dir: ctx.data_dir,
            shutdown: tx,
            task,
            maintenance,
            links,
            ollama,
        });
        Ok(gateway::status())
    }

    /// Stops the listener (open streams end; loaded models keep running as ordinary
    /// servers) and removes the client token file.
    pub async fn stop(&self) {
        let Some(running) = self.running.lock().await.take() else {
            return;
        };
        running.maintenance.abort();
        running.links.abort();
        if let Some((stop, mut task)) = running.ollama {
            let _ = stop.send(());
            if tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
        let _ = running.shutdown.send(());
        let mut task = running.task;
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
        gateway::remove_client_token(&running.data_dir);
        gateway::set_stopped(None);
    }

    pub async fn status(&self) -> GatewayStatus {
        gateway::status()
    }

    /// Token Harness's own clients (the Playground) send while a token is required.
    pub async fn client_token(&self) -> Option<String> {
        self.running
            .lock()
            .await
            .as_ref()
            .map(|r| r.client_token.clone())
    }

    pub async fn scheduler(&self) -> Option<Arc<Scheduler>> {
        self.running
            .lock()
            .await
            .as_ref()
            .map(|r| r.scheduler.clone())
    }
}

fn cors(origins: &[String]) -> CorsLayer {
    let mut list: Vec<HeaderValue> = APP_ORIGINS
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    list.extend(origins.iter().filter_map(|o| HeaderValue::from_str(o).ok()));
    let mut expose = vec![
        HeaderName::from_static("x-harness-request-id"),
        HeaderName::from_static("x-harness-model"),
        HeaderName::from_static("x-harness-server"),
        HeaderName::from_static("x-harness-queue-ms"),
        HeaderName::from_static("x-harness-load-ms"),
        HeaderName::from_static("x-harness-compat"),
        HeaderName::from_static("x-harness-error"),
        // Speech: what the Playground's Speech panel reads.
        HeaderName::from_static("x-harness-voice"),
        HeaderName::from_static("x-harness-sample-rate"),
        HeaderName::from_static("x-harness-truncated"),
        header::RETRY_AFTER,
    ];
    expose.extend(
        auth::RATE_HEADERS
            .iter()
            .map(|h| HeaderName::from_static(h)),
    );
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(list))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(AllowHeaders::mirror_request())
        .expose_headers(expose)
        .max_age(std::time::Duration::from_secs(600))
}

macro_rules! inference {
    ($api:expr) => {
        post(
            |State(s): State<GwState>, h: HeaderMap, b: Bytes| async move {
                proxy::handle(s, $api, h, b).await
            },
        )
    };
}

type OllamaListener = (u16, oneshot::Sender<()>, tokio::task::JoinHandle<()>);

/// Binds the Ollama port (same routes, same host as the gateway). Something that already
/// answers there is never replaced: a real Ollama (its `/api/version` answers without
/// Harness's marker), another Harness gateway, or any other program holding the port each
/// give a warning key (`GatewayStatus::ollama_warning`) and the gateway keeps running on its
/// own port.
async fn ollama_listener(
    state: &GwState,
    settings: &GatewaySettings,
) -> Result<Option<OllamaListener>, String> {
    let port = settings.ollama_port;
    let probe_host = harness_core::process::connect_host(&settings.host);
    let probe_host = if probe_host.contains(':') && !probe_host.starts_with('[') {
        format!("[{}]", probe_host)
    } else {
        probe_host
    };
    let probe = state
        .http
        .get(format!("http://{}:{}/api/version", probe_host, port))
        .timeout(std::time::Duration::from_millis(1500))
        .send()
        .await;
    if let Ok(resp) = probe {
        let harness = resp.headers().contains_key("x-harness-compat");
        let is_ollama = resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .is_some_and(|v| v.get("version").is_some());
        let key = match (is_ollama, harness) {
            (true, false) => gateway::WARN_OLLAMA_RUNNING,
            (true, true) => gateway::WARN_OLLAMA_HARNESS,
            _ => gateway::WARN_OLLAMA_BUSY,
        };
        return Err(format!("{}: {}", key, port));
    }
    // Nothing answers: Ollama installed but not running keeps its default port (it would
    // fail to start later), and a program that holds the port without answering is named.
    let (installed, holder) = tokio::task::spawn_blocking(move || {
        (
            port == gateway::OLLAMA_DEFAULT_PORT && harness_core::ports::ollama_installed(),
            harness_core::ports::holder(port),
        )
    })
    .await
    .unwrap_or((false, None));
    if let Some(h) = holder {
        let key = if harness_core::ports::is_ollama(&h) {
            gateway::WARN_OLLAMA_RUNNING
        } else {
            gateway::WARN_OLLAMA_BUSY
        };
        return Err(format!("{}: {} ({})", key, port, h.label()));
    }
    if installed {
        return Err(format!("{}: {}", gateway::WARN_OLLAMA_INSTALLED, port));
    }
    let listener = match tokio::net::TcpListener::bind((settings.host.as_str(), port)).await {
        Ok(l) => l,
        Err(e) => return Err(format!("{}: {} ({})", gateway::WARN_OLLAMA_BUSY, port, e)),
    };
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let mut st = state.clone();
    st.addr = addr;
    let app = router(st, &settings.cors_origins);
    let (tx, rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    Ok(Some((addr.port(), tx, task)))
}

/// The gateway's routes (exposed for tests).
pub fn router(state: GwState, cors_origins: &[String]) -> Router {
    let allowed: Arc<[String]> = cors_origins.to_vec().into();
    Router::new()
        .merge(ollama::routes())
        .route("/health", get(health))
        .route(
            "/v1/harness/whoami",
            get(|State(s): State<GwState>, h: HeaderMap| async move {
                match auth::whoami(&s, &h) {
                    Ok(caller) => axum::Json(serde_json::json!({
                        "token_name": caller.token_name,
                        "scopes": caller.scopes,
                        "harness_version": env!("CARGO_PKG_VERSION"),
                    }))
                    .into_response(),
                    Err(error) => error.into_response(),
                }
            }),
        )
        .route(
            "/metrics",
            get(|State(s): State<GwState>, h: HeaderMap| metrics(s, h)),
        )
        .route(
            "/v1/models",
            get(|State(s): State<GwState>, h: HeaderMap| proxy::models(s, h)),
        )
        .route(
            "/v1/models/{*model}",
            get(
                |State(s): State<GwState>, h: HeaderMap, Path(m): Path<String>| {
                    proxy::model(s, h, m)
                },
            ),
        )
        .route("/v1/chat/completions", inference!(Api::Chat))
        .route("/v1/completions", inference!(Api::Completions))
        .route("/v1/responses", inference!(Api::Responses))
        .route(
            "/v1/responses/{id}",
            get(
                |State(s): State<GwState>,
                 h: HeaderMap,
                 Path(id): Path<String>,
                 Query(q): Query<responses::RetrieveQuery>| {
                    responses::get(s, h, id, q)
                },
            )
            .delete(
                |State(s): State<GwState>, h: HeaderMap, Path(id): Path<String>| {
                    responses::delete(s, h, id)
                },
            ),
        )
        .route(
            "/v1/responses/{id}/cancel",
            post(
                |State(s): State<GwState>, h: HeaderMap, Path(id): Path<String>| {
                    responses::cancel(s, h, id)
                },
            ),
        )
        .route(
            "/v1/responses/{id}/input_items",
            get(
                |State(s): State<GwState>,
                 h: HeaderMap,
                 Path(id): Path<String>,
                 Query(q): Query<harness_core::gateway::responses::ItemsQuery>| {
                    responses::input_items(s, h, id, q)
                },
            ),
        )
        .route(
            "/v1/images/generations",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| {
                images::handle(s, images::Kind::Generations, h, b)
            }),
        )
        .route(
            "/v1/images/edits",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| {
                images::handle(s, images::Kind::Edits, h, b)
            }),
        )
        .route(
            "/v1/videos",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| videos::create(s, h, b))
                .get(|State(s): State<GwState>, h: HeaderMap| videos::list(s, h)),
        )
        .route(
            "/v1/videos/{id}",
            get(
                |State(s): State<GwState>, h: HeaderMap, Path(id): Path<String>| {
                    videos::retrieve(s, h, id)
                },
            )
            .delete(
                |State(s): State<GwState>, h: HeaderMap, Path(id): Path<String>| {
                    videos::delete(s, h, id)
                },
            ),
        )
        .route(
            "/v1/videos/{id}/content",
            get(
                |State(s): State<GwState>, h: HeaderMap, Path(id): Path<String>| {
                    videos::content(s, h, id)
                },
            ),
        )
        .route("/v1/embeddings", inference!(Api::Embeddings))
        .route("/v1/rerank", inference!(Api::Rerank))
        .route("/v1/hf", inference!(Api::Hf))
        .route(
            "/v1/hf/{model}",
            post(
                |State(s): State<GwState>,
                 h: HeaderMap,
                 Path(model): Path<String>,
                 Query(q): Query<std::collections::HashMap<String, String>>,
                 b: Bytes| async move {
                    let ct = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok());
                    match hf::body(&model, ct, q.get("parameters").map(String::as_str), b) {
                        Ok(body) => proxy::handle(s, Api::Hf, h, body).await,
                        Err(e) => e.into_response(),
                    }
                },
            ),
        )
        .route(
            "/v1/audio/transcriptions",
            post(
                |State(s): State<GwState>,
                 h: HeaderMap,
                 m: Result<
                    axum::extract::Multipart,
                    axum::extract::multipart::MultipartRejection,
                >| { audio::handle(s, audio::Task::Transcribe, h, m) },
            ),
        )
        .route(
            "/v1/audio/translations",
            post(
                |State(s): State<GwState>,
                 h: HeaderMap,
                 m: Result<
                    axum::extract::Multipart,
                    axum::extract::multipart::MultipartRejection,
                >| { audio::handle(s, audio::Task::Translate, h, m) },
            ),
        )
        .route(
            "/v1/audio/speech",
            post(|State(s): State<GwState>, h: HeaderMap, body: Bytes| speech::handle(s, h, body)),
        )
        .route(
            "/v1/audio/process",
            post(
                |State(s): State<GwState>, request: axum::extract::Request| {
                    audio_process::handle(s, request)
                },
            ),
        )
        .route(
            "/v1/systemone",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| {
                decisions::handle(s, decisions::Flavor::SystemOne, h, b)
            }),
        )
        .route(
            "/v1/decisions",
            post(|State(s): State<GwState>, h: HeaderMap, b: Bytes| {
                decisions::handle(s, decisions::Flavor::Decisions, h, b)
            }),
        )
        .route("/v1/messages", inference!(Api::Messages))
        .route("/v1/messages/count_tokens", inference!(Api::CountTokens))
        .fallback(not_found)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            guard_tts_routes,
        ))
        .layer(axum::middleware::from_fn(schedule_priority))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(cors(cors_origins))
        // Outermost: a foreign browser origin is answered before CORS (which would answer a
        // preflight itself), the body, a worker or a model load. One layer covers both the
        // gateway port and the Ollama-compatible port (both are built by this function).
        .layer(axum::middleware::from_fn_with_state(allowed, origin_guard))
        .with_state(state)
}

/// Apply one load priority across text, image, audio and Ollama routes. The
/// task-local follows awaited engine starts without changing model definitions.
async fn schedule_priority(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let priority = request_priority(request.headers(), request.uri().path());
    harness_core::jobs::JOB_PRIORITY
        .scope(priority, next.run(request))
        .await
}

fn request_priority(headers: &HeaderMap, path: &str) -> harness_core::jobs::Priority {
    use harness_core::jobs::Priority;
    let value = |key: &str| headers.get(key).and_then(|value| value.to_str().ok());
    if matches!(
        path,
        "/v1/embeddings" | "/v1/rerank" | "/api/embeddings" | "/api/embed"
    ) || value("x-harness-purpose") == Some("warmup")
    {
        return Priority::Background;
    }
    match value("x-harness-priority") {
        Some("background") => Priority::Background,
        Some("interactive") => Priority::Interactive,
        Some("agent") => Priority::Agent,
        _ if value("x-harness-client") == Some("harness-playground") => Priority::Interactive,
        _ => Priority::Agent,
    }
}

#[cfg(test)]
mod job_priority_tests {
    use super::*;
    use harness_core::jobs::Priority;

    #[test]
    fn chat_agents_and_background_routes_share_priority_rules() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            request_priority(&headers, "/v1/chat/completions"),
            Priority::Agent
        );
        headers.insert(
            "x-harness-client",
            HeaderValue::from_static("harness-playground"),
        );
        for path in [
            "/v1/chat/completions",
            "/v1/audio/speech",
            "/v1/images/generations",
        ] {
            assert_eq!(request_priority(&headers, path), Priority::Interactive);
        }
        headers.insert(
            "x-harness-priority",
            HeaderValue::from_static("interactive"),
        );
        assert_eq!(
            request_priority(&headers, "/v1/embeddings"),
            Priority::Background
        );
        assert_eq!(
            request_priority(&headers, "/api/embed"),
            Priority::Background
        );
        headers.insert("x-harness-purpose", HeaderValue::from_static("warmup"));
        assert_eq!(
            request_priority(&headers, "/v1/chat/completions"),
            Priority::Background
        );
        headers.remove("x-harness-purpose");
        headers.insert("x-harness-priority", HeaderValue::from_static("background"));
        assert_eq!(
            request_priority(&headers, "/v1/audio/speech"),
            Priority::Background
        );
    }
}

/// `403 forbidden_origin` for browser requests from a page that is neither the Harness window
/// nor on the user's CORS list (see [`harness_core::network::check_browser_origin`]). Requests
/// without an `Origin` header (curl, SDKs, CLI clients) pass untouched.
async fn origin_guard(
    State(allowed): State<Arc<[String]>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    use harness_core::network::OriginDenied;
    if let Err((why, origin)) = harness_core::network::check_browser_headers(
        request.headers().get("origin").map(|v| v.as_bytes()),
        request
            .headers()
            .get("sec-fetch-site")
            .map(|v| v.as_bytes()),
        &allowed,
    ) {
        let message = match why {
            OriginDenied::Null => {
                "Requests with Origin: null are not accepted by the Harness gateway.".to_string()
            }
            OriginDenied::CrossSite => "Cross-site browser requests without an allowed Origin are not accepted by the Harness gateway.".to_string(),
            OriginDenied::NotAllowed => format!(
                "The origin \"{origin}\" is not allowed. Add it to \"Allowed browser origins\" in the gateway settings to let that web page call the gateway."
            ),
        };
        return errors::GatewayError::new(403, "forbidden_origin", message)
            .anthropic(request.uri().path().starts_with("/v1/messages"))
            .into_response();
    }
    next.run(request).await
}

/// Reject model-targeted requests before route handlers can start a worker or expose controls.
async fn guard_tts_routes(
    State(state): State<GwState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if request.method() == Method::POST && request.uri().path() == "/v1/audio/speech" {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if matches!(
        path,
        "/load" | "/v1/voices" | "/voices" | "/v1/audio/voices"
    ) {
        return errors::GatewayError::new(
            400,
            "unsupported_route",
            "Worker control and voice upload routes are not exposed",
        )
        .into_response();
    }
    if !request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.starts_with("application/json"))
    {
        return next.run(request).await;
    }
    let path = path.to_string();
    let (parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return errors::GatewayError::new(
                400,
                "invalid_request",
                "Request body exceeds the gateway limit",
            )
            .into_response()
        }
    };
    if let Some(model) = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(String::from))
    {
        if let Ok(def) = state.scheduler.resolve(Some(&model)).await {
            if harness_core::link::parse_model(&def.id).is_some()
                && !matches!(
                    path.as_str(),
                    "/v1/chat/completions"
                        | "/v1/completions"
                        | "/v1/responses"
                        | "/v1/embeddings"
                        | "/v1/rerank"
                        | "/v1/messages"
                )
            {
                let caller = match auth::check(
                    &state,
                    &parts.headers,
                    false,
                    harness_core::token_policy::GatewayEndpoint::of_path(&path)
                        .unwrap_or(harness_core::token_policy::GatewayEndpoint::Chat),
                ) {
                    Ok(caller) => caller,
                    Err(e) => return e.into_response(),
                };
                if let Err(e) = caller.check_model(&def.alias) {
                    return e.into_response();
                }
                return errors::GatewayError::remote("link.unsupported_endpoint").into_response();
            }
            if speech::is_crispasr_tts(&def) {
                if let Err(e) = auth::check(
                    &state,
                    &parts.headers,
                    false,
                    harness_core::token_policy::GatewayEndpoint::Audio,
                ) {
                    return e.into_response();
                }
                return errors::GatewayError::new(
                    400,
                    "unsupported_route",
                    "CrispASR TTS only accepts POST /v1/audio/speech",
                )
                .into_response();
            }
        }
    }
    next.run(axum::extract::Request::from_parts(
        parts,
        axum::body::Body::from(bytes),
    ))
    .await
}

async fn health() -> Response {
    axum::Json(serde_json::json!({"status": "ok", "service": "harness-gateway"})).into_response()
}

async fn metrics(state: GwState, headers: HeaderMap) -> Response {
    if let Err(e) = auth::check(
        &state,
        &headers,
        false,
        harness_core::token_policy::GatewayEndpoint::Metrics,
    ) {
        return e.into_response();
    }
    (
        [(
            header::CONTENT_TYPE,
            harness_core::analytics::prometheus::CONTENT_TYPE,
        )],
        harness_core::analytics::prometheus::render().await,
    )
        .into_response()
}

async fn not_found(uri: axum::http::Uri) -> Response {
    errors::GatewayError::new(
        404,
        "not_found",
        format!(
            "{} is not a gateway endpoint (see /v1/models, /v1/chat/completions, /v1/responses, /v1/messages, /v1/embeddings, /v1/rerank, and the Ollama-compatible /api/*).",
            uri.path()
        ),
    )
    .into_response()
}

#[cfg(test)]
mod cors_tests {
    /// The app's own pages (a cross-origin fetch from the webview) can read the headers the
    /// Playground uses, the speech ones included (`x-harness-truncated` says the audio is partial).
    #[tokio::test]
    async fn the_app_reads_the_speech_headers() {
        let app = axum::Router::new()
            .route("/", axum::routing::get(|| async { "ok" }))
            .layer(super::cors(&[]));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let res = reqwest::Client::new()
            .get(format!("http://{addr}/"))
            .header("origin", super::APP_ORIGINS[0])
            .send()
            .await
            .unwrap();
        let exposed = res
            .headers()
            .get("access-control-expose-headers")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        for header in [
            "x-harness-truncated",
            "x-harness-voice",
            "x-harness-sample-rate",
            "x-harness-request-id",
        ] {
            assert!(exposed.contains(header), "{header} not in {exposed:?}");
        }
    }
}
