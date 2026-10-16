//! Gateway, auth, scheduler and multipart/JSON boundary against a fake worker.
mod common;
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    process::LaunchConfig,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn gguf(path: &std::path::Path, family: &str) {
    use std::io::Write;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap();
    f.write_all(&2u64.to_le_bytes()).unwrap();
    for (key, value) in [
        ("general.architecture", "audiocpp"),
        ("audiocpp.model_spec.family", family),
    ] {
        f.write_all(&(key.len() as u64).to_le_bytes()).unwrap();
        f.write_all(key.as_bytes()).unwrap();
        f.write_all(&8u32.to_le_bytes()).unwrap();
        f.write_all(&(value.len() as u64).to_le_bytes()).unwrap();
        f.write_all(value.as_bytes()).unwrap();
    }
}

#[tokio::test]
async fn processing_rebuilds_both_client_formats_and_sanitizes_worker_results() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().into());
    harness_core::config::AppSettings {
        models_dir: dir.path().to_string_lossy().into_owned(),
        audio_cpp_enabled: true,
        ..Default::default()
    }
    .save()
    .unwrap();
    let model = dir.path().join("sam.gguf");
    gguf(&model, "sam_audio");
    let def = harness_core::domain::store::Domain::global()
        .save_definition(ModelDefinition::new("sam", ModelRef::Local { path: model }))
        .unwrap();
    let wav = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../harness-core/tests/data/audio/hello.wav"),
    )
    .unwrap();
    let encoded = STANDARD.encode(&wav);
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let calls = seen.clone();
    let response_audio = encoded.clone();
    let fake = Router::new().route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/props", get(|| async { Json(json!({"total_slots":1})) }))
        .route("/v1/tasks/run", post(move |Json(body): Json<Value>| {
            let calls = calls.clone(); let audio = response_audio.clone();
            async move {
                let attempt = {
                    let mut calls = calls.lock().unwrap();
                    calls.push(body.clone());
                    calls.iter().filter(|call| call["request"]["text"] == body["request"]["text"]).count()
                };
                let prompt = body["request"]["text"].as_str().unwrap();
                if prompt == "oom-twice" || (prompt == "oom-once" && attempt == 1) {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":{"message":"SAM Audio decoder graph allocation failed"}}))).into_response();
                }
                match body["request"]["text"].as_str().unwrap() {
                    "unsafe" => Json(json!({"artifacts":[{"id":"target","path":"C:/secret.wav"}]})),
                    "empty" => Json(json!({"timing":{"path":"never return"}})),
                    "malformed" => Json(json!({"audio":"not base64"})),
                    "enhance" => Json(json!({"audio":audio,"path":"never return","timing":{"wall_ms":1}})),
                    _ => Json(json!({"named_audio_outputs":[{"id":"target","audio":audio},{"id":"residual","audio":audio}],"path":"never return"})),
                }.into_response()
            }
        }));
    let port = harness_core::engine::audio_cpp::random_port().unwrap();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    let fake = tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });
    let bridge = dir
        .path()
        .join(format!("bridge{}", std::env::consts::EXE_SUFFIX));
    assert!(harness_core::process::quiet_command("rustc")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/support/upstream-bridge.rs")
        )
        .arg("-o")
        .arg(&bridge)
        .status()
        .unwrap()
        .success());
    let transport = dir.path().join("transport.gguf");
    std::fs::write(&transport, b"GGUF").unwrap();
    let registry = Arc::new(ServerRegistry::default());
    let pm = registry.get_or_create("process-transport");
    common::start_process(
        &pm,
        LaunchConfig {
            binary_path: bridge.clone(),
            model_path: transport.clone(),
            definition_id: Some(def.id.clone()),
            host: "127.0.0.1".into(),
            custom_args: Some(format!("--bridge-target {port}")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let gateway = GatewayServer::new();
    use futures_util::FutureExt;
    let result = std::panic::AssertUnwindSafe(async {
        let status = gateway.start_ephemeral(GatewayContext { registry: registry.clone(), runtime: Arc::default(), gate: Arc::default(), data_dir: dir.path().into(), downloads: None }, &harness_core::gateway::GatewaySettings { enabled: true, port: 0, ..Default::default() }).await.unwrap();
        let base = status.endpoint.unwrap().host_url;
        let servers_before = registry.all().len();
        let url = format!("{base}/v1/audio/process"); let http = reqwest::Client::new();
        let response = http.post(&url).json(&json!({"model":"sam","audio_base64":format!("data:audio/wav;base64,{encoded}"),"prompt":"man speaking","seed":7})).send().await.unwrap();
        assert_eq!(response.status(), 200); let body: Value = response.json().await.unwrap();
        assert_eq!(body["model"], "sam"); assert_eq!(body["outputs"].as_array().unwrap().len(), 2);
        assert_eq!(body["outputs"][0], json!({"name":"target","audio_base64":encoded,"mime":"audio/wav"}));
        assert!(!body.to_string().contains("never return"));
        let call = seen.lock().unwrap()[0].clone(); assert_eq!(call["model"], "sam");
        assert_eq!(call["request"]["text"], "man speaking"); assert_eq!(call["request"]["options"], json!({"seed":7}));
        assert_eq!(call["request"].as_object().unwrap().len(), 3);
        let mut form = b"--process-test\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nsam\r\n--process-test\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\nenhance\r\n--process-test\r\nContent-Disposition: form-data; name=\"seed\"\r\n\r\n9\r\n--process-test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"../../secret.wav\"\r\nContent-Type: audio/wav\r\n\r\n".to_vec();
        form.extend_from_slice(&wav); form.extend_from_slice(b"\r\n--process-test--\r\n");
        let response = http.post(&url).header("content-type", "multipart/form-data; boundary=process-test").body(form).send().await.unwrap(); assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap(); assert_eq!(body["outputs"][0]["name"], "enhanced"); assert!(!body.to_string().contains("path"));
        let count = seen.lock().unwrap().len();
        for extra in [json!({"unknown":"x"}), json!({"options":{"path":"secret"}}), json!({"seed":-9}), json!({"prompt":""}), json!({"audio_base64":"!"})] {
            let mut request = json!({"model":"sam","audio_base64":encoded,"prompt":"man speaking"}); request.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            assert_eq!(http.post(&url).json(&request).send().await.unwrap().status(), 400);
        }
        for field in ["audio", "output_dir", "voice_ref", "path", "unknown", "guidance_scale"] {
            let mut request = json!({"model":"sam","audio_base64":encoded,"prompt":"man speaking"});
            request[field] = json!("C:/Windows/win.ini");
            let response = http.post(&url).json(&request).send().await.unwrap();
            assert_eq!(response.status(), 400);
            let error = response.text().await.unwrap();
            assert!(error.contains(field));
            assert!(!error.contains("C:/Windows/win.ini"));
            let mut form = format!("--reject\r\nContent-Disposition: form-data; name=\"{field}\"\r\n\r\nC:/Windows/win.ini\r\n--reject\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nsam\r\n--reject\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\nman speaking\r\n--reject\r\nContent-Disposition: form-data; name=\"file\"; filename=\"input.wav\"\r\nContent-Type: audio/wav\r\n\r\n").into_bytes();
            form.extend_from_slice(&wav);
            form.extend_from_slice(b"\r\n--reject--\r\n");
            let response = http.post(&url).header("content-type", "multipart/form-data; boundary=reject").body(form).send().await.unwrap();
            assert_eq!(response.status(), 400);
            let error = response.text().await.unwrap();
            assert!(error.contains(field));
            assert!(!error.contains("C:/Windows/win.ini"));
        }
        assert_eq!(seen.lock().unwrap().len(), count);
        for prompt in ["unsafe", "empty", "malformed"] {
            let response = http.post(&url).json(&json!({"model":"sam","audio_base64":encoded,"prompt":prompt})).send().await.unwrap();
            assert_eq!(response.status(), 502); assert!(!response.text().await.unwrap().contains("secret"));
        }
        for path in ["/v1/tasks/run", "/v1/tasks/batch", "/v1/tasks/stream", "/v1/tasks/unload_all", "/v1/models/load", "/v1/models/unload", "/v1/ui/config"] {
            assert!(http.post(format!("{base}{path}")).json(&json!({"model":"sam","audio":"secret"})).send().await.unwrap().status().is_client_error());
        }
        assert_eq!(registry.all().len(), servers_before);
        let scheduler = gateway.scheduler().await.unwrap();
        for (prompt, expected_status) in [("oom-once", 200), ("oom-twice", 502)] {
            // An idle, gateway-managed audio worker is the memory recovery victim.
            let idle_model = dir.path().join(format!("{prompt}.gguf"));
            gguf(&idle_model, "sam_audio");
            let idle_def = harness_core::domain::store::Domain::global()
                .save_definition(ModelDefinition::new(prompt, ModelRef::Local { path: idle_model }))
                .unwrap();
            // Refresh the scheduler's short definitions cache for this new companion.
            scheduler.resolve(Some(prompt)).await.unwrap();
            let idle_pm = registry.get_or_create(&format!("gw-{prompt}"));
            common::start_process(&idle_pm, LaunchConfig {
                binary_path: bridge.clone(), model_path: transport.clone(),
                definition_id: Some(idle_def.id.clone()), host: "127.0.0.1".into(),
                custom_args: Some(format!("--bridge-target {port}")), ..Default::default()
            }).await.unwrap();
            let lease = scheduler.acquire_audio_process(&idle_def).await.unwrap();
            assert!(!scheduler.unload_if_idle(&idle_def.id).await.unwrap());
            assert_eq!(idle_pm.get_status(), harness_core::process::ServerStatus::Running);
            drop(lease);
            let response = http.post(&url).json(&json!({"model":"sam","audio_base64":encoded,"prompt":prompt})).send().await.unwrap();
            assert_eq!(response.status(), expected_status);
            if expected_status == 502 {
                let error: Value = response.json().await.unwrap();
                assert_eq!(error["error"]["harness"]["error_key"], "audio_process.gpu_memory");
            }
            assert_eq!(seen.lock().unwrap().iter().filter(|call| call["request"]["text"] == prompt).count(), 2);
            assert_ne!(idle_pm.get_status(), harness_core::process::ServerStatus::Running);
        }
        harness_core::config::AppSettings { audio_cpp_enabled: false, ..Default::default() }.save().unwrap();
        assert_eq!(http.post(&url).json(&json!({"model":"sam","audio_base64":encoded,"prompt":"man speaking"})).send().await.unwrap().status(), 403);
    }).catch_unwind().await;
    gateway.stop().await;
    registry.stop_all().await;
    fake.abort();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
