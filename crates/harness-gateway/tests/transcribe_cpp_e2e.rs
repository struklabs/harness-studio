//! transcribe.cpp speech recognition end to end (no mocks): the managed `transcribe-cpp-cuda`
//! environment (with an NVIDIA driver; `transcribe-cpp` otherwise) is built from its hashed
//! lock, the library finds a transcribe.cpp GGUF, and `/v1/audio/transcriptions` goes through
//! the gateway to the worker it starts. The speech to recognize is made with the system voice
//! (Windows' System.Speech, macOS' `say`).
//!
//! ```text
//! HS_TRANSCRIBE_CPP_MODEL=<models>/handy-computer/parakeet-tdt_ctc-110m-gguf/parakeet-tdt_ctc-110m-Q4_K_M.gguf \
//!   cargo test -p harness-gateway --test transcribe_cpp_e2e -- --ignored --nocapture
//! ```
//!
//! `HS_TRANSCRIBE_CPP_E2E_ROOT=<folder>` keeps the data root (and the environment) between
//! runs; by default it is a temporary folder (uv, Python and the packages: about 60 MB, or
//! about 1.3 GB for the CUDA build).

#![cfg(any(windows, target_os = "macos"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};

/// About ten seconds of English.
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

/// A 16 kHz mono 16-bit WAV of `text` in an English system voice.
fn speak(text: &str, wav: &Path) {
    #[cfg(windows)]
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(
            "Add-Type -AssemblyName System.Speech; \
             $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $v = $s.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -eq 'en-US' } | Select-Object -First 1; \
             if ($v) { $s.SelectVoice($v.VoiceInfo.Name) }; \
             $f = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono); \
             $s.SetOutputToWaveFile($env:HS_E2E_WAV, $f); $s.Speak($env:HS_E2E_TEXT); $s.Dispose()",
        )
        .env("HS_E2E_WAV", wav)
        .env("HS_E2E_TEXT", text)
        .status()
        .unwrap();
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("say")
        .args(["-v", "Samantha", "-o"])
        .arg(wav)
        .args(["--data-format=LEI16@16000", text])
        .status()
        .unwrap();
    assert!(status.success());
}

/// Lower case, letters and digits only, single spaces.
fn words(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::test]
#[ignore = "builds the transcribe-cpp environment and needs HS_TRANSCRIBE_CPP_MODEL"]
async fn a_transcribe_cpp_model_transcribes_through_the_gateway() {
    let Some(model) = std::env::var_os("HS_TRANSCRIBE_CPP_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    else {
        eprintln!("HS_TRANSCRIBE_CPP_MODEL not set, skipping");
        return;
    };
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_TRANSCRIBE_CPP_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("transcribe-cpp-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    // `<root>/<owner>/<repo>/<file>`, as a download lays it out.
    settings.models_dir = model
        .ancestors()
        .nth(3)
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 19893;
    settings.save().unwrap();

    let wav = dir.join("speech.wav");
    speak(SENTENCE, &wav);
    let audio = std::fs::read(&wav).unwrap();
    let audio_secs = (audio.len() - 44) as f64 / 32_000.0;

    let cuda = harness_core::pyenv::envs::cuda_present();
    let name = if cuda {
        "transcribe-cpp-cuda"
    } else {
        "transcribe-cpp"
    };
    let t = Instant::now();
    let env = harness_core::pyenv::envs::install(name).await.unwrap();
    eprintln!("{name} in {:?}: {} bytes", t.elapsed(), env.size_bytes);
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );

    // The library lists the file as a speech recognition model; CrispASR does not take it.
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
    assert_eq!(found.kind, harness_core::domain::ModelKind::Asr);
    assert!(harness_core::engine::transcribe_cpp::is_model(&model));
    assert!(!harness_core::engine::crispasr::is_model(&model));
    let translates = harness_core::engine::transcribe_cpp::can_translate(&model);

    let def = ModelDefinition::new(
        "transcribe-cpp-e2e",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias.starts_with("transcribe-cpp-e2e") {
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
                port: 19883,
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
        let mut all = vec![("model", "transcribe-cpp-e2e")];
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
    let text = words(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["text"]
            .as_str()
            .unwrap(),
    );
    for word in [
        "harness",
        "local models",
        "speech into text",
        "brown fox",
        "lazy dog",
    ] {
        assert!(text.contains(word), "{word} missing in {text:?}");
    }

    // The worker says which backend it loaded the model on.
    let (id, worker) = registry
        .active_servers()
        .into_iter()
        .next()
        .expect("a running worker");
    let logs = registry.get(&id).unwrap().get_logs(None);
    let line = logs
        .iter()
        .find(|l| l.contains("transcribe_cpp ") && l.contains(" on "))
        .unwrap_or_else(|| panic!("no start line in {logs:#?}"));
    eprintln!("worker: {line}");
    if cuda {
        assert!(line.to_lowercase().contains("on cuda"), "{line}");
    }

    let t = Instant::now();
    let res = send(
        "/v1/audio/transcriptions",
        &[("response_format", "text"), ("language", "en")],
    )
    .await
    .unwrap();
    assert_eq!(res.status(), 200);
    let plain = res.text().await.unwrap();
    eprintln!(
        "warm text answer in {:?} for {audio_secs:.1} s of audio: {plain:?}",
        t.elapsed()
    );
    assert!(words(&plain).contains("lazy dog") && plain.ends_with('\n'));

    let res = send(
        "/v1/audio/transcriptions",
        &[("response_format", "verbose_json")],
    )
    .await
    .unwrap();
    assert_eq!(res.status(), 200);
    let verbose: serde_json::Value = res.json().await.unwrap();
    let segments = verbose["segments"].as_array().unwrap();
    eprintln!("{} segments", segments.len());
    assert!(!segments.is_empty(), "{verbose}");
    let mut last = 0.0;
    for s in segments {
        let end = s["end"].as_f64().unwrap();
        assert!(end >= last && end <= audio_secs + 0.5, "{verbose}");
        last = end;
    }

    // Translation only when the header says the model translates.
    let res = send("/v1/audio/translations", &[]).await.unwrap();
    let code = res.status();
    let body = res.text().await.unwrap();
    eprintln!("translation: {code} {body}");
    if translates {
        assert_eq!(code, 200, "{body}");
    } else {
        assert_eq!(code, 400, "{body}");
    }

    // Not audio: refused, the worker keeps serving.
    let res = post(
        format!("{base}/v1/audio/transcriptions"),
        form(b"RIFF\0\0\0\0WAVEjunk", &[("model", "transcribe-cpp-e2e")]),
    )
    .await
    .unwrap();
    let code = res.status();
    eprintln!("junk: {code} {}", res.text().await.unwrap());
    assert!(code.is_client_error(), "{code}");
    let res = send("/v1/audio/transcriptions", &[]).await.unwrap();
    assert_eq!(res.status(), 200);

    // The worker's own port refuses a request without its key, and a page of another origin.
    let res = http
        .get(format!("http://127.0.0.1:{}/v1/models", worker.port))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let res = http
        .post(format!(
            "http://127.0.0.1:{}/v1/audio/transcriptions",
            worker.port
        ))
        .header("origin", "https://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);

    server.stop().await;
    registry.stop_all().await;
}
