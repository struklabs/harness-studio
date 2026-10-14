//! CrispASR route/field allowlist against a private fake worker.
use axum::{
    extract::Multipart,
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

fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn write_gguf(path: &std::path::Path, arch: &str) {
    let mut gguf = b"GGUF".to_vec();
    gguf.extend(3u32.to_le_bytes());
    gguf.extend(0u64.to_le_bytes());
    gguf.extend(1u64.to_le_bytes());
    for (i, s) in ["general.architecture", arch].iter().enumerate() {
        if i == 1 {
            gguf.extend(8u32.to_le_bytes());
        }
        gguf.extend((s.len() as u64).to_le_bytes());
        gguf.extend(s.as_bytes());
    }
    std::fs::write(path, gguf).unwrap();
}

#[tokio::test]
async fn crispasr_allowlist_and_authenticated_multipart() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let model = dir.path().join("data2vec.gguf");
    let mut gguf = b"GGUF".to_vec();
    gguf.extend(3u32.to_le_bytes());
    gguf.extend(0u64.to_le_bytes());
    gguf.extend(1u64.to_le_bytes());
    for (i, s) in ["general.architecture", "wav2vec2"].iter().enumerate() {
        if i == 1 {
            gguf.extend(8u32.to_le_bytes());
        }
        gguf.extend((s.len() as u64).to_le_bytes());
        gguf.extend(s.as_bytes());
    }
    std::fs::write(&model, gguf).unwrap();
    let def = ModelDefinition::new("crisp", ModelRef::Local { path: model });
    assert!(harness_gateway::audio::is_crispasr_model(&def));
    let def = harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();
    let canary = dir.path().join("canary.gguf");
    write_gguf(&canary, "canary");
    let canary_def = harness_core::domain::store::Domain::global()
        .save_definition(ModelDefinition::new(
            "crispc",
            ModelRef::Local { path: canary },
        ))
        .unwrap();
    let translated = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let translated_seen = translated.clone();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = listener.local_addr().unwrap().port();
    let fake = Router::new().route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/props", get(|| async { Json(json!({"total_slots":1})) }))
        .route("/v1/audio/transcriptions", post(move |headers: axum::http::HeaderMap, mut mp: Multipart| { let seen = seen.clone(); async move {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer private-worker-key");
            let mut fields = Vec::new();
            let mut audio = None;
            while let Some(field) = mp.next_field().await.unwrap() {
                let name = field.name().unwrap().to_string();
                if name == "file" { audio = Some(field.bytes().await.unwrap()); }
                else { fields.push((name, field.text().await.unwrap())); }
            }
            assert_eq!(audio.unwrap().as_ref(), b"RIFF0000WAVEdata");
            if let Some(i) = fields.iter().position(|f| f.0 == "translate") {
                assert_eq!(fields.remove(i).1, "true");
                translated_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            let alias = fields.iter().find(|f| f.0 == "model").unwrap().1.clone();
            fields.sort();
            assert_eq!(fields, vec![("language".into(), "tr".into()), ("model".into(), alias), ("prompt".into(), "context".into()), ("response_format".into(), "verbose_json".into()), ("temperature".into(), "0.3".into()), ("timestamp_granularities[]".into(), "word".into())]);
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Json(json!({"text":"a", "duration":1.25,"segments":[{"start":0.1,"end":1.2,"text":"a"}]})).into_response()
        }}));
    let fake = tokio::spawn(async move {
        axum::serve(listener, fake).await.unwrap();
    });
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
    pm.start(LaunchConfig {
        binary_path: bridge,
        model_path: transport,
        definition_id: Some(def.id),
        api_key: Some("private-worker-key".into()),
        host: "127.0.0.1".into(),
        port: port(),
        custom_args: Some(format!("--bridge-target {target}")),
        ..Default::default()
    })
    .await
    .unwrap();
    let pm2 = registry.get_or_create("canary-transport");
    pm2.start(LaunchConfig {
        binary_path: dir
            .path()
            .join(format!("bridge{}", std::env::consts::EXE_SUFFIX)),
        model_path: dir.path().join("transport.gguf"),
        definition_id: Some(canary_def.id),
        api_key: Some("private-worker-key".into()),
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
        for _ in 0..200 { if pm.get_status() == ServerStatus::Running { break; } tokio::time::sleep(std::time::Duration::from_millis(50)).await; }
        assert_eq!(pm.get_status(), ServerStatus::Running, "{:?}", pm.get_logs(Some(20)));
        let status = gateway.start(GatewayContext { registry:registry.clone(), runtime:Arc::default(), gate:Arc::default(), data_dir:dir.path().into(), downloads:None },
            &harness_core::gateway::GatewaySettings { enabled:true, port:port(), ..Default::default() }).await.unwrap();
        let base = status.endpoint.unwrap().host_url;
        let http = reqwest::Client::new();
        let (ct, mut body) = harness_core::asr::onnx::multipart(b"RIFF0000WAVEdata", "verbose_json", "crisp");
        let boundary = ct.split("boundary=").nth(1).unwrap();
        body.truncate(body.len() - format!("--{boundary}--\r\n").len());
        for (k, v) in [("language", "tr"), ("prompt", "context"), ("temperature", "0.3"), ("timestamp_granularities[]", "word")] {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        for _ in 0..200 { if pm2.get_status() == ServerStatus::Running { break; } tokio::time::sleep(std::time::Duration::from_millis(50)).await; }
        // `translate` is the gateway's own field, never a caller's.
        let mut rejected = body.clone();
        rejected.truncate(rejected.len() - format!("--{boundary}--\r\n").len());
        rejected.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"translate\"\r\n\r\ntrue\r\n--{boundary}--\r\n").as_bytes());
        let res = http.post(format!("{base}/v1/audio/transcriptions")).header("content-type", &ct).body(rejected).send().await.unwrap();
        assert_eq!(res.status(), 400, "translate field");
        // A model without translation is refused before anything is forwarded.
        let res = http.post(format!("{base}/v1/audio/translations")).header("content-type", &ct).body(body.clone()).send().await.unwrap();
        assert_eq!(res.status(), 400, "wav2vec2 cannot translate");
        // Reject fields before a worker can see them.
        for field in ["diarize_embedder", "lid_model", "punc_model", "truecase_model", "model_path", "voice", "unknown", "speakers"] {
            let mut rejected = body.clone();
            rejected.truncate(rejected.len() - format!("--{boundary}--\r\n").len());
            rejected.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"\r\n\r\n1\r\n--{boundary}--\r\n").as_bytes());
            let res = http.post(format!("{base}/v1/audio/transcriptions")).header("content-type", &ct).body(rejected).send().await.unwrap();
            assert_eq!(res.status(), 400, "{field}");
        }
        for endpoint in ["/api/chat", "/api/generate", "/api/embed", "/v1/chat/completions", "/v1/embeddings", "/v1/audio/speech"] {
            let res = http.post(format!("{base}{endpoint}")).json(&json!({"model":"crisp","messages":[{"role":"user","content":"hello"}],"prompt":"hello","input":"hello","stream":false})).send().await.unwrap();
            assert_eq!(res.status(), 400, "{endpoint}: {}", res.text().await.unwrap());
        }
        for endpoint in ["/load", "/inference", "/backends"] {
            let res = http.post(format!("{base}{endpoint}")).json(&json!({"model":"crisp"})).send().await.unwrap();
            assert!(!res.status().is_success(), "{endpoint}");
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let res = http.post(format!("{base}/v1/audio/transcriptions")).header("content-type", &ct).body(body.clone()).send().await.unwrap();
        let code = res.status(); let v: Value = res.json().await.unwrap();
        assert_eq!(code, 200, "{v}"); assert_eq!(v["segments"][0]["start"], 0.1); assert_eq!(v["duration"], 1.25);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(translated.load(std::sync::atomic::Ordering::SeqCst), 0);
        // A translating model: translations become a transcriptions request with translate=true.
        let canary_body = String::from_utf8_lossy(&body).replace("name=\"model\"\r\n\r\ncrisp\r\n", "name=\"model\"\r\n\r\ncrispc\r\n").into_bytes();
        let res = http.post(format!("{base}/v1/audio/translations")).header("content-type", &ct).body(canary_body).send().await.unwrap();
        let code = res.status(); let text = res.text().await.unwrap();
        assert_eq!(code, 200, "{text}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(translated.load(std::sync::atomic::Ordering::SeqCst), 1);
    }).catch_unwind().await;
    gateway.stop().await;
    registry.stop_all().await;
    fake.abort();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
