//! The MLX audio engine (mlx-audio) end to end, on Apple Silicon (no mocks): the managed
//! `mlx-vlm` environment (which carries mlx-audio) is built from its hashed lock, the library
//! finds an mlx-audio text-to-speech folder, and `/v1/audio/speech` goes through the gateway to
//! the worker it starts.
//!
//! ```text
//! HS_MLX_TTS_MODEL=<an mlx-community Qwen3-TTS folder, e.g. Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit> \
//!   cargo test -p harness-gateway --test mlx_audio_e2e -- --ignored --nocapture
//! ```
//!
//! `HS_MLX_E2E_ROOT=<folder>` keeps the data root (and the environment) between runs; by
//! default it is a temporary folder (uv, Python and the packages: about 800 MB).

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::json;

#[tokio::test]
#[ignore = "builds the mlx-vlm environment (~800 MB) and needs HS_MLX_TTS_MODEL"]
async fn an_mlx_audio_model_speaks_through_the_gateway() {
    let Some(model) = std::env::var_os("HS_MLX_TTS_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        eprintln!("HS_MLX_TTS_MODEL not set, skipping");
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
            .prefix(&harness_core::paths::test_dir_prefix("mlx-audio-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    // `<root>/<owner>/<repo>`, as a download lays it out.
    settings.models_dir = model
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 19891;
    settings.save().unwrap();

    // The engine is missing until its environment is built (a kept root may have it already).
    if let Err(missing) = harness_core::engine::mlx::audio_binary() {
        assert!(
            missing.contains("runtime.family_not_installed"),
            "{missing}"
        );
    }
    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("mlx-vlm").await.unwrap();
    eprintln!(
        "environment in {:?}: {} bytes, {:?}",
        t.elapsed(),
        env.size_bytes,
        env.manifest.as_ref().map(|m| &m.python)
    );
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );
    harness_core::engine::mlx::audio_binary().unwrap();

    // The library lists the folder as an mlx-audio pipeline model (not as a chat model).
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    let found = library
        .iter()
        .find(|m| m.path == model)
        .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()));
    eprintln!(
        "library: {} {} {}",
        found.name, found.quantization, found.size_bytes
    );
    assert_eq!(found.kind, harness_core::domain::ModelKind::Pipeline);
    assert!(harness_core::engine::mlx::is_tts_dir(&model));
    assert!(!harness_core::engine::mlx::is_model_dir(&model));
    assert!(!harness_core::engine::is_mistralrs_dir(&model));

    let def = ModelDefinition::new(
        "mlx-tts-e2e",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    // The same folder with a 5 s time limit (`--max-seconds`).
    let mut limited = ModelDefinition::new(
        "mlx-tts-e2e-limited",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    limited.load.insert(
        "max_seconds".into(),
        harness_core::params::ParamValue::Int(5),
    );
    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias.starts_with("mlx-tts-e2e") {
            store.delete_definition(&old.id).unwrap();
        }
    }
    store.save_definition(def).unwrap();
    store.save_definition(limited).unwrap();

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
                port: 19881,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    let speak = |voice: &str, format: &str| {
        http.post(format!("{base}/v1/audio/speech")).json(&json!({
            "model": "mlx-tts-e2e",
            "input": "Harness speaks with MLX audio on this Mac.",
            "voice": voice,
            "response_format": format
        }))
    };
    let t = Instant::now();
    let res = speak("ryan", "wav").send().await.unwrap();
    let code = res.status();
    let kind = res
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string());
    let audio = res.bytes().await.unwrap();
    eprintln!(
        "first answer in {:?} (load included): {code} {kind:?} {} bytes",
        t.elapsed(),
        audio.len()
    );
    assert_eq!(code, 200, "{}", String::from_utf8_lossy(&audio));
    assert_eq!(kind.as_deref(), Some("audio/wav"));
    assert!(audio.starts_with(b"RIFF"), "not a WAV");
    // 16-bit mono at 24 kHz: more than a second of speech.
    assert!(audio.len() > 48_000, "{} bytes", audio.len());

    // An OpenAI client's voice name gets the model's default voice; another speaker is refused.
    let t = Instant::now();
    let res = speak("alloy", "wav").send().await.unwrap();
    assert_eq!(res.status(), 200);
    eprintln!("second answer in {:?}", t.elapsed());
    let res = speak("bob", "wav").send().await.unwrap();
    let code = res.status();
    let text = res.text().await.unwrap();
    eprintln!("unknown voice: {code} {text}");
    assert_eq!(code, 400, "{text}");
    assert!(text.contains("serena"), "{text}");

    // A short text never runs away: sampling can miss the end token, the frame cap (here the
    // 75-frame floor, 6 s) stops it. 16-bit mono at 24 kHz: 48 000 bytes a second.
    for round in 0..5 {
        let res = http
            .post(format!("{base}/v1/audio/speech"))
            .json(&json!({ "model": "mlx-tts-e2e", "input": "Hello from Harness Studio on a Mac.", "voice": "ryan", "response_format": "wav" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let seconds = (res.bytes().await.unwrap().len() - 44) as f64 / 48_000.0;
        eprintln!("short text, round {round}: {seconds:.2} s of audio");
        assert!(
            seconds <= 6.5,
            "{seconds} s of audio for a 35-character text"
        );
    }

    // A long text under a 5 s time limit: the audio made by then, marked as cut.
    let long = "Harness Studio runs local models on this computer. ".repeat(12);
    let t = Instant::now();
    let res = http
        .post(format!("{base}/v1/audio/speech"))
        .json(&json!({ "model": "mlx-tts-e2e-limited", "input": long, "voice": "ryan", "response_format": "wav" }))
        .send()
        .await
        .unwrap();
    let code = res.status();
    let cut = res.headers().get("x-harness-truncated").cloned();
    let audio = res.bytes().await.unwrap();
    eprintln!(
        "long text with a 5 s limit: {code} {cut:?} {} bytes in {:?} (load included)",
        audio.len(),
        t.elapsed()
    );
    assert_eq!(code, 200, "{}", String::from_utf8_lossy(&audio));
    assert_eq!(cut.as_ref().map(|v| v.to_str().unwrap()), Some("time"));
    assert!(audio.starts_with(b"RIFF") && audio.len() > 48_000);

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
