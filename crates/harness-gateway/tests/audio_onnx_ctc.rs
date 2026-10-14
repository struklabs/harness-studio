//! Gateway routing against a managed loopback bridge and fake CTC sidecar.
mod common;

use axum::{
    extract::Multipart,
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
use std::sync::Arc;

#[tokio::test]
async fn ctc_multipart_routes_to_sidecar_and_preserves_segments() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let package = dir.path().join("speech");
    std::fs::create_dir_all(package.join("onnx")).unwrap();
    for (name, value) in [
        (
            "config.json",
            json!({"architectures":["Wav2Vec2ForCTC"],"vocab_size":3,"pad_token_id":0}),
        ),
        (
            "preprocessor_config.json",
            json!({"sampling_rate":16000,"feature_size":1}),
        ),
        ("vocab.json", json!({"<pad>":0,"a":1,"|":2})),
    ] {
        std::fs::write(package.join(name), value.to_string()).unwrap();
    }
    let graph = package.join("onnx/model.onnx");
    std::fs::write(&graph, b"graph").unwrap();
    let def = ModelDefinition::new("ctc", ModelRef::Local { path: graph });
    assert!(harness_gateway::audio::is_speech_model(&def));
    assert!(!harness_gateway::audio::is_speech_model(
        &ModelDefinition::new(
            "embed",
            ModelRef::Local {
                path: dir.path().join("embed.onnx")
            }
        )
    ));
    assert!(harness_gateway::audio::is_speech_model(
        &ModelDefinition::new(
            "whisper",
            ModelRef::Local {
                path: dir.path().join("ggml-base.bin")
            }
        )
    ));
    let def = harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = listener.local_addr().unwrap().port();
    let fake = Router::new().route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/props", get(|| async { Json(json!({"total_slots":1})) }))
        .route("/v1/audio/transcriptions", post(move |mut mp: Multipart| { let seen = seen.clone(); async move {
            let (mut file, mut model) = (None, None);
            let mut format = "json".to_string();
            while let Some(field) = mp.next_field().await.unwrap() {
                match field.name().unwrap_or_default() {
                    "file" => file = Some(field.bytes().await.unwrap()),
                    "model" => model = Some(field.text().await.unwrap()),
                    "response_format" => format = field.text().await.unwrap(),
                    _ => {}
                }
            }
            // Match server.rs::transcription_upload: a file and nonblank model are
            // required, and response_format must be one of the supported values.
            if file.is_none() || model.as_deref().is_none_or(|m| m.trim().is_empty())
                || !matches!(format.as_str(), "json" | "verbose_json" | "text" | "srt" | "vtt") {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid multipart"}))).into_response();
            }
            assert_eq!(file.unwrap().as_ref(), b"RIFF0000WAVEdata");
            assert_eq!(model.as_deref(), Some("ctc"));
            assert_eq!(format, "verbose_json");
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Json(json!({"text":"a", "duration":1.25,"segments":[{"start":0.1,"end":1.2,"text":"a"}]})).into_response()
        }}));
    let fake = tokio::spawn(async move {
        axum::serve(listener, fake).await.unwrap();
    });
    // Prove the fake rejects the old request that omitted model, as the sidecar does.
    let missing_model = "--upload\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n\r\nRIFF0000WAVEdata\r\n--upload--\r\n";
    let rejected = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{target}/v1/audio/transcriptions"))
        .header("content-type", "multipart/form-data; boundary=upload")
        .body(missing_model)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    // The registry owns the bridge; only its transport is exercised, no ONNX library needed.
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
    let pm = registry.get_or_create("ctc-transport");
    common::start_process(
        &pm,
        LaunchConfig {
            binary_path: bridge,
            model_path: transport,
            definition_id: Some(def.id),
            host: "127.0.0.1".into(),
            port: 0,
            custom_args: Some(format!("--bridge-target {target}")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let gateway = GatewayServer::new();
    use futures_util::FutureExt;
    let result = std::panic::AssertUnwindSafe(async {
        for _ in 0..200 { if pm.get_status() == ServerStatus::Running { break; } tokio::time::sleep(std::time::Duration::from_millis(50)).await; }
        assert_eq!(pm.get_status(), ServerStatus::Running, "{:?}", pm.get_logs(Some(20)));
        let status = gateway.start_ephemeral(GatewayContext { registry:registry.clone(), runtime:Arc::default(), gate:Arc::default(), data_dir:dir.path().into(), downloads:None },
            &harness_core::gateway::GatewaySettings { enabled:true, port:0, ..Default::default() }).await.unwrap();
        let base = status.endpoint.unwrap().host_url;
        let http = reqwest::Client::new();
        let (ct, mut body) = harness_core::asr::onnx::multipart(b"RIFF0000WAVEdata", "verbose_json", "ctc");
        let boundary = ct.split("boundary=").nth(1).unwrap();
        body.truncate(body.len() - format!("--{boundary}--\r\n").len());
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\ntr\r\n--{boundary}--\r\n").as_bytes());
        let res = http.post(format!("{base}/v1/audio/transcriptions")).header("content-type", &ct).body(body.clone()).send().await.unwrap();
        let code = res.status(); let v: Value = res.json().await.unwrap();
        assert_eq!(code, 200, "{v}"); assert_eq!(v["segments"][0]["start"], 0.1); assert_eq!(v["duration"], 1.25);
        let res = http.post(format!("{base}/v1/audio/translations")).header("content-type", &ct).body(body).send().await.unwrap();
        assert_eq!(res.status(), 400);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }).catch_unwind().await;
    gateway.stop().await;
    registry.stop_all().await;
    fake.abort();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
