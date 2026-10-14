//! MLX speech recognition (mlx-audio) end to end, on Apple Silicon (no mocks): the managed
//! `mlx-vlm` environment (which carries mlx-audio) is built from its hashed lock, the library
//! finds an MLX Whisper / Parakeet folder, and `/v1/audio/transcriptions` goes through the
//! gateway to the worker it starts. The speech to recognize is made with macOS's `say`.
//!
//! ```text
//! HS_MLX_STT_MODEL=<an mlx-community recognition folder, e.g. whisper-tiny-asr-fp16> \
//!   cargo test -p harness-gateway --test mlx_stt_e2e -- --ignored --nocapture
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

/// About ten seconds of English: Whisper tiny is reliable on it (a two-second clip is not).
const SENTENCE: &str = "Harness Studio runs local models on this computer. It can load a model, answer a question, and turn speech into text. The quick brown fox jumps over the lazy dog.";

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
#[ignore = "builds the mlx-vlm environment (~800 MB) and needs HS_MLX_STT_MODEL"]
async fn an_mlx_recognition_model_transcribes_through_the_gateway() {
    let Some(model) = std::env::var_os("HS_MLX_STT_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        eprintln!("HS_MLX_STT_MODEL not set, skipping");
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
            .prefix(&harness_core::paths::test_dir_prefix("mlx-stt-e2e"))
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
    settings.control_api.port = 19892;
    settings.save().unwrap();

    // 16 kHz 16-bit WAV of a known sentence, in an English voice (the system's default voice
    // may be another language's).
    let wav = dir.join("speech.wav");
    let said = std::process::Command::new("say")
        .args(["-v", "Samantha", "-o"])
        .arg(&wav)
        .args(["--data-format=LEI16@16000", SENTENCE])
        .status()
        .unwrap();
    assert!(said.success());
    let audio = std::fs::read(&wav).unwrap();

    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("mlx-vlm").await.unwrap();
    eprintln!("environment in {:?}: {} bytes", t.elapsed(), env.size_bytes);
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );

    // The library lists the folder as an mlx-audio recognition model; no other engine takes it.
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    let found = library
        .iter()
        .find(|m| m.path == model)
        .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()));
    eprintln!(
        "library: {} {:?} {} bytes",
        found.name, found.architecture, found.size_bytes
    );
    assert_eq!(found.kind, harness_core::domain::ModelKind::Pipeline);
    assert_eq!(found.task.as_deref(), Some("automatic-speech-recognition"));
    assert!(harness_core::engine::mlx::is_stt_dir(&model));
    assert!(!harness_core::engine::is_mistralrs_dir(&model));
    assert!(harness_core::engine::python_hf::detect(&model).is_none());
    let whisper = found.architecture.as_deref() == Some("whisper");

    let def = ModelDefinition::new(
        "mlx-stt-e2e",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias.starts_with("mlx-stt-e2e") {
            store.delete_definition(&old.id).unwrap();
        }
    }
    store.save_definition(def).unwrap();

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
                port: 19882,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    let post = |url: String, body: Vec<u8>| {
        http.post(url)
            .header("content-type", content_type.clone())
            .body(body)
            .send()
    };
    let send = |path: &str, fields: &[(&str, &str)]| {
        let mut all = vec![("model", "mlx-stt-e2e")];
        all.extend_from_slice(fields);
        post(format!("{base}{path}"), form(&audio, &all))
    };

    let t = Instant::now();
    let res = send("/v1/audio/transcriptions", &[]).await.unwrap();
    let code = res.status();
    let body = res.text().await.unwrap();
    eprintln!(
        "first answer in {:?} (load included): {code} {body}",
        t.elapsed()
    );
    assert_eq!(code, 200, "{body}");
    let text = serde_json::from_str::<serde_json::Value>(&body).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_lowercase();
    for word in [
        "harness",
        "local models",
        "speech into text",
        "brown fox",
        "lazy dog",
    ] {
        assert!(text.contains(word), "{word} missing in {text:?}");
    }
    // Nothing made up in the silence after the last word.
    assert!(text.ends_with("lazy dog."), "{text:?}");

    let t = Instant::now();
    let res = send(
        "/v1/audio/transcriptions",
        &[("response_format", "text"), ("language", "en")],
    )
    .await
    .unwrap();
    assert_eq!(res.status(), 200);
    let plain = res.text().await.unwrap();
    eprintln!("text answer in {:?}: {plain:?}", t.elapsed());
    assert!(plain.to_lowercase().contains("lazy dog") && plain.ends_with('\n'));

    // Subtitles are whisper.cpp's; this worker answers text only.
    let res = send("/v1/audio/transcriptions", &[("response_format", "srt")])
        .await
        .unwrap();
    assert_eq!(res.status(), 400);

    // Whisper translates (to English, here from English); Parakeet refuses.
    let res = send("/v1/audio/translations", &[]).await.unwrap();
    let code = res.status();
    let body = res.text().await.unwrap();
    eprintln!("translation: {code} {body}");
    if whisper {
        assert_eq!(code, 200, "{body}");
        assert!(body.to_lowercase().contains("lazy dog"), "{body}");
    } else {
        assert_eq!(code, 400, "{body}");
    }

    // Not audio: refused, the worker keeps serving.
    let res = post(
        format!("{base}/v1/audio/transcriptions"),
        form(b"RIFF\0\0\0\0WAVEjunk", &[("model", "mlx-stt-e2e")]),
    )
    .await
    .unwrap();
    let code = res.status();
    eprintln!("junk: {code} {}", res.text().await.unwrap());
    assert!(code.is_client_error(), "{code}");
    let res = send("/v1/audio/transcriptions", &[]).await.unwrap();
    assert_eq!(res.status(), 200);

    // The worker's own port refuses a request without its key, and a page of another origin.
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
    let res = http
        .post(format!("http://127.0.0.1:{worker}/v1/audio/transcriptions"))
        .header("origin", "https://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);

    server.stop().await;
    registry.stop_all().await;
}
