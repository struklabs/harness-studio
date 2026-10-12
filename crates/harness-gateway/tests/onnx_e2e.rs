//! ONNX encoder models through the real gateway (no mocks): `/v1/embeddings` and `/v1/rerank`
//! by model alias load a `harness-onnx` server on demand (the managed ONNX Runtime library is
//! downloaded on that first use), the local-document retrieval (RAG) client talks to the same
//! endpoints, and the models are listed like any other.
//!
//! ```text
//! HS_ONNX_MODELS=<folder with sentence-transformers/all-MiniLM-L6-v2 and
//!                 cross-encoder/ms-marco-MiniLM-L6-v2 as Harness downloads them>
//! cargo build -p harness-onnx
//! cargo test -p harness-gateway --test onnx_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets. The sidecar is the one built by
//! `cargo build -p harness-onnx` (`target/debug`), or `HARNESS_ONNX_BIN`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::process::ServerStatus;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

fn models_dir() -> Option<PathBuf> {
    let p = std::env::var_os("HS_ONNX_MODELS").map(PathBuf::from);
    if p.is_none() {
        eprintln!("HS_ONNX_MODELS not set, skipping");
    }
    p
}

fn minilm(root: &Path) -> PathBuf {
    root.join("sentence-transformers/all-MiniLM-L6-v2/onnx/model.onnx")
}

fn cross_encoder(root: &Path) -> PathBuf {
    root.join("cross-encoder/ms-marco-MiniLM-L6-v2/onnx/model.onnx")
}

#[tokio::test]
#[ignore = "needs HS_ONNX_MODELS, the harness-onnx sidecar and downloads ONNX Runtime"]
async fn embeddings_and_rerank_by_alias() {
    let Some(models) = models_dir() else { return };
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("onnx-e2e"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    settings.control_api.port = 19979;
    settings.save().unwrap();

    // Two definitions, nothing else about the files is told to Harness.
    let store = harness_core::domain::store::Domain::global();
    store
        .save_definition(ModelDefinition::new(
            "onnx-minilm",
            ModelRef::Local {
                path: minilm(&models),
            },
        ))
        .unwrap();
    store
        .save_definition(ModelDefinition::new(
            "onnx-ranker",
            ModelRef::Local {
                path: cross_encoder(&models),
            },
        ))
        .unwrap();

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
                port: 19970,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    // The retrieval models the RAG picker offers include both, with their roles.
    let defs = store.definitions().unwrap();
    let retrieval = harness_core::rag::models::retrieval_models(&defs);
    let find = |alias: &str| retrieval.iter().find(|m| m.alias == alias).cloned();
    let embed = find("onnx-minilm").expect("the embedder is a retrieval model");
    assert_eq!(
        embed.role,
        harness_core::rag::models::RetrievalRole::Embedding
    );
    assert_eq!(embed.dims, Some(384));
    assert_eq!(embed.context_length, Some(256));
    assert_eq!(
        find("onnx-ranker").expect("the reranker is one too").role,
        harness_core::rag::models::RetrievalRole::Rerank
    );

    // 1. Embeddings: the first request loads the model (and fetches ONNX Runtime).
    let t = std::time::Instant::now();
    let res = http
        .post(format!("{base}/v1/embeddings"))
        .json(&json!({"model": "onnx-minilm", "input": ["This is an example sentence"]}))
        .send()
        .await
        .unwrap();
    let status = res.status();
    let load_ms = res
        .headers()
        .get("x-harness-load-ms")
        .map(|v| v.to_str().unwrap().to_string());
    let body: Value = res.json().await.unwrap();
    eprintln!(
        "first embeddings request: {status} in {:?}, load {load_ms:?}",
        t.elapsed()
    );
    assert_eq!(status, 200, "{body}");
    let v: Vec<f64> = body["data"][0]["embedding"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    assert_eq!(v.len(), 384);
    // First value of sentence-transformers' own encode() of that sentence.
    assert!((v[0] - 0.067_656_9).abs() < 1e-3, "{}", v[0]);

    // The RAG client (what Playground documents use) goes through the same route.
    let endpoint = harness_core::rag::client::Endpoint {
        base_url: format!("{base}/v1"),
        token: None,
    };
    let vectors = harness_core::rag::client::embed(
        &endpoint,
        "onnx-minilm",
        &[
            "A dog is chasing a ball in the park".to_string(),
            "A puppy runs after a toy in the garden".to_string(),
            "Quarterly revenue grew by twelve percent".to_string(),
        ],
    )
    .await
    .unwrap();
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    assert!(dot(&vectors[0], &vectors[1]) > dot(&vectors[0], &vectors[2]) + 0.2);

    // 2. Rerank by alias, through the RAG client as well.
    let ranked = harness_core::rag::client::rerank(
        &endpoint,
        "onnx-ranker",
        "How many people live in Berlin?",
        &[
            "The weather is nice today.".to_string(),
            "Berlin had a population of 3,520,031 registered inhabitants in an area of 891.82 square kilometers.".to_string(),
            "Berlin is well known for its museums.".to_string(),
        ],
    )
    .await
    .unwrap();
    eprintln!("rerank: {ranked:?}");
    assert_eq!(
        ranked.iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![1, 2, 0]
    );

    // 3. Both servers run side by side, as ONNX servers on their own ports.
    let running: Vec<_> = registry
        .all()
        .into_iter()
        .filter(|(_, pm)| pm.get_status() == ServerStatus::Running)
        .collect();
    assert_eq!(running.len(), 2, "one server per model");
    for (id, pm) in &running {
        let cfg = pm.launch_config().unwrap();
        assert_eq!(
            harness_core::engine::server_for(&cfg).family(),
            harness_core::engine::ONNX_FAMILY,
            "{id}"
        );
        let mode = pm.active_server().unwrap().serve_mode;
        eprintln!("{id}: {:?} ({:?})", cfg.alias, mode);
    }

    // 4. The gateway lists them, and the wrong endpoint for a model is a clear error.
    let models: Value = http
        .get(format!("{base}/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"onnx-minilm") && ids.contains(&"onnx-ranker"),
        "{ids:?}"
    );
    let wrong = http
        .post(format!("{base}/v1/rerank"))
        .json(&json!({"model": "onnx-minilm", "query": "q", "documents": ["d"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 501);

    server.stop().await;
    registry.stop_all().await;
}
