//! LFM2.5-Audio speech output end to end (no mocks): the managed `liquid-audio` environment is
//! built from its hashed lock, the library lists the model folder as a text-to-speech model, and
//! `/v1/audio/speech` goes through the gateway to the worker it starts (Liquid's `liquid-audio`
//! package on PyTorch). The audio is checked as a real recording (WAV header, length, level).
//!
//! ```text
//! HS_LIQUID_AUDIO_MODEL=<models>/LiquidAI/LFM2.5-Audio-1.5B \
//!   cargo test -p harness-gateway --test liquid_audio_e2e -- --ignored --nocapture
//! ```
//!
//! `HS_LIQUID_AUDIO_E2E_ROOT=<folder>` keeps the data root (and the environment) between runs;
//! by default it is a temporary folder (uv, Python and the packages: about 0.9 GB). The model
//! folder is the Hub repo `LiquidAI/LFM2.5-Audio-1.5B` (about 3.4 GB); it is only read.

#![cfg(any(windows, target_os = "macos", target_os = "linux"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};

const TEXT: &str = "Harness Studio runs local models on this computer. It can now speak with LFM two point five Audio.";

/// Seconds, peak, RMS and sample rate of a 16-bit mono WAV, checking its header.
fn inspect(wav: &[u8]) -> (f64, f64, f64, u32) {
    assert!(wav.len() > 44 && &wav[0..4] == b"RIFF" && &wav[8..12] == b"WAVE");
    let channels = u16::from_le_bytes([wav[22], wav[23]]);
    let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    let bits = u16::from_le_bytes([wav[34], wav[35]]);
    assert_eq!((channels, bits), (1, 16));
    let samples: Vec<f64> = wav[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b) as f64 / 32768.0)
        .collect();
    let peak = samples.iter().fold(0.0_f64, |m, s| m.max(s.abs()));
    let rms = (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt();
    (samples.len() as f64 / rate as f64, peak, rms, rate)
}

#[tokio::test]
#[ignore = "builds the liquid-audio environment and needs HS_LIQUID_AUDIO_MODEL"]
async fn an_lfm2_audio_model_speaks_through_the_gateway() {
    let Some(model) = std::env::var_os("HS_LIQUID_AUDIO_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        eprintln!("HS_LIQUID_AUDIO_MODEL not set, skipping");
        return;
    };
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_LIQUID_AUDIO_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("liquid-audio-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    // `<root>/<owner>/<repo>`, as a download lays it out.
    settings.models_dir = model
        .ancestors()
        .nth(2)
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 19894;
    settings.save().unwrap();

    assert!(harness_core::engine::python_hf::is_liquid_audio_dir(&model));
    // Not installed yet (a kept data root may have it): the launch names the missing runtime,
    // the UI offers the install.
    if !harness_core::pyenv::envs::status_in(
        &harness_core::pyenv::envs::root(),
        &harness_core::pyenv::envs::LIQUID_AUDIO,
    )
    .ready
    {
        assert_eq!(
            harness_core::engine::python_hf::binary_for(&model).unwrap_err(),
            "runtime.family_not_installed: liquid-audio"
        );
    }

    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("liquid-audio")
        .await
        .unwrap();
    eprintln!(
        "liquid-audio env in {:?}: {} bytes",
        t.elapsed(),
        env.size_bytes
    );
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );
    assert!(harness_core::model_registry::installed_families()
        .iter()
        .any(|f| f == "liquid-audio"));

    // The library lists the folder as a text-to-speech model of the Python engine.
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    let found = library
        .iter()
        .find(|m| m.path == model)
        .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()));
    eprintln!(
        "library: {} {:?} {} bytes",
        found.name, found.task, found.size_bytes
    );
    assert_eq!(found.task.as_deref(), Some("text-to-speech"));

    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias.starts_with("lfm-audio-e2e") {
            store.delete_definition(&old.id).unwrap();
        }
    }
    store
        .save_definition(ModelDefinition::new(
            "lfm-audio-e2e",
            ModelRef::Local {
                path: model.clone(),
            },
        ))
        .unwrap();

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
                port: 19884,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let speak = |body: serde_json::Value| {
        http.post(format!("{base}/v1/audio/speech"))
            .json(&body)
            .send()
    };

    // Cold: the worker starts and loads the model (load time included).
    let t = Instant::now();
    let res = speak(serde_json::json!({
        "model": "lfm-audio-e2e", "input": TEXT, "voice": "uk_female", "response_format": "wav"
    }))
    .await
    .unwrap();
    let code = res.status();
    let content_type = res
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string());
    let wav = res.bytes().await.unwrap();
    let cold = t.elapsed();
    assert_eq!(code, 200, "{}", String::from_utf8_lossy(&wav));
    assert_eq!(content_type.as_deref(), Some("audio/wav"));
    let (secs, peak, rms, rate) = inspect(&wav);
    eprintln!("cold answer in {cold:?} (load included): {secs:.2} s of audio at {rate} Hz, peak {peak:.2}, rms {rms:.3}");
    assert_eq!(rate, 24_000);
    assert!(
        secs > 3.0 && secs < 20.0,
        "{secs} s for {} characters",
        TEXT.len()
    );
    assert!(
        peak > 0.05 && peak <= 1.0 && rms > 0.01,
        "silent audio: peak {peak}, rms {rms}"
    );
    std::fs::write(dir.join("lfm-tts-cold.wav"), &wav).unwrap();

    // Warm: same text, an OpenAI voice name; this is the generation time and real-time factor.
    let t = Instant::now();
    let res = speak(serde_json::json!({
        "model": "lfm-audio-e2e", "input": TEXT, "voice": "alloy", "response_format": "wav"
    }))
    .await
    .unwrap();
    assert_eq!(res.status(), 200);
    let wav = res.bytes().await.unwrap();
    let warm = t.elapsed().as_secs_f64();
    let (secs, peak, rms, _) = inspect(&wav);
    eprintln!(
        "WARM generation {warm:.1} s for {secs:.2} s of audio: RTF {:.2} (peak {peak:.2}, rms {rms:.3})",
        warm / secs
    );
    assert!(secs > 3.0 && rms > 0.01);
    std::fs::write(dir.join("lfm-tts-warm.wav"), &wav).unwrap();

    // Raw PCM (no ffmpeg needed) and a refused voice; the worker keeps serving.
    let res = speak(serde_json::json!({
        "model": "lfm-audio-e2e", "input": "Short.", "response_format": "pcm"
    }))
    .await
    .unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.bytes().await.unwrap().len() > 4_000);
    let res = speak(serde_json::json!({
        "model": "lfm-audio-e2e", "input": "Short.", "voice": "bogus"
    }))
    .await
    .unwrap();
    let code = res.status();
    let body = res.text().await.unwrap();
    eprintln!("bogus voice: {code} {body}");
    assert_eq!(code, 400, "{body}");

    // The worker's own port refuses a request without its key, and a page of another origin.
    let (_, worker) = registry
        .active_servers()
        .into_iter()
        .next()
        .expect("a running worker");
    let res = http
        .get(format!("http://127.0.0.1:{}/v1/models", worker.port))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let res = http
        .post(format!("http://127.0.0.1:{}/v1/audio/speech", worker.port))
        .header("origin", "https://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);

    server.stop().await;
    registry.stop_all().await;
}
