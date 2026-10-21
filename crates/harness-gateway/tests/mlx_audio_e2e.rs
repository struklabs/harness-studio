//! The MLX audio engine (mlx-audio) end to end, on Apple Silicon (no mocks): the managed
//! `mlx-vlm` environment (which carries mlx-audio) is built from its hashed lock, the library
//! finds an mlx-audio text-to-speech folder, and `/v1/audio/speech` goes through the gateway to
//! the worker it starts.
//!
//! ```text
//! HS_MLX_TTS_MODEL=<an mlx-community text-to-speech folder of a tested family, e.g.
//!   Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit, chatterbox-turbo-4bit, Soprano-80M-bf16,
//!   VoxCPM2-4bit, Spark-TTS-0.5B-bf16> \
//!   cargo test -p harness-gateway --test mlx_audio_e2e -- --ignored --nocapture
//! ```
//!
//! With `HS_MLX_STT_MODEL=<an MLX Whisper folder in the same models folder>` the first answer
//! is also transcribed back and must say what was asked.
//!
//! With `HS_MLX_TTS_CLONES=1` the model must clone a voice from a reference recording (the first
//! answer, sent inline with its transcript): e.g. Qwen3-TTS-12Hz-0.6B-Base-8bit,
//! chatterbox-turbo-4bit. Without it, a cloning request must be refused.
//!
//! `HS_MLX_E2E_ROOT=<folder>` keeps the data root (and the environment) between runs; by
//! default it is a temporary folder (uv, Python and the packages: about 800 MB).

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use base64::{engine::general_purpose::STANDARD, Engine as _};
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

    // Qwen3-TTS has named speakers; the other families speak with one default voice.
    let voices = harness_core::engine::mlx::tts_voices(&model);
    let voice = voices.first().cloned();
    eprintln!("voices: {voices:?}");
    let stt = std::env::var_os("HS_MLX_STT_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_dir());
    if let Some(stt) = &stt {
        store_definition("mlx-stt-e2e-check", stt);
    }
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

    fn store_definition(alias: &str, path: &std::path::Path) {
        let store = harness_core::domain::store::Domain::global();
        for old in store.definitions().unwrap() {
            if old.alias == alias {
                store.delete_definition(&old.id).unwrap();
            }
        }
        store
            .save_definition(ModelDefinition::new(
                alias,
                ModelRef::Local {
                    path: path.to_path_buf(),
                },
            ))
            .unwrap();
    }

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

    let speak = |voice: Option<&str>, format: &str| {
        let mut body = json!({
            "model": "mlx-tts-e2e",
            "input": "Harness speaks with MLX audio on this Mac.",
            "response_format": format
        });
        if let Some(voice) = voice {
            body["voice"] = json!(voice);
        }
        http.post(format!("{base}/v1/audio/speech")).json(&body)
    };
    let t = Instant::now();
    let res = speak(voice.as_deref(), "wav").send().await.unwrap();
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
    // 16-bit mono at the model's rate: more than a second of speech.
    let bytes_per_second = 2.0 * u32::from_le_bytes(audio[24..28].try_into().unwrap()) as f64;
    let seconds = (audio.len() - 44) as f64 / bytes_per_second;
    eprintln!("{seconds:.2} s of audio at {} Hz", bytes_per_second / 2.0);
    assert!(seconds > 1.5, "{seconds} s of audio");

    if stt.is_some() {
        const BOUNDARY: &str = "harness-e2e-boundary";
        let mut body = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(&audio);
        body.extend_from_slice(
            format!("\r\n--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nmlx-stt-e2e-check\r\n--{BOUNDARY}--\r\n").as_bytes(),
        );
        let res = http
            .post(format!("{base}/v1/audio/transcriptions"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(body)
            .send()
            .await
            .unwrap();
        let code = res.status();
        let heard = res.text().await.unwrap();
        eprintln!("heard back: {code} {heard}");
        assert_eq!(code, 200, "{heard}");
        // Most of the sentence is heard back; a small model may slur a name ("Harness").
        let heard = heard.to_lowercase();
        let words = [
            "harness", "speaks", "with", "mlx", "audio", "on", "this", "mac",
        ];
        let score = words.iter().filter(|w| heard.contains(*w)).count();
        eprintln!("words heard back: {score}/{}", words.len());
        assert!(score >= 5, "{heard}");
    }

    // The first answer, sent back inline as a reference recording with its transcript, clones
    // its voice on a model that clones (HS_MLX_TTS_CLONES=1); another model refuses it. A
    // reference that names a file is refused either way.
    let clone = |reference: serde_json::Value| {
        http.post(format!("{base}/v1/audio/speech")).json(&json!({
            "model": "mlx-tts-e2e",
            "input": "This voice was cloned from a short recording.",
            "voice_ref": reference,
            "voice_ref_text": "Harness speaks with MLX audio on this Mac.",
            "voice_consent": true
        }))
    };
    let t = Instant::now();
    let res = clone(json!({"type": "base64", "data": STANDARD.encode(&audio)}))
        .send()
        .await
        .unwrap();
    let code = res.status();
    let cloned = res.bytes().await.unwrap();
    eprintln!(
        "cloned answer in {:?}: {code} {} bytes",
        t.elapsed(),
        cloned.len()
    );
    if std::env::var_os("HS_MLX_TTS_CLONES").is_some() {
        assert_eq!(code, 200, "{}", String::from_utf8_lossy(&cloned));
        assert!(cloned.starts_with(b"RIFF"), "not a WAV");
        let seconds = (cloned.len() - 44) as f64 / bytes_per_second;
        eprintln!("{seconds:.2} s of cloned audio");
        assert!(seconds > 1.5, "{seconds} s of cloned audio");
    } else {
        let text = String::from_utf8_lossy(&cloned);
        assert_eq!(code, 400, "{text}");
        assert!(text.contains("does not clone voices"), "{text}");
    }
    let res = clone(json!({"type": "path", "data": "/etc/hosts"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);

    // An OpenAI client's voice name gets the model's default voice; another speaker is refused
    // by a model that has named ones.
    let t = Instant::now();
    let res = speak(Some("alloy"), "wav").send().await.unwrap();
    assert_eq!(res.status(), 200);
    eprintln!("second answer in {:?}", t.elapsed());
    if let Some(last) = voices.last() {
        let res = speak(Some("bob"), "wav").send().await.unwrap();
        let code = res.status();
        let text = res.text().await.unwrap();
        eprintln!("unknown voice: {code} {text}");
        assert_eq!(code, 400, "{text}");
        assert!(text.contains(last.as_str()), "{text}");
    }

    // A short text never runs away: sampling can miss the end token, the token cap (here the
    // floor, 6 s of audio) stops it.
    let mut short = json!({ "model": "mlx-tts-e2e", "input": "Hello from Harness Studio on a Mac.", "response_format": "wav" });
    let mut long_request = json!({ "model": "mlx-tts-e2e-limited", "response_format": "wav" });
    if let Some(voice) = &voice {
        short["voice"] = json!(voice);
        long_request["voice"] = json!(voice);
    }
    for round in 0..5 {
        let res = http
            .post(format!("{base}/v1/audio/speech"))
            .json(&short)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let seconds = (res.bytes().await.unwrap().len() - 44) as f64 / bytes_per_second;
        eprintln!("short text, round {round}: {seconds:.2} s of audio");
        assert!(
            seconds <= 6.5,
            "{seconds} s of audio for a 35-character text"
        );
    }

    // A long text under a 5 s time limit: the audio made by then, marked as cut.
    long_request["input"] = json!("Harness Studio runs local models on this computer. ".repeat(12));
    let t = Instant::now();
    let res = http
        .post(format!("{base}/v1/audio/speech"))
        .json(&long_request)
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
    // Some audio, however little: a model slower than real time (CSM on an M1 makes about a
    // third of a second a second, after a few seconds on its voice prompt) has made under a
    // second of it by then.
    assert!(audio.starts_with(b"RIFF") && audio.len() as f64 > 44.0 + bytes_per_second / 10.0);

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
