//! The MLX engine end to end, on Apple Silicon (no mocks): the managed `mlx` environment is
//! built from its hashed lock, the library finds an MLX model folder, and a chat goes through
//! the gateway to the worker it starts.
//!
//! ```text
//! HS_MLX_MODEL=<an mlx-community chat model folder, e.g. Qwen3-4B-Instruct-2507-4bit> \
//!   cargo test -p harness-gateway --test mlx_e2e -- --ignored --nocapture
//! ```
//!
//! `HS_MLX_E2E_ROOT=<folder>` keeps the data root (and the environment) between runs; by
//! default it is a temporary folder (uv, Python and the packages: about 400 MB).

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

#[tokio::test]
#[ignore = "builds the mlx environment (~400 MB) and needs HS_MLX_MODEL"]
async fn an_mlx_model_chats_through_the_gateway() {
    let Some(model) = std::env::var_os("HS_MLX_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        eprintln!("HS_MLX_MODEL not set, skipping");
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
            .prefix(&harness_core::paths::test_dir_prefix("mlx-e2e"))
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
    settings.control_api.port = 19889;
    settings.save().unwrap();

    // The engine is missing until its environment is built.
    let missing = harness_core::engine::mlx::binary().unwrap_err();
    assert!(
        missing.contains("runtime.family_not_installed"),
        "{missing}"
    );
    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("mlx").await.unwrap();
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
    harness_core::engine::mlx::binary().unwrap();

    // The library lists the folder as an MLX chat model (not as a mistral.rs one).
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
    assert!(harness_core::engine::mlx::is_model_dir(&model));
    assert!(!harness_core::engine::is_mistralrs_dir(&model));

    let def = ModelDefinition::new(
        "mlx-e2e",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias == "mlx-e2e" {
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
                port: 19879,
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
            "model": "mlx-e2e",
            "messages": [{"role": "user", "content": "What is the capital of France? Answer in one short sentence."}],
            "max_tokens": 40,
            "temperature": 0
        }))
        .send()
        .await
        .unwrap();
    let code = res.status();
    let headers = res.headers().clone();
    let body: Value = res.json().await.unwrap();
    eprintln!("first answer in {:?} (load included): {body}", t.elapsed());
    assert_eq!(code, 200, "{body}");
    let answer = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");
    assert!(answer.to_lowercase().contains("paris"), "{answer:?}");
    eprintln!("headers: {:?}", headers.get("x-harness-model"));

    // Streaming and a tool call.
    let res = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": "mlx-e2e",
            "messages": [{"role": "user", "content": "Count from 1 to 3."}],
            "max_tokens": 20,
            "stream": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let text = res.text().await.unwrap();
    assert!(text.contains("data: "), "{text}");
    let res = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": "mlx-e2e",
            "messages": [{"role": "user", "content": "What is the weather in Ankara?"}],
            "max_tokens": 100,
            "tools": [{"type": "function", "function": {"name": "get_weather", "description": "Get the weather of a city", "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}}}]
        }))
        .send()
        .await
        .unwrap();
    let body: Value = res.json().await.unwrap();
    eprintln!("tool call: {}", body["choices"][0]["message"]);
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["function"]["name"], "get_weather",
        "{body}"
    );

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
