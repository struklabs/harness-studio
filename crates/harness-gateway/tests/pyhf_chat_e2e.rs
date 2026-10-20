//! The Python engine's chat models end to end (no mocks): the managed `hf-cpu` environment is built
//! from its hashed lock, a safetensors chat checkpoint (and a vision one) that no mistral.rs build
//! is registered for runs on `python/hf_worker.py`, and `/v1/chat/completions` answers through
//! the gateway: whole and streamed, with a tool call, and with a picture.
//!
//! ```text
//! HS_PYHF_CHAT_MODEL=<.../Qwen/Qwen2.5-0.5B-Instruct> \
//! HS_PYHF_VLM_MODEL=<.../HuggingFaceTB/SmolVLM-256M-Instruct> \
//! HS_PYHF_E2E_ROOT=<empty folder for the data root and the environment> \
//!   cargo test -p harness-gateway --test pyhf_chat_e2e -- --ignored --nocapture
//! ```
//!
//! Both model folders sit under one models directory as `<root>/<owner>/<repo>`. Without
//! `HS_PYHF_E2E_ROOT` the data root is a temporary folder (uv, Python and the packages: about 1.5 GB).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

/// A 128 x 128 PNG, solid red.
const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAIAAAACACAIAAABMXPacAAAAyUlEQVR42u3RQREAAAjDsImYf2WIQQY8clcFTabVYbEAAAABACAAAAQAgAAAEAAAAgBAAAAIAAABACAAAAQAgAAAEAAAAgBAAAAIAAABACAAAAQAgAAAEAAAAgBAAAAIAAABACAAAAQAgAAAEAAAAgBAAAAAcAEAAAEAIAAABACAAAAQAAACAEAAAAgAAAEAIAAABACAAAAQAAACAEAAAAgAAAEAIAAABACAAAAQAAACAEAAAAgAAAEAIAAABACAAAAQAAAC8AFgAWyvA9CBbf8CAAAAAElFTkSuQmCC";

fn model_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// The `data:` payloads of a server-sent event stream, `[DONE]` last.
fn events(text: &str) -> Vec<String> {
    text.split("\n\n")
        .filter_map(|e| e.trim().strip_prefix("data: ").map(str::to_string))
        .collect()
}

#[tokio::test]
#[ignore = "builds the hf-cpu environment (~1.5 GB) and needs HS_PYHF_CHAT_MODEL and HS_PYHF_VLM_MODEL"]
async fn python_chat_models_answer_through_the_gateway() {
    let (Some(chat), Some(vlm)) = (
        model_dir("HS_PYHF_CHAT_MODEL"),
        model_dir("HS_PYHF_VLM_MODEL"),
    ) else {
        eprintln!("HS_PYHF_CHAT_MODEL / HS_PYHF_VLM_MODEL not set, skipping");
        return;
    };
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_PYHF_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("pyhf-chat-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    settings.models_dir = chat
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 11669;
    settings.save().unwrap();

    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("hf-cpu").await.unwrap();
    eprintln!("environment in {:?}: {} bytes", t.elapsed(), env.size_bytes);
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );
    harness_core::engine::python_hf::binary().unwrap();

    // No mistral.rs build is registered here, so the Python engine owns both folders.
    assert!(!harness_core::engine::mistralrs_registered());
    for model in [&chat, &vlm] {
        assert!(harness_core::engine::is_mistralrs_dir(model));
        assert_eq!(
            harness_core::engine::server_for_model(model)
                .unwrap()
                .family(),
            harness_core::engine::PYTHON_HF
        );
    }
    assert_eq!(
        harness_core::engine::python_hf::chat_task(&chat),
        Some("text-generation")
    );
    assert_eq!(
        harness_core::engine::python_hf::chat_task(&vlm),
        Some("image-text-to-text")
    );
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    for model in [&chat, &vlm] {
        let found = library
            .iter()
            .find(|m| &m.path == model)
            .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()));
        assert_eq!(found.kind, harness_core::domain::ModelKind::Llm);
        eprintln!("library: {} {}", found.name, found.size_bytes);
    }

    let store = harness_core::domain::store::Domain::global();
    for (alias, path) in [("pyhf-chat", &chat), ("pyhf-vlm", &vlm)] {
        for old in store.definitions().unwrap() {
            if old.alias == alias {
                store.delete_definition(&old.id).unwrap();
            }
        }
        store
            .save_definition(ModelDefinition::new(
                alias,
                ModelRef::Local { path: path.clone() },
            ))
            .unwrap();
    }

    // Workers take their port from Harness's automatic range (11440-11499), whose first port is
    // the app's own default: a saved profile on 11440 keeps this run off it (nothing listens).
    harness_core::servers::save_profile(harness_core::servers::ServerProfile {
        id: "keep-off-11440".into(),
        name: "Keep Off 11440".into(),
        model_id: None,
        description: String::new(),
        config: harness_core::process::LaunchConfig {
            host: "127.0.0.1".into(),
            port: 11440,
            ..Default::default()
        },
        auto_start: false,
        updated_at: 0,
    })
    .unwrap();

    let registry = Arc::new(ServerRegistry::default());
    // The workers' logs, as they appear (loading a model on a cold disk takes minutes).
    let logs = registry.clone();
    tokio::spawn(async move {
        let mut seen = std::collections::HashMap::<String, u64>::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            for (id, pm) in logs.all() {
                let since = seen.entry(id.clone()).or_default();
                let (next, lines) = pm.get_run_logs_since(*since);
                *since = next;
                for line in lines {
                    eprintln!("[worker {id}] {line}");
                }
            }
        }
    });
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
                port: 11668,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let chat_url = format!("{base}/v1/chat/completions");

    // A whole answer (the first request loads the model).
    let t = Instant::now();
    let res = http
        .post(&chat_url)
        .json(&json!({
            "model": "pyhf-chat",
            "messages": [{"role": "user", "content": "What is the capital of France? Answer in one short sentence."}],
            "max_tokens": 40,
            "temperature": 0
        }))
        .send()
        .await
        .unwrap();
    let code = res.status();
    let body: Value = res.json().await.unwrap();
    eprintln!("whole answer in {:?} (load included): {body}", t.elapsed());
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["object"], "chat.completion");
    let answer = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");
    assert!(answer.to_lowercase().contains("paris"), "{answer:?}");
    assert!(body["usage"]["completion_tokens"].as_u64().unwrap() > 0);
    assert!(body["usage"]["prompt_tokens"].as_u64().unwrap() > 0);

    // A streamed answer: OpenAI chunks, a finish reason, usage on request, [DONE].
    let res = http
        .post(&chat_url)
        .json(&json!({
            "model": "pyhf-chat",
            "messages": [{"role": "user", "content": "Write one sentence of at least ten words about the sea."}],
            "max_tokens": 60,
            "temperature": 0,
            "stream": true,
            "stream_options": {"include_usage": true}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert!(res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream")));
    let text = res.text().await.unwrap();
    let data = events(&text);
    eprintln!(
        "streamed {} events: {}",
        data.len(),
        text.replace('\n', " ")
    );
    assert_eq!(data.last().map(String::as_str), Some("[DONE]"), "{text}");
    let chunks: Vec<Value> = data[..data.len() - 1]
        .iter()
        .map(|d| serde_json::from_str(d).unwrap())
        .collect();
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    let streamed: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert!(streamed.split_whitespace().count() >= 5, "{streamed:?}");
    assert!(chunks
        .iter()
        .any(|c| c["choices"][0]["finish_reason"].is_string()));
    assert!(
        chunks.last().unwrap()["usage"]["total_tokens"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        chunks
            .iter()
            .filter(|c| c["choices"][0]["delta"]["content"].is_string())
            .count()
            > 2
    );

    // A tool call (Qwen's <tool_call> format, parsed into OpenAI tool_calls).
    let tools = json!([{"type": "function", "function": {
        "name": "get_weather",
        "description": "Get the current weather of a city",
        "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}
    }}]);
    let res = http
        .post(&chat_url)
        .json(&json!({
            "model": "pyhf-chat",
            "messages": [{"role": "user", "content": "What is the weather in Ankara right now? Use the tool."}],
            "max_tokens": 120,
            "temperature": 0,
            "tools": tools
        }))
        .send()
        .await
        .unwrap();
    let body: Value = res.json().await.unwrap();
    eprintln!("tool call: {}", body["choices"][0]);
    let call = &body["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(call["function"]["name"], "get_weather", "{body}");
    assert_eq!(body["choices"][0]["finish_reason"], "tool_calls");
    let args: Value =
        serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
    assert!(
        args["city"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("ankara"),
        "{args}"
    );

    // A picture: a data: URL in an image_url part, answered by the vision model.
    let res = http
        .post(&chat_url)
        .json(&json!({
            "model": "pyhf-vlm",
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
    eprintln!("picture: {body}");
    assert_eq!(code, 200, "{body}");
    let answer = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");
    assert!(answer.to_lowercase().contains("red"), "{answer:?}");

    // A text model refuses a picture with a 400, not a crash.
    let res = http
        .post(&chat_url)
        .json(&json!({
            "model": "pyhf-chat",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "What is this?"},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{RED_PNG}")}}
            ]}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());

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
