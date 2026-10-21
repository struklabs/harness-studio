//! The MLX retrieval engine (mlx-embeddings, Qwen3-Reranker on mlx-lm) end to end, on Apple
//! Silicon (no mocks): the managed `mlx-vlm` environment (which carries both) is built from its
//! hashed lock, the library lists an MLX embedder and an MLX reranker, the knowledge-base picker
//! offers them, and `/v1/embeddings` and `/v1/rerank` go through the gateway to the workers it
//! starts.
//!
//! ```text
//! HS_MLX_EMBED_MODEL=<an mlx-community embedder folder, e.g. bge-small-en-v1.5-bf16> \
//! HS_MLX_RERANK_MODEL=<an mlx-community Qwen3-Reranker folder, e.g. Qwen3-Reranker-0.6B-4bit> \
//!   cargo test -p harness-gateway --test mlx_embed_e2e -- --ignored --nocapture
//! ```
//!
//! Both folders must be in the same models folder (`<root>/<owner>/<repo>`).
//! `HS_MLX_E2E_ROOT=<folder>` keeps the data root (and the environment) between runs; by
//! default it is a temporary folder (uv, Python and the packages: about 800 MB).

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::process::ServerStatus;
use harness_core::rag::models::RetrievalRole;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

#[tokio::test]
#[ignore = "builds the mlx-vlm environment (~800 MB) and needs HS_MLX_EMBED_MODEL / HS_MLX_RERANK_MODEL"]
async fn mlx_embeddings_and_rerank_run_through_the_gateway() {
    let folder = |var: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
    };
    let (Some(embedder), Some(reranker)) =
        (folder("HS_MLX_EMBED_MODEL"), folder("HS_MLX_RERANK_MODEL"))
    else {
        eprintln!("HS_MLX_EMBED_MODEL / HS_MLX_RERANK_MODEL not set, skipping");
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
            .prefix(&harness_core::paths::test_dir_prefix("mlx-embed-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    settings.models_dir = embedder
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 19892;
    settings.save().unwrap();

    if let Err(missing) = harness_core::engine::mlx::embed_binary() {
        assert!(
            missing.contains("runtime.family_not_installed"),
            "{missing}"
        );
    }
    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("mlx-vlm").await.unwrap();
    eprintln!("environment in {:?}: {} bytes", t.elapsed(), env.size_bytes);
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );
    harness_core::engine::mlx::embed_binary().unwrap();

    // The library lists them as an embedding model and a reranker (not as chat models).
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    let kind_of = |path: &PathBuf| {
        library
            .iter()
            .find(|m| &m.path == path)
            .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()))
            .kind
    };
    assert_eq!(
        kind_of(&embedder),
        harness_core::domain::ModelKind::Embedding
    );
    assert_eq!(
        kind_of(&reranker),
        harness_core::domain::ModelKind::Reranker
    );
    assert!(!harness_core::engine::mlx::is_model_dir(&reranker));

    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias.starts_with("mlx-e2e-") {
            store.delete_definition(&old.id).unwrap();
        }
    }
    for (alias, path) in [("mlx-e2e-embed", &embedder), ("mlx-e2e-rerank", &reranker)] {
        store
            .save_definition(ModelDefinition::new(
                alias,
                ModelRef::Local { path: path.clone() },
            ))
            .unwrap();
    }

    // The knowledge-base picker offers both, with their roles.
    let retrieval = harness_core::rag::models::retrieval_models(&store.definitions().unwrap());
    let find = |alias: &str| retrieval.iter().find(|m| m.alias == alias).cloned();
    let embed = find("mlx-e2e-embed").expect("the embedder is a retrieval model");
    assert_eq!(embed.role, RetrievalRole::Embedding);
    eprintln!("embedder: {embed:?}");
    assert_eq!(
        find("mlx-e2e-rerank")
            .expect("the reranker is one too")
            .role,
        RetrievalRole::Rerank
    );

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
                port: 19882,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    // 1. Embeddings: the first request loads the model.
    let t = Instant::now();
    let res = http
        .post(format!("{base}/v1/embeddings"))
        .json(&json!({"model": "mlx-e2e-embed", "input": "This is an example sentence"}))
        .send()
        .await
        .unwrap();
    let code = res.status();
    let body: Value = res.json().await.unwrap();
    eprintln!("first embeddings request: {code} in {:?}", t.elapsed());
    assert_eq!(code, 200, "{body}");
    let v = body["data"][0]["embedding"].as_array().unwrap();
    assert_eq!(Some(v.len() as u64), embed.dims);
    let norm: f64 = v
        .iter()
        .map(|x| x.as_f64().unwrap().powi(2))
        .sum::<f64>()
        .sqrt();
    assert!((norm - 1.0).abs() < 1e-3, "normalized: {norm}");

    // The RAG client (what knowledge bases use) goes through the same route.
    let endpoint = harness_core::rag::client::Endpoint {
        base_url: format!("{base}/v1"),
        token: None,
    };
    let t = Instant::now();
    let vectors = harness_core::rag::client::embed(
        &endpoint,
        "mlx-e2e-embed",
        &[
            "A dog is chasing a ball in the park".to_string(),
            "A puppy runs after a toy in the garden".to_string(),
            "Quarterly revenue grew by twelve percent".to_string(),
        ],
    )
    .await
    .unwrap();
    eprintln!("three texts embedded in {:?}", t.elapsed());
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let (near, far) = (dot(&vectors[0], &vectors[1]), dot(&vectors[0], &vectors[2]));
    eprintln!("similar {near:.3}, unrelated {far:.3}");
    assert!(near > far + 0.1);

    // 2. Rerank by alias, through the RAG client as well.
    let t = Instant::now();
    let ranked = harness_core::rag::client::rerank(
        &endpoint,
        "mlx-e2e-rerank",
        "How many people live in Berlin?",
        &[
            "The weather is nice today.".to_string(),
            "Berlin had a population of 3,520,031 registered inhabitants in an area of 891.82 square kilometers.".to_string(),
            "Berlin is well known for its museums.".to_string(),
        ],
    )
    .await
    .unwrap();
    eprintln!("rerank in {:?} (load included): {ranked:?}", t.elapsed());
    assert_eq!(ranked[0].0, 1);
    assert_eq!(ranked.last().unwrap().0, 0);

    // 3. Both run as mlx-embed workers, in their serve modes.
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
            harness_core::engine::MLX_EMBED,
            "{id}"
        );
        eprintln!(
            "{id}: {:?} ({:?})",
            cfg.alias,
            pm.active_server().unwrap().serve_mode
        );
    }

    // 4. The wrong endpoint for a model is a clear error.
    let wrong = http
        .post(format!("{base}/v1/rerank"))
        .json(&json!({"model": "mlx-e2e-embed", "query": "q", "documents": ["d"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 501);

    server.stop().await;
    registry.stop_all().await;
}
