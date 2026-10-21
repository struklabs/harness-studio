//! A CTranslate2 Whisper export (faster-whisper) end to end, no mocks: the managed `hf-cpu`
//! environment is built from its hashed lock, the export runs on `python/hf_worker.py` and
//! `/v1/audio/transcriptions` answers through the gateway as `json`, `text`, `verbose_json`
//! (with word timestamps), `srt` and `vtt`.
//!
//! ```text
//! HS_CT2_WHISPER_MODEL=<.../Systran/faster-whisper-tiny> \
//! HS_CT2_WHISPER_WAV=<a short English speech WAV> \
//! HS_PYHF_E2E_ROOT=<empty folder for the data root and the environment> \
//!   cargo test -p harness-gateway --test pyhf_whisper_e2e -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::Value;

const BOUNDARY: &str = "harness-e2e-boundary";

/// A multipart/form-data body: the audio as `file`, then text fields.
fn form(audio: &[u8], fields: &[(&str, &str)]) -> Vec<u8> {
    let mut body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(audio);
    body.extend_from_slice(b"\r\n");
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

#[tokio::test]
#[ignore = "builds the hf-cpu environment (~1.5 GB) and needs HS_CT2_WHISPER_MODEL and HS_CT2_WHISPER_WAV"]
async fn a_ctranslate2_whisper_export_transcribes_through_the_gateway() {
    let (Some(model), Some(wav)) = (
        std::env::var_os("HS_CT2_WHISPER_MODEL")
            .map(PathBuf::from)
            .filter(|p| p.is_dir()),
        std::env::var_os("HS_CT2_WHISPER_WAV")
            .map(PathBuf::from)
            .filter(|p| p.is_file()),
    ) else {
        eprintln!("HS_CT2_WHISPER_MODEL / HS_CT2_WHISPER_WAV not set, skipping");
        return;
    };
    let audio = std::fs::read(&wav).unwrap();
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_PYHF_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("pyhf-whisper-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    settings.models_dir = model
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 11681;
    settings.save().unwrap();

    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("hf-cpu").await.unwrap();
    eprintln!("environment in {:?}: {} bytes", t.elapsed(), env.size_bytes);
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );
    harness_core::engine::python_hf::binary().unwrap();

    assert!(harness_core::engine::python_hf::is_ct2_whisper_dir(&model));
    assert_eq!(
        harness_core::engine::python_hf::detect(&model).as_deref(),
        Some("automatic-speech-recognition")
    );
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    let found = library
        .iter()
        .find(|m| m.path == model)
        .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()));
    eprintln!(
        "library: {} {:?} {}",
        found.name, found.kind, found.size_bytes
    );

    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias == "ct2-whisper" {
            store.delete_definition(&old.id).unwrap();
        }
    }
    store
        .save_definition(ModelDefinition::new(
            "ct2-whisper",
            ModelRef::Local {
                path: model.clone(),
            },
        ))
        .unwrap();

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
    let logs = registry.clone();
    tokio::spawn(async move {
        let mut seen = std::collections::HashMap::<String, u64>::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
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
                port: 11680,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    let send = |path: &str, fields: &[(&str, &str)]| {
        let mut all = vec![("model", "ct2-whisper")];
        all.extend_from_slice(fields);
        http.post(format!("{base}{path}"))
            .header("content-type", content_type.clone())
            .body(form(&audio, &all))
            .send()
    };

    // json (the first request loads the model).
    let t = Instant::now();
    let res = send("/v1/audio/transcriptions", &[("language", "en")])
        .await
        .unwrap();
    let code = res.status();
    let body = res.text().await.unwrap();
    eprintln!("json in {:?} (load included): {code} {body}", t.elapsed());
    assert_eq!(code, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    let text = v["text"].as_str().unwrap().to_lowercase();
    assert!(text.contains("hello"), "{text:?}");
    assert!(text.contains("fox"), "{text:?}");
    assert!(v.get("segments").is_none());

    // verbose_json: language, duration, timed segments.
    let res = send(
        "/v1/audio/transcriptions",
        &[("response_format", "verbose_json")],
    )
    .await
    .unwrap();
    let code = res.status();
    let body = res.text().await.unwrap();
    eprintln!("verbose_json: {code} {body}");
    assert_eq!(code, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["task"], "transcribe");
    assert_eq!(v["language"], "english");
    assert!(v["duration"].as_f64().unwrap() > 1.0);
    let segments = v["segments"].as_array().unwrap();
    assert!(!segments.is_empty());
    assert!(segments[0]["end"].as_f64().unwrap() > segments[0]["start"].as_f64().unwrap());
    assert!(v.get("words").is_none());

    // Word timestamps.
    let res = send(
        "/v1/audio/transcriptions",
        &[
            ("response_format", "verbose_json"),
            ("timestamp_granularities[]", "word"),
        ],
    )
    .await
    .unwrap();
    let body = res.text().await.unwrap();
    eprintln!("words: {body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    let words = v["words"].as_array().unwrap();
    assert!(words.len() >= 5);
    assert!(words[0]["word"].is_string() && words[0]["end"].as_f64().unwrap() > 0.0);

    // text, srt, vtt.
    let res = send("/v1/audio/transcriptions", &[("response_format", "text")])
        .await
        .unwrap();
    let text = res.text().await.unwrap();
    eprintln!("text: {text:?}");
    assert!(text.to_lowercase().contains("hello") && text.ends_with('\n'));
    let res = send("/v1/audio/transcriptions", &[("response_format", "srt")])
        .await
        .unwrap();
    let srt = res.text().await.unwrap();
    eprintln!("srt: {srt:?}");
    assert!(srt.starts_with("1\n00:00:"), "{srt:?}");
    let res = send("/v1/audio/transcriptions", &[("response_format", "vtt")])
        .await
        .unwrap();
    let vtt = res.text().await.unwrap();
    eprintln!("vtt: {vtt:?}");
    assert!(vtt.starts_with("WEBVTT"), "{vtt:?}");

    // Whisper translation (English audio stays English) answers its task.
    let res = send(
        "/v1/audio/translations",
        &[("response_format", "verbose_json")],
    )
    .await
    .unwrap();
    let body = res.text().await.unwrap();
    eprintln!("translation: {body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["task"],
        "translate"
    );

    // An unknown format is refused, not served as json.
    let res = send("/v1/audio/transcriptions", &[("response_format", "xml")])
        .await
        .unwrap();
    assert_eq!(res.status(), 400);

    server.stop().await;
    registry.stop_all().await;
}
