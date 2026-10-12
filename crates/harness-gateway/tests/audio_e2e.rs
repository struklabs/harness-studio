//! Speech to text through the gateway with a real whisper-server and a real model (no mocks):
//!
//! ```text
//! HS_WHISPER_SERVER=<whisper-server.exe> HS_WHISPER_MODEL=<ggml-*.bin> HS_WHISPER_WAV=<speech.wav>
//! HS_WHISPER_TEXT=<words the wav says> [HS_WHISPER_WEBM=<webm/opus speech, needs ffmpeg>]
//! cargo test -p harness-gateway --test audio_e2e -- --ignored --nocapture
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::Value;

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

fn form(parts: &[(&str, Option<&str>, Vec<u8>)]) -> (String, Vec<u8>) {
    let boundary = "----hs-e2e-boundary";
    let mut body = Vec::new();
    for (name, file, data) in parts {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"").as_bytes(),
        );
        if let Some(f) = file {
            body.extend_from_slice(
                format!("; filename=\"{f}\"\r\nContent-Type: application/octet-stream").as_bytes(),
            );
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn post(
    http: &reqwest::Client,
    base: &str,
    path: &str,
    parts: &[(&str, Option<&str>, Vec<u8>)],
) -> (u16, reqwest::header::HeaderMap, String) {
    let (ct, body) = form(parts);
    let r = http
        .post(format!("{base}{path}"))
        .header("content-type", ct)
        .body(body)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let headers = r.headers().clone();
    (status, headers, r.text().await.unwrap())
}

#[tokio::test]
#[ignore]
async fn e2e_transcription_jit_ttl_formats() {
    let (Some(server), Some(model), Some(wav)) = (
        env_path("HS_WHISPER_SERVER"),
        env_path("HS_WHISPER_MODEL"),
        env_path("HS_WHISPER_WAV"),
    ) else {
        eprintln!("HS_WHISPER_SERVER / HS_WHISPER_MODEL / HS_WHISPER_WAV not set, skipping");
        return;
    };
    let want = std::env::var("HS_WHISPER_TEXT")
        .unwrap_or_else(|_| "hello".into())
        .to_lowercase();

    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("gw-audio"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.server.port = 18810;
    s.control_api.port = 18819;
    s.save().unwrap();

    // The whisper runtime, registered the way a user would (a local folder).
    let entry = harness_core::runtimes::RuntimeRegistry::global()
        .add_local(&harness_core::runtimes::AddLocal {
            path: server.parent().unwrap().to_path_buf(),
            name: "whisper e2e".into(),
            id: None,
            copy: false,
            family: "whisper.cpp".into(),
        })
        .unwrap();
    eprintln!(
        "runtime: {} family={} backend={:?}",
        entry.id, entry.family, entry.backend
    );

    let mut def = ModelDefinition::new(
        "gw-whisper",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    def.ttl_seconds = Some(8);
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();
    // A chat model that is not a speech model: llama.cpp is not needed to check the refusal.
    std::fs::write(dir.join("x.gguf"), b"GGUF").unwrap();
    let chat_def = ModelDefinition::new(
        "gw-notspeech",
        ModelRef::Local {
            path: dir.join("x.gguf"),
        },
    );
    harness_core::domain::store::Domain::global()
        .save_definition(chat_def)
        .unwrap();

    let registry = Arc::new(ServerRegistry::default());
    let ctx = GatewayContext {
        registry: registry.clone(),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: dir.clone(),
        downloads: None,
    };
    let gw = GatewayServer::new();
    let status = gw
        .start(
            ctx,
            &GatewaySettings {
                enabled: true,
                port: 18810,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let audio = std::fs::read(&wav).unwrap();
    let running = || {
        registry
            .all()
            .into_iter()
            .filter(|(_, pm)| pm.get_status() == harness_core::process::ServerStatus::Running)
            .count()
    };
    assert_eq!(running(), 0);

    // JIT load + default json.
    let t = Instant::now();
    let (st, h, body) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &[
            ("model", None, b"gw-whisper".to_vec()),
            ("file", Some("speech.wav"), audio.clone()),
        ],
    )
    .await;
    eprintln!("first request {st} in {:?}: {body}", t.elapsed());
    assert_eq!(st, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    let text = v["text"].as_str().unwrap();
    assert!(!text.starts_with(' '));
    assert!(text.to_lowercase().contains(&want), "{text} vs {want}");
    assert!(
        h.contains_key("x-harness-load-ms"),
        "the model was loaded just in time"
    );
    assert_eq!(running(), 1);

    // Warm request, every format.
    for (fmt, needle) in [
        ("text", want.as_str()),
        ("srt", "-->"),
        ("vtt", "WEBVTT"),
        ("verbose_json", "segments"),
    ] {
        let (st, _, body) = post(
            &http,
            &base,
            "/v1/audio/transcriptions",
            &[
                ("model", None, b"gw-whisper".to_vec()),
                ("response_format", None, fmt.as_bytes().to_vec()),
                ("language", None, b"en".to_vec()),
                ("file", Some("speech.wav"), audio.clone()),
            ],
        )
        .await;
        eprintln!(
            "{fmt}: {st} {}",
            body.chars()
                .take(160)
                .collect::<String>()
                .replace('\n', "\\n")
        );
        assert_eq!(st, 200, "{fmt}: {body}");
        assert!(
            body.to_lowercase().contains(&needle.to_lowercase()),
            "{fmt}: {body}"
        );
    }

    // Translations (English audio: the transcript comes back unchanged in English).
    let (st, _, body) = post(
        &http,
        &base,
        "/v1/audio/translations",
        &[
            ("model", None, b"gw-whisper".to_vec()),
            ("file", Some("speech.wav"), audio.clone()),
        ],
    )
    .await;
    eprintln!("translations: {st} {body}");
    // The tiny.en model is English-only and refuses to translate; multilingual models answer.
    assert!(st == 200 || st == 400 || st == 502, "{st} {body}");

    // Errors.
    let (st, _, body) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &[("model", None, b"gw-whisper".to_vec())],
    )
    .await;
    assert_eq!(st, 400, "{body}");
    assert!(body.contains("file"), "{body}");
    let (st, _, body) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &[
            ("model", None, b"gw-notspeech".to_vec()),
            ("file", Some("speech.wav"), audio.clone()),
        ],
    )
    .await;
    assert_eq!(st, 400, "{body}");
    assert!(body.contains("model_not_speech"), "{body}");
    let (st, _, body) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &[
            ("model", None, b"gw-whisper".to_vec()),
            ("response_format", None, b"nope".to_vec()),
            ("file", Some("speech.wav"), audio.clone()),
        ],
    )
    .await;
    assert_eq!(st, 400, "{body}");
    // Chat on a speech model.
    let r = http
        .post(format!("{base}/v1/chat/completions"))
        .json(
            &serde_json::json!({"model":"gw-whisper","messages":[{"role":"user","content":"hi"}]}),
        )
        .send()
        .await
        .unwrap();
    let st = r.status().as_u16();
    let body = r.text().await.unwrap();
    eprintln!("chat on a speech model: {st} {body}");
    assert_eq!(st, 400, "{body}");

    // Container conversion through ffmpeg.
    if let Some(webm) = env_path("HS_WHISPER_WEBM") {
        let (st, _, body) = post(
            &http,
            &base,
            "/v1/audio/transcriptions",
            &[
                ("model", None, b"gw-whisper".to_vec()),
                ("file", Some("speech.webm"), std::fs::read(webm).unwrap()),
            ],
        )
        .await;
        eprintln!("webm: {st} {body}");
        if harness_core::audio::ffmpeg_path().is_some() {
            assert_eq!(st, 200, "{body}");
            assert!(body.to_lowercase().contains(&want), "{body}");
        } else {
            assert_eq!(st, 415, "{body}");
        }
    }

    // TTL: the idle model is unloaded.
    let deadline = Instant::now() + Duration::from_secs(40);
    while running() > 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert_eq!(running(), 0, "the TTL unloaded the whisper model");
    eprintln!("whisper model unloaded after its TTL");

    // And it loads again on the next request.
    let (st, h, _) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &[
            ("model", None, b"gw-whisper".to_vec()),
            ("file", Some("speech.wav"), audio.clone()),
        ],
    )
    .await;
    assert_eq!(st, 200);
    assert!(h.contains_key("x-harness-load-ms"));

    gw.stop().await;
    registry.stop_all().await;
    assert_eq!(running(), 0);
}
