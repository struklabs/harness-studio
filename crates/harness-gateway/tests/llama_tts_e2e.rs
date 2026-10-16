//! llama.cpp text to speech end to end (no mocks): a Qwen3-TTS GGUF and its projector speak
//! through the runtime's `llama-tts` behind `/v1/audio/speech`. The runtime is a llama.cpp
//! release folder (with `llama-server` and `llama-tts`; a CUDA build runs on the GPU).
//!
//! ```text
//! HS_LLAMA_SERVER=<llama.cpp folder>/llama-server.exe \
//! HS_LLAMA_TTS_MODEL=<models>/ggml-org/Qwen3-TTS-12Hz-1.7B-Base-GGUF/Qwen3-TTS-12Hz-1.7B-Base-Q4_K_M.gguf \
//! HS_LLAMA_TTS_MMPROJ=<models>/ggml-org/Qwen3-TTS-12Hz-1.7B-Base-GGUF/mmproj-Qwen3-TTS-12Hz-1.7B-Base-Q8_0.gguf \
//!   cargo test -p harness-gateway --test llama_tts_e2e -- --ignored --nocapture
//! ```
//!
//! `HS_LLAMA_TTS_E2E_ROOT=<folder>` keeps the data root between runs (default: a kept temporary
//! folder); the WAVs it makes are written there to be listened to.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// Seconds and RMS (0..1) of a 16-bit PCM WAV.
fn measure(wav: &[u8]) -> (f64, f64) {
    assert!(
        wav.len() > 44 && &wav[..4] == b"RIFF" && &wav[8..12] == b"WAVE",
        "not a WAV"
    );
    let channels = u16::from_le_bytes([wav[22], wav[23]]) as f64;
    let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]) as f64;
    // The data chunk.
    let mut at = 12;
    while at + 8 <= wav.len() {
        let size =
            u32::from_le_bytes([wav[at + 4], wav[at + 5], wav[at + 6], wav[at + 7]]) as usize;
        if &wav[at..at + 4] == b"data" {
            let data = &wav[at + 8..(at + 8 + size).min(wav.len())];
            let samples: Vec<f64> = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b) as f64 / 32768.0)
                .collect();
            let rms =
                (samples.iter().map(|s| s * s).sum::<f64>() / samples.len().max(1) as f64).sqrt();
            return (samples.len() as f64 / channels / rate, rms);
        }
        at += 8 + size + (size & 1);
    }
    panic!("no data chunk");
}

#[tokio::test]
#[ignore = "needs HS_LLAMA_SERVER, HS_LLAMA_TTS_MODEL and HS_LLAMA_TTS_MMPROJ"]
async fn qwen3_tts_speaks_through_the_gateway() {
    let (Some(server_bin), Some(model), Some(mmproj)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_LLAMA_TTS_MODEL"),
        env_path("HS_LLAMA_TTS_MMPROJ"),
    ) else {
        eprintln!("HS_LLAMA_SERVER / HS_LLAMA_TTS_MODEL / HS_LLAMA_TTS_MMPROJ not set, skipping");
        return;
    };
    let tts_bin = harness_core::tts::llama::binary_next_to(&server_bin)
        .expect("llama-tts next to llama-server");
    eprintln!("llama-tts: {}", tts_bin.display());
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_LLAMA_TTS_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("llama-tts-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    settings.models_dir = model
        .ancestors()
        .nth(3)
        .unwrap()
        .to_string_lossy()
        .to_string();
    // The runtime the user chose: this llama.cpp folder.
    settings.runtime_mode = harness_core::config::RuntimeMode::Custom;
    settings.custom_binary.binary_path = Some(server_bin.clone());
    settings.custom_binary.is_valid = true;
    settings.control_api.port = 19894;
    settings.save().unwrap();

    let mut def = ModelDefinition::new(
        "llama-tts-e2e",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    def.mmproj = Some(mmproj.clone());
    let found = harness_core::tts::llama::model_of(&def).expect("a llama-tts model");
    assert_eq!((found.model, found.mmproj), (model.clone(), mmproj.clone()));
    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias.starts_with("llama-tts-e2e") {
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

    // Speaks, keeps the WAV, and answers its length.
    let say = |name: &'static str, body: serde_json::Value| {
        let dir = dir.clone();
        let res = speak(body);
        async move {
            let t = Instant::now();
            let res = res.await.unwrap();
            let code = res.status();
            let truncated = res
                .headers()
                .get("x-harness-truncated")
                .map(|v| v.to_str().unwrap().to_string());
            let bytes = res.bytes().await.unwrap();
            assert_eq!(code, 200, "{}", String::from_utf8_lossy(&bytes));
            let took = t.elapsed().as_secs_f64();
            let (secs, rms) = measure(&bytes);
            let path = dir.join(format!("{name}.wav"));
            std::fs::write(&path, &bytes).unwrap();
            eprintln!(
                "{name}: {secs:.2} s of audio in {took:.2} s (RTF {:.2}), RMS {rms:.3}, truncated {truncated:?} -> {}",
                took / secs,
                path.display()
            );
            assert!(rms > 0.01, "{name} is silent");
            secs
        }
    };

    let short = say(
        "short",
        serde_json::json!({
            "model": "llama-tts-e2e",
            "input": "Harness Studio runs local models on this computer. It turns text into speech.",
            "voice": "alloy",
            "response_format": "wav"
        }),
    )
    .await;
    // Frame budget of one run: at most 120 s, and a two-sentence text is well under 20 s.
    assert!((1.0..20.0).contains(&short), "{short}");

    let german = say(
        "german",
        serde_json::json!({
            "model": "llama-tts-e2e",
            "input": "Guten Morgen. Das Wetter ist heute schön.",
            "language": "de",
            "response_format": "wav"
        }),
    )
    .await;
    assert!(german > 1.0);

    let long_text = [
        "Harness Studio runs local models on this computer.",
        "It can load a model, answer a question, and turn speech into text.",
        "It can also turn text into speech, one group of sentences at a time.",
        "Each group gets its own frame budget, so a model that misses its end token stops in time.",
        "The groups are joined with a short pause between them.",
        "The quick brown fox jumps over the lazy dog.",
        "This last sentence makes the text longer than one group.",
    ]
    .join(" ");
    assert!(long_text.len() > harness_core::tts::llama::MAX_RUN_CHARS);
    let long = say(
        "long",
        serde_json::json!({ "model": "llama-tts-e2e", "input": long_text, "response_format": "wav" }),
    )
    .await;
    assert!(long > short, "{long} <= {short}");

    for (body, param) in [
        (
            serde_json::json!({"model": "llama-tts-e2e", "input": "Hello.", "language": "xx"}),
            "language",
        ),
        (
            serde_json::json!({"model": "llama-tts-e2e", "input": ""}),
            "input",
        ),
    ] {
        let res = speak(body).await.unwrap();
        let code = res.status();
        let text = res.text().await.unwrap();
        eprintln!("{param}: {code} {text}");
        assert_eq!(code, 400, "{text}");
    }

    server.stop().await;
    registry.stop_all().await;
}
