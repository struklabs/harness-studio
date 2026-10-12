//! The Lemonade engine through the real gateway (no mocks): the release is downloaded from
//! GitHub and verified, `lemond` is started on a port Harness picks, a small llama.cpp model
//! is pulled through Lemonade's API, and `/v1/chat/completions` / `/v1/embeddings` by the
//! model's Lemonade name go through the gateway to it.
//!
//! ```text
//! cargo test -p harness-gateway --test lemonade_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Downloads: Lemonade (5 MB), its Vulkan llama.cpp build (35 MB, fetched by `lemond` on the
//! first request), Qwen3-0.6B-GGUF (0.4 GB) and nomic-embed-text-v1-GGUF (80 MB). Temporary data
//! root (never the user's); needs a GPU with a Vulkan driver, or set
//! `HS_LEMONADE_BACKEND=cpu`. The NPU path (FastFlowLM) needs Ryzen AI hardware and is not
//! exercised here: on this kind of machine the test asserts it is reported as absent.

use std::sync::Arc;

use harness_core::gateway::GatewaySettings;
use harness_core::lemonade::{self, InstallRequest, LemonadeSettings};
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const CHAT_MODEL: &str = "Qwen3-0.6B-GGUF";
const EMBED_MODEL: &str = "nomic-embed-text-v1-GGUF";

#[tokio::test]
#[ignore = "downloads Lemonade, a llama.cpp backend and two models; needs a GPU or HS_LEMONADE_BACKEND=cpu"]
async fn install_start_pull_and_chat_through_the_gateway() {
    harness_core::secrets::use_memory_store();
    // A short folder: Lemonade's Hugging Face cache layout makes long paths longer still.
    let dir = tempfile::Builder::new()
        .prefix("hs-lem-")
        .tempdir_in(std::env::temp_dir())
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut app = harness_core::config::AppSettings::load();
    app.control_api.port = 19989;
    app.save().unwrap();

    // 1. Install: the release archive is checked against GitHub's SHA-256.
    let pick = lemonade::preview_release(None).await.unwrap();
    eprintln!("release {} {:?}", pick.tag, pick.assets[0].name);
    assert!(pick.assets[0].name.contains("embeddable"));
    assert!(pick.unverified.is_empty(), "the archive has a digest");
    let entry = lemonade::install(InstallRequest {
        tag: Some(pick.tag.clone()),
    })
    .await
    .unwrap();
    eprintln!("installed {} {:?}", entry.id, entry.version);
    assert_eq!(entry.family, "lemonade");
    assert_eq!(entry.version.as_deref(), pick.tag.strip_prefix('v'));

    let status = lemonade::status();
    assert_eq!(status.installs.len(), 1);
    assert!(status.installs[0].active);
    assert!(!status.running);
    eprintln!("npu: {:?}", status.npu);
    let backend = std::env::var("HS_LEMONADE_BACKEND").unwrap_or_else(|_| "vulkan".into());
    lemonade::save_settings(LemonadeSettings {
        backend: backend.clone(),
        runtime: None,
    })
    .await
    .unwrap();

    // 2. Start: loopback, a Harness-picked port, a key.
    let t = std::time::Instant::now();
    let ep = lemonade::start().await.unwrap();
    eprintln!("lemond up in {:?} at {}", t.elapsed(), ep.base);
    assert!(ep.base.starts_with("http://127.0.0.1:114"), "{}", ep.base);
    let http = reqwest::Client::new();
    let no_key = http
        .get(format!("{}/v1/models", ep.base))
        .send()
        .await
        .unwrap();
    assert_eq!(
        no_key.status(),
        401,
        "the API is locked with the per-run key"
    );
    let live = http.get(format!("{}/live", ep.base)).send().await.unwrap();
    assert_eq!(live.status(), 200);

    // 3. The registry; the NPU backend is not available on a machine without an NPU.
    let list = lemonade::list_models().await.unwrap();
    assert!(list.live && list.catalog.models.len() > 20);
    assert!(list.catalog.models.iter().any(|m| m.id == CHAT_MODEL));
    let flm = list
        .catalog
        .backends
        .iter()
        .find(|b| b.recipe == "flm")
        .expect("the FastFlowLM backend is listed");
    if !lemonade::status().npu.detected {
        assert_eq!(flm.state, "unsupported", "{flm:?}");
        assert!(
            !list.catalog.models.iter().any(|m| m.recipe == "flm"),
            "no NPU models are offered without an NPU"
        );
    }

    // 4. Pull through Lemonade, with progress.
    for model in [CHAT_MODEL, EMBED_MODEL] {
        let pulling = tokio::spawn(async move { lemonade::pull_model(model).await });
        let mut saw_progress = false;
        while !pulling.is_finished() {
            if let Some(p) = lemonade::pull_progress() {
                saw_progress |= p.bytes_downloaded > 0;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        pulling.await.unwrap().unwrap();
        eprintln!("pulled {model} (progress seen: {saw_progress})");
    }
    let models = lemonade::list_models().await.unwrap().catalog.models;
    assert!(
        models
            .iter()
            .find(|m| m.id == CHAT_MODEL)
            .unwrap()
            .downloaded
    );
    let served: Vec<String> = lemonade::gateway_models()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert!(served.contains(&CHAT_MODEL.to_string()), "{served:?}");
    assert!(served.contains(&EMBED_MODEL.to_string()), "{served:?}");

    // 5. The gateway: listed by Lemonade name, chat and embeddings route to lemond.
    let registry = Arc::new(ServerRegistry::default());
    let server = GatewayServer::new();
    let started = server
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
                port: 19980,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = started.endpoint.unwrap().host_url;
    let listed: Value = http
        .get(format!("{base}/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(
        ids.contains(&CHAT_MODEL) && ids.contains(&EMBED_MODEL),
        "{ids:?}"
    );

    let t = std::time::Instant::now();
    let res = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": CHAT_MODEL,
            "messages": [{"role": "user", "content": "Reply with the single word: pong. /no_think"}],
            "max_tokens": 200
        }))
        .send()
        .await
        .unwrap();
    let status = res.status();
    let server_header = res
        .headers()
        .get("x-harness-server")
        .map(|v| v.to_str().unwrap().to_string());
    let body: Value = res.json().await.unwrap();
    eprintln!("chat: {status} in {:?}: {body}", t.elapsed());
    assert_eq!(status, 200, "{body}");
    assert_eq!(server_header.as_deref(), Some("lemonade"));
    let msg = &body["choices"][0]["message"];
    let text = format!("{}{}", msg["content"], msg["reasoning_content"]);
    assert!(text.len() > 4, "the model answered: {text}");
    assert!(body["usage"]["completion_tokens"].as_u64().unwrap_or(0) > 0);

    // The same model streams.
    let stream = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": CHAT_MODEL, "stream": true, "max_tokens": 30,
            "messages": [{"role": "user", "content": "Count: one two three"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), 200);
    let text = stream.text().await.unwrap();
    assert!(text.contains("data: [DONE]"), "{text}");

    // Embeddings by the other Lemonade model.
    let res = http
        .post(format!("{base}/v1/embeddings"))
        .json(&json!({"model": EMBED_MODEL, "input": ["search_query: hello"]}))
        .send()
        .await
        .unwrap();
    let status = res.status();
    let emb: Value = res.json().await.unwrap();
    assert_eq!(status, 200, "{emb}");
    let dims = emb["data"][0]["embedding"].as_array().unwrap().len();
    eprintln!("embedding: {dims} dimensions");
    assert!(dims >= 256);

    // A model Lemonade does not have is the usual unknown-model error, not a hang.
    let missing = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": "no-such-model", "messages": [{"role": "user", "content": "x"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    // 6. The backend Lemonade ran is the one the setting asked for.
    let log = lemonade::log_tail(200).join("\n");
    assert!(
        log.contains(&format!("Using LlamaCpp Backend: {backend}")),
        "{log}"
    );

    // 7. Stopping ends lemond and its llama-server.
    server.stop().await;
    lemonade::stop().await.unwrap();
    assert!(lemonade::endpoint().is_none());
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let gone = http.get(format!("{}/live", ep.base)).send().await;
    assert!(gone.is_err(), "lemond no longer answers");
}
