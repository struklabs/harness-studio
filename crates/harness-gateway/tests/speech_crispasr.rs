//! Security boundary tests, independent of model downloads or installed runtimes.
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_gateway::crispasr_tts::{body, rebuild};
use serde_json::json;

#[test]
fn preset_voices_never_become_client_paths() {
    let voices = vec!["af_heart".into()];
    for voice in [
        "C:/Windows/win.ini",
        "C:\\Windows\\win.ini",
        "../af_heart",
        "..",
        "/etc/passwd",
        "\\\\server\\share",
        "af_heart.gguf",
        "other",
        "af_heart\0",
        "data:audio/wav;base64,UklGRg==",
    ] {
        assert_eq!(
            rebuild(
                "alias",
                &voices,
                true,
                &serde_json::to_vec(&json!({"input":"Hello", "voice":voice})).unwrap()
            )
            .unwrap_err()
            .status,
            400,
            "{voice}"
        );
    }
    let result = rebuild(
        "worker",
        &voices,
        true,
        br#"{"model":"alias","input":"Hello","voice":"af_heart","speed":1.25}"#,
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"model":"worker","input":"Hello","response_format":"wav","speed":1.25})
    );
    assert_eq!(
        rebuild(
            "worker",
            &["alba".into()],
            false,
            br#"{"input":"Hello","voice":"alba"}"#
        )
        .unwrap()["voice"],
        "alba"
    );
}

#[test]
fn strict_fields_and_size_limits() {
    let voices = vec!["af_heart".into()];
    for field in [
        "consent_attestation",
        "voice_ref",
        "instructions",
        "stream",
        "phonemes",
        "language",
        "duration",
        "model_path",
        "spoken_disclaimer",
    ] {
        let mut req = json!({"input":"Hello"});
        req[field] = json!("anything");
        assert_eq!(
            rebuild("alias", &voices, true, &serde_json::to_vec(&req).unwrap())
                .unwrap_err()
                .status,
            400,
            "{field}"
        );
    }
    for req in [
        json!({"input":"x".repeat(4001)}),
        json!({"input":""}),
        json!({"input":false}),
        json!({"input":"hi","speed":0.24}),
        json!({"input":"hi","speed":4.01}),
        json!({"input":"hi","speed":"1"}),
        json!({"input":"hi","voice":{"type":"base64","data":"A".repeat(7*1024*1024)}}),
        json!({"input":"hi","response_format":"f32"}),
        json!({"input":"hi","model":"C:/model.gguf"}),
    ] {
        assert_eq!(
            rebuild("alias", &voices, true, &serde_json::to_vec(&req).unwrap())
                .unwrap_err()
                .status,
            400
        );
    }
    assert!(rebuild(
        "alias",
        &voices,
        true,
        &serde_json::to_vec(&json!({"input":"ğ".repeat(4000)})).unwrap()
    )
    .is_ok());
    assert!(rebuild(
        "alias",
        &["alba".into()],
        false,
        br#"{"input":"hi","speed":1.1}"#
    )
    .is_err());
    assert!(rebuild("alias", &[], true, br#"{"input":"hi"}"#).is_err());
}

#[test]
fn every_other_worker_surface_is_closed() {
    let def = ModelDefinition::new(
        "alias",
        ModelRef::Local {
            path: "unused.gguf".into(),
        },
    );
    for path in [
        "/v1/chat/completions",
        "/v1/completions",
        "/v1/embeddings",
        "/api/generate",
        "/api/chat",
        "/api/embed",
        "/v1/audio/transcriptions",
        "/v1/audio/translations",
        "/v1/voices",
        "/v1/audio/voices",
        "/load",
        "/health",
        "/v1/audio/speech/",
        "/v1/audio/speech?voice=file",
    ] {
        assert_eq!(body(&def, "POST", path, b"{}").unwrap_err().status, 400);
    }
    for method in ["GET", "PUT", "DELETE", "PATCH", "OPTIONS"] {
        assert_eq!(
            body(&def, method, "/v1/audio/speech", b"{}")
                .unwrap_err()
                .status,
            400
        );
    }
}

#[tokio::test]
async fn http_rejections_never_start_a_worker() {
    use harness_core::{gateway::GatewaySettings, servers::ServerRegistry};
    use harness_gateway::{GatewayContext, GatewayServer};
    use std::sync::Arc;
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let path = dir.path().join("kokoro.gguf");
    let mut bytes = b"GGUF".to_vec();
    bytes.extend(3u32.to_le_bytes());
    bytes.extend(0u64.to_le_bytes());
    bytes.extend(1u64.to_le_bytes());
    for (i, value) in ["general.architecture", "kokoro"].iter().enumerate() {
        if i == 1 {
            bytes.extend(8u32.to_le_bytes());
        }
        bytes.extend((value.len() as u64).to_le_bytes());
        bytes.extend(value.as_bytes());
    }
    std::fs::write(&path, bytes).unwrap();
    let mut definition = ModelDefinition::new("crisp-tts", ModelRef::Local { path });
    definition.pinned = true;
    let def = harness_core::domain::store::Domain::global()
        .save_definition(definition)
        .unwrap();
    let registry = Arc::new(ServerRegistry::default());
    let gw = GatewayServer::new();
    let settings = GatewaySettings {
        port: std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port(),
        require_token: false,
        ..Default::default()
    };
    let ctx = GatewayContext {
        registry: registry.clone(),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: dir.path().into(),
        downloads: None,
    };
    let status = gw.start(ctx, &settings).await.unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    for endpoint in [
        "/v1/chat/completions",
        "/v1/completions",
        "/v1/embeddings",
        "/v1/responses",
        "/api/chat",
        "/api/generate",
        "/api/embed",
        "/api/show",
        "/load",
        "/v1/voices",
    ] {
        let res = http
            .post(format!("{base}{endpoint}"))
            .json(&json!({"model":"crisp-tts","input":"hello","messages":[],"prompt":"hello"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            400,
            "{endpoint}: {}",
            res.text().await.unwrap()
        );
    }
    for req in [
        json!({"model":"crisp-tts","input":"hi","voice":"C:/Windows/win.ini"}),
        json!({"model":"crisp-tts","input":"hi","consent_attestation":"yes"}),
        json!({"model":"crisp-tts","input":"x".repeat(4001)}),
        json!({"model":"crisp-tts","input":"hi","voice":"af_heart"}),
    ] {
        let res = http
            .post(format!("{base}/v1/audio/speech"))
            .json(&req)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    }
    let (ct, body) = harness_core::asr::onnx::multipart(b"RIFF0000WAVEdata", "json", "crisp-tts");
    let res = http
        .post(format!("{base}/v1/audio/transcriptions"))
        .header("content-type", ct)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let scheduler = gw.scheduler().await.unwrap();
    assert_eq!(
        scheduler
            .acquire_asr(&def)
            .await
            .err()
            .unwrap()
            .http_status(),
        400
    );
    assert_eq!(
        scheduler
            .acquire_tokens(&def, None)
            .await
            .err()
            .unwrap()
            .http_status(),
        400
    );
    assert!(
        registry
            .all()
            .iter()
            .all(|(_, worker)| worker.launch_config().is_none()),
        "A rejected request configured a worker"
    );
    gw.stop().await;
}
