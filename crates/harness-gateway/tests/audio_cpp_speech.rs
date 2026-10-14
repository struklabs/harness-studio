//! Real gateway and scheduler against a fake resident audio.cpp HTTP server.
//! Only the byte transport is a managed test child; no model runtime is installed.
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    process::{LaunchConfig, ServerStatus},
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

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

fn wav() -> Vec<u8> {
    let mut b = b"RIFF".to_vec();
    b.extend(38u32.to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(24000u32.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(2u32.to_le_bytes());
    b.extend(0i16.to_le_bytes());
    b
}

#[tokio::test]
async fn audio_cpp_tts_uses_resident_speech_endpoint_and_never_routes_other_tasks() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().into());
    harness_core::config::AppSettings {
        audio_cpp_enabled: true,
        ..Default::default()
    }
    .save()
    .unwrap();
    let package = dir.path().join("english");
    std::fs::create_dir_all(package.join("embeddings")).unwrap();
    let model = package.join("pocket.gguf");
    gguf(&model, "pocket_tts");
    std::fs::write(package.join("embeddings/alba.safetensors"), b"embedding").unwrap();
    let domain = harness_core::domain::store::Domain::global();
    let def = domain
        .save_definition(ModelDefinition::new(
            "pocket",
            ModelRef::Local {
                path: model.clone(),
            },
        ))
        .unwrap();
    assert_eq!(
        harness_core::engine::server_for_model(&model)
            .unwrap()
            .family(),
        "audio.cpp"
    );
    assert!(!harness_core::engine::is_llama_server(&model, false));
    let other = dir.path().join("asr.gguf");
    gguf(&other, "sensevoice");
    domain
        .save_definition(ModelDefinition::new("asr", ModelRef::Local { path: other }))
        .unwrap();

    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = calls.clone();
    let audio = wav();
    let expected = audio.clone();
    let fake = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"status":"ok","backend":"vulkan","models":1})) }),
        )
        .route("/props", get(|| async { Json(json!({"total_slots":1})) }))
        .route(
            "/v1/audio/speech",
            post(move |Json(v): Json<Value>| {
                let seen = seen.clone();
                let audio = audio.clone();
                async move {
                    if v.get("speed").is_some() {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":{"message":"PocketTTS framework session does not support runtime style controls"}}))).into_response();
                    }
                    if v["input"] == "fail" {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":{"message":"synthesis failed"}}))).into_response();
                    }
                    if v["input"] == "timeout" {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        return StatusCode::GATEWAY_TIMEOUT.into_response();
                    }
                    if v["model"] != "pocket" || v["response_format"] != "wav" {
                        return StatusCode::BAD_REQUEST.into_response();
                    }
                    seen.lock().unwrap().push(v);
                    ([("content-type", "audio/wav")], audio).into_response()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = listener.local_addr().unwrap().port();
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
    let pm = registry.get_or_create("audio-transport");
    pm.start(LaunchConfig {
        binary_path: bridge,
        model_path: transport,
        definition_id: Some(def.id),
        host: "127.0.0.1".into(),
        port: port(),
        custom_args: Some(format!("--bridge-target {target}")),
        ..Default::default()
    })
    .await
    .unwrap();
    let gateway = GatewayServer::new();
    use futures_util::FutureExt;
    let result = std::panic::AssertUnwindSafe(async {
        for _ in 0..200 {
            if pm.get_status() == ServerStatus::Running {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(
            pm.get_status(),
            ServerStatus::Running,
            "{:?}",
            pm.get_logs(Some(20))
        );
        let status = gateway
            .start(
                GatewayContext {
                    registry: registry.clone(),
                    runtime: Arc::default(),
                    gate: Arc::default(),
                    data_dir: dir.path().into(),
                    downloads: None,
                },
                &harness_core::gateway::GatewaySettings {
                    enabled: true,
                    port: port(),
                    request_timeout_secs: 1,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let url = format!("{}/v1/audio/speech", status.endpoint.unwrap().host_url);
        let http = reqwest::Client::new();
        // Two calls lease the same resident server. The second omits voice and uses alba.
        for voice in [Some("alba"), None] {
            let mut request = json!({"model":"pocket","input":"Merhaba","response_format":"wav"});
            if let Some(voice) = voice {
                request["voice"] = json!(voice);
                request["speed"] = json!(1.0);
            }
            let response = http.post(&url).json(&request).send().await.unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["content-type"], "audio/wav");
            assert_eq!(response.headers()["x-harness-model"], "pocket");
            assert_eq!(response.bytes().await.unwrap().as_ref(), expected);
        }
        let response = http
            .post(&url)
            .json(&json!({"model":"pocket","input":"Hi","speed":1.25,"response_format":"wav"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["param"], "speed");
        let response = http
            .post(&url)
            .json(&json!({"model":"pocket","input":"fail","response_format":"wav"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 502);
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["message"], "audio.cpp: synthesis failed");
        let response = http
            .post(&url)
            .json(&json!({"model":"pocket","input":"timeout","response_format":"wav"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 504);
        let error: Value = response.json().await.unwrap();
        assert_eq!(
            error["error"]["message"],
            "The audio.cpp server did not answer in time."
        );
        let response = http
            .post(&url)
            .json(&json!({"model":"asr","input":"Hello","response_format":"wav"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
        for extra in [
            json!({"voice_ref":"secret.wav"}),
            json!({"voice_ref":{"type":"path","path":"secret.wav"}}),
            json!({"voice":"../secret"}),
            json!({"voice_ref":{"type":"base64","data":"!"}}),
        ] {
            let mut request = json!({"model":"pocket","input":"Hi","response_format":"wav"});
            request
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert_eq!(
                http.post(&url)
                    .json(&request)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                400
            );
        }
        let base = url.strip_suffix("/v1/audio/speech").unwrap();
        for path in [
            "/v1/tasks/run",
            "/v1/tasks/batch",
            "/v1/models/load",
            "/v1/audio/alignments",
            "/v1/chat/completions",
        ] {
            let response = http
                .post(format!("{base}{path}"))
                .json(&json!({"model":"pocket","messages":[],"audio":"secret.wav"}))
                .send()
                .await
                .unwrap();
            assert!(
                response.status().is_client_error(),
                "{path}: {}",
                response.status()
            );
        }
        harness_core::config::AppSettings {
            audio_cpp_enabled: false,
            ..Default::default()
        }
        .save()
        .unwrap();
        let response = http
            .post(&url)
            .json(&json!({"model":"pocket","input":"Hi","response_format":"wav"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403);
        assert!(response.text().await.unwrap().contains("Settings"));
        let refused = registry.get_or_create("disabled-audio");
        let config = LaunchConfig {
            model_path: model.clone(),
            ..Default::default()
        };
        assert!(refused
            .start(config.clone())
            .await
            .unwrap_err()
            .contains("Settings"));
        assert_eq!(refused.get_status(), ServerStatus::Stopped);
        assert!(harness_core::orchestrator::launch(
            &registry,
            &Default::default(),
            &Default::default(),
            "disabled-load".into(),
            config
        )
        .await
        .unwrap_err()
        .contains("Settings"));
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        for call in calls.iter() {
            assert_eq!(call["input"], "Merhaba");
            assert_eq!(call["voice"], "alba");
        }
    })
    .catch_unwind()
    .await;
    gateway.stop().await;
    registry.stop_all().await;
    fake.abort();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
