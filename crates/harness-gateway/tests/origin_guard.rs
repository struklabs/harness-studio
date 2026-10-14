//! Browser Origin gate of the gateway against the real in-process listeners (gateway port,
//! Ollama-compatible port). No llama-server is needed: the requests that pass the gate are
//! unknown-model requests (404 before any load), and the ones that do not never reach a handler.
//!
//! Temporary data root (never the user's) and in-memory secrets.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use reqwest::Method;
use serde_json::{json, Value};

const OPEN_WEBUI: &str = "http://localhost:3000";
const EVIL: &str = "https://evil.example";

fn data_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("gw-origin"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.control_api.port = 18080;
        s.save().unwrap();
        dir
    })
}

/// One gateway at a time (the gateway status is process-wide).
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Gw {
    server: GatewayServer,
    registry: Arc<ServerRegistry>,
    base: String,
    ollama: Option<String>,
    http: reqwest::Client,
}

impl Gw {
    async fn start(ollama: bool) -> Self {
        let registry = Arc::new(ServerRegistry::default());
        let ctx = GatewayContext {
            registry: registry.clone(),
            runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
            gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
            data_dir: data_root().to_path_buf(),
            downloads: None,
        };
        let s = GatewaySettings {
            enabled: true,
            port: 0,
            cors_origins: vec![OPEN_WEBUI.into()],
            ollama_port_enabled: ollama,
            ollama_port: if ollama { free_port() } else { 0 },
            ..Default::default()
        };
        let server = GatewayServer::new();
        let status = server.start_ephemeral(ctx, &s).await.unwrap();
        Gw {
            base: status.endpoint.unwrap().host_url,
            ollama: status.ollama_port.map(|p| format!("http://127.0.0.1:{p}")),
            server,
            registry,
            http: reqwest::Client::new(),
        }
    }

    /// A "simple" cross-site POST: multipart, no custom headers, so no preflight.
    fn multipart(&self, base: &str, path: &str, origin: Option<&str>) -> reqwest::RequestBuilder {
        let boundary = "----hs-origin-boundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nnope\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\n\
             Content-Type: audio/wav\r\n\r\nRIFF\r\n--{boundary}--\r\n"
        );
        let mut req = self
            .http
            .post(format!("{base}{path}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body);
        if let Some(o) = origin {
            req = req.header("origin", o);
        }
        req
    }

    fn no_workers(&self) {
        let running: Vec<String> = self
            .registry
            .all()
            .into_iter()
            .filter(|(_, pm)| pm.get_status() != harness_core::process::ServerStatus::Stopped)
            .map(|(id, _)| id)
            .collect();
        assert!(running.is_empty(), "a worker was started: {running:?}");
    }
}

async fn body(r: reqwest::Response) -> (u16, Value) {
    let status = r.status().as_u16();
    let text = r.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

fn assert_forbidden_origin(status: u16, body: &Value) {
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "forbidden_origin", "{body}");
}

#[tokio::test]
async fn foreign_origins_are_refused_before_anything_happens() {
    let _s = serial().await;
    let gw = Gw::start(false).await;
    for path in ["/v1/audio/process", "/v1/audio/transcriptions"] {
        let r = gw
            .multipart(&gw.base, path, Some(EVIL))
            .send()
            .await
            .unwrap();
        assert!(r.headers().get("access-control-allow-origin").is_none());
        let (status, b) = body(r).await;
        assert_forbidden_origin(status, &b);
    }
    // Urlencoded and text/plain posts too, and the Anthropic route (Anthropic error shape).
    for ct in ["application/x-www-form-urlencoded", "text/plain"] {
        let r = gw
            .http
            .post(format!("{}/v1/chat/completions", gw.base))
            .header("origin", EVIL)
            .header("content-type", ct)
            .body("model=nope")
            .send()
            .await
            .unwrap();
        let (status, b) = body(r).await;
        assert_forbidden_origin(status, &b);
    }
    let r = gw
        .http
        .post(format!("{}/v1/messages", gw.base))
        .header("origin", EVIL)
        .json(&json!({"model": "nope", "max_tokens": 1, "messages": []}))
        .send()
        .await
        .unwrap();
    let (status, b) = body(r).await;
    assert_eq!(status, 403, "{b}");
    assert_eq!(b["type"], "error", "{b}");
    assert_eq!(b["error"]["code"], "forbidden_origin", "{b}");
    // GET too (the model list is not for foreign pages either).
    let r = gw
        .http
        .get(format!("{}/v1/models", gw.base))
        .header("origin", EVIL)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    gw.no_workers();
    gw.server.stop().await;
}

#[tokio::test]
async fn null_origin_cross_site_and_preflight_are_refused() {
    let _s = serial().await;
    let gw = Gw::start(false).await;
    let (status, b) = body(
        gw.multipart(&gw.base, "/v1/audio/transcriptions", Some("null"))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_forbidden_origin(status, &b);

    // Sec-Fetch-Site: cross-site from a non-allowlisted origin, and with no Origin at all
    // (a no-cors request).
    let (status, b) = body(
        gw.multipart(&gw.base, "/v1/audio/transcriptions", Some(EVIL))
            .header("sec-fetch-site", "cross-site")
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_forbidden_origin(status, &b);
    let (status, b) = body(
        gw.multipart(&gw.base, "/v1/audio/transcriptions", None)
            .header("sec-fetch-site", "cross-site")
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_forbidden_origin(status, &b);

    // The preflight is answered by the gate, not by the CORS layer.
    let pre = gw
        .http
        .request(
            Method::OPTIONS,
            format!("{}/v1/audio/transcriptions", gw.base),
        )
        .header("origin", EVIL)
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .send()
        .await
        .unwrap();
    assert!(pre.headers().get("access-control-allow-origin").is_none());
    let (status, b) = body(pre).await;
    assert_forbidden_origin(status, &b);
    gw.no_workers();
    gw.server.stop().await;
}

#[tokio::test]
async fn allowed_origins_and_clients_without_origin_still_work() {
    let _s = serial().await;
    let gw = Gw::start(false).await;
    // curl-like: no Origin, no browser headers. Reaches the handler (unknown model: 404).
    let (status, b) = body(
        gw.multipart(&gw.base, "/v1/audio/transcriptions", None)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_ne!(b["error"]["code"], "forbidden_origin", "{b}");
    assert_eq!(status, 404, "{b}");

    // An Open WebUI-like page on the CORS list, cross-site fetch included.
    let r = gw
        .multipart(&gw.base, "/v1/audio/transcriptions", Some(OPEN_WEBUI))
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers()["access-control-allow-origin"], OPEN_WEBUI);
    let (status, b) = body(r).await;
    assert_ne!(b["error"]["code"], "forbidden_origin", "{b}");
    assert_eq!(status, 404, "{b}");
    let pre = gw
        .http
        .request(Method::OPTIONS, format!("{}/v1/chat/completions", gw.base))
        .header("origin", OPEN_WEBUI)
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .send()
        .await
        .unwrap();
    assert!(pre.status().is_success(), "{}", pre.status());
    assert_eq!(pre.headers()["access-control-allow-origin"], OPEN_WEBUI);

    // The Harness window itself.
    for app in ["tauri://localhost", "http://tauri.localhost"] {
        let r = gw
            .http
            .get(format!("{}/health", gw.base))
            .header("origin", app)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{app}");
    }
    gw.no_workers();
    gw.server.stop().await;
}

#[tokio::test]
async fn ollama_port_applies_the_same_gate() {
    let _s = serial().await;
    let gw = Gw::start(true).await;
    let ollama = gw.ollama.clone().expect("the Ollama port is bound");
    for (method, path) in [
        (Method::POST, "/api/chat"),
        (Method::POST, "/api/generate"),
        (Method::GET, "/api/tags"),
        (Method::OPTIONS, "/api/chat"),
    ] {
        let r = gw
            .http
            .request(method.clone(), format!("{ollama}{path}"))
            .header("origin", EVIL)
            .send()
            .await
            .unwrap();
        let (status, b) = body(r).await;
        assert_forbidden_origin(status, &b);
    }
    let (status, b) = body(
        gw.multipart(&ollama, "/v1/audio/transcriptions", Some(EVIL))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_forbidden_origin(status, &b);
    // No Origin, or an allowed one: unchanged.
    let r = gw
        .http
        .get(format!("{ollama}/api/tags"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = gw
        .http
        .get(format!("{ollama}/api/tags"))
        .header("origin", OPEN_WEBUI)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    gw.no_workers();
    gw.server.stop().await;
}
