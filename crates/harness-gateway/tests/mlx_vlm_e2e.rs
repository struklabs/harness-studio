//! The MLX vision engine (mlx-vlm) end to end, on Apple Silicon (no mocks): the managed
//! `mlx-vlm` environment is built from its hashed lock, the library finds an MLX vision model
//! folder, and a chat with an image goes through the gateway to the worker it starts.
//!
//! ```text
//! HS_MLX_VLM_MODEL=<an mlx-community vision model folder, e.g. Qwen3-VL-2B-Instruct-4bit> \
//!   cargo test -p harness-gateway --test mlx_vlm_e2e -- --ignored --nocapture
//! ```
//!
//! `HS_MLX_E2E_ROOT=<folder>` keeps the data root (and the environment) between runs; by
//! default it is a temporary folder (uv, Python and the packages: about 800 MB).

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

/// A 64x64 red square.
const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAT0lEQVR42u3PQQkAAAgEsAtx/ZMZxgi+hcEKLNO+FgEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQGBywKqxUDxqh7TUQAAAABJRU5ErkJggg==";

#[tokio::test]
#[ignore = "builds the mlx-vlm environment (~800 MB) and needs HS_MLX_VLM_MODEL"]
async fn an_mlx_vision_model_sees_an_image_through_the_gateway() {
    let Some(model) = std::env::var_os("HS_MLX_VLM_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        eprintln!("HS_MLX_VLM_MODEL not set, skipping");
        return;
    };
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_MLX_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("mlx-vlm-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    // `<root>/<owner>/<repo>`, as a download lays it out.
    settings.models_dir = model
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 19890;
    settings.save().unwrap();

    // The engine is missing until its environment is built (a kept root may have it already).
    if let Err(missing) = harness_core::engine::mlx::vlm_binary() {
        assert!(
            missing.contains("runtime.family_not_installed"),
            "{missing}"
        );
    }
    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("mlx-vlm").await.unwrap();
    eprintln!(
        "environment in {:?}: {} bytes, {:?}",
        t.elapsed(),
        env.size_bytes,
        env.manifest.as_ref().map(|m| &m.python)
    );
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );
    harness_core::engine::mlx::vlm_binary().unwrap();

    // The library lists the folder as an MLX vision model (not as a mistral.rs one).
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    let found = library
        .iter()
        .find(|m| m.path == model)
        .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()));
    eprintln!(
        "library: {} {} {}",
        found.name, found.quantization, found.size_bytes
    );
    assert_eq!(found.kind, harness_core::domain::ModelKind::Llm);
    assert!(harness_core::engine::mlx::is_vlm_dir(&model));
    assert!(!harness_core::engine::mlx::is_model_dir(&model));
    assert!(!harness_core::engine::is_mistralrs_dir(&model));

    let def = ModelDefinition::new(
        "mlx-vlm-e2e",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias == "mlx-vlm-e2e" {
            store.delete_definition(&old.id).unwrap();
        }
    }
    store.save_definition(def).unwrap();

    let registry = Arc::new(ServerRegistry::default());
    let server = GatewayServer::new();
    let status = server
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: dir.clone(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 19880,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    let t = Instant::now();
    let res = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": "mlx-vlm-e2e",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "What color is this image? Answer with one word."},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{RED_PNG}")}}
            ]}],
            "max_tokens": 20,
            "temperature": 0
        }))
        .send()
        .await
        .unwrap();
    let code = res.status();
    let body: Value = res.json().await.unwrap();
    eprintln!("first answer in {:?} (load included): {body}", t.elapsed());
    assert_eq!(code, 200, "{body}");
    let answer = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");
    assert!(answer.to_lowercase().contains("red"), "{answer:?}");

    // Plain text and streaming.
    let res = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": "mlx-vlm-e2e",
            "messages": [{"role": "user", "content": "What is the capital of France? One word."}],
            "max_tokens": 20,
            "stream": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let text = res.text().await.unwrap();
    assert!(text.contains("data: "), "{text}");
    assert!(text.contains("Paris"), "{text}");

    // An image by URL or file path never reaches mlx-vlm (it would fetch or read it).
    for url in [
        "http://127.0.0.1:9/x.png",
        "/etc/hosts",
        "file:///etc/hosts",
    ] {
        let res = http
            .post(format!("{base}/v1/chat/completions"))
            .json(&json!({
                "model": "mlx-vlm-e2e",
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": "Describe."},
                    {"type": "image_url", "image_url": {"url": url}}
                ]}],
                "max_tokens": 5
            }))
            .send()
            .await
            .unwrap();
        let code = res.status();
        let body = res.text().await.unwrap();
        eprintln!("{url}: {code} {body}");
        assert_eq!(code, 400, "{url}: {body}");
    }

    // The worker's own port refuses a request without its key.
    let worker = registry
        .active_servers()
        .into_iter()
        .map(|(_, info)| info.port)
        .next()
        .expect("a running worker");
    let res = http
        .get(format!("http://127.0.0.1:{worker}/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);

    server.stop().await;
    registry.stop_all().await;
}
