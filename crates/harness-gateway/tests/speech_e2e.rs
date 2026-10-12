//! Text to speech through the gateway with the real sherpa-onnx runtime and real models (no
//! mocks). It downloads the runtime (20 MB) and a Turkish Piper voice (21 MB) from the official
//! releases, each checked against its SHA-256:
//!
//! ```text
//! cargo test -p harness-gateway --test speech_e2e -- --ignored --nocapture
//! HS_TTS_E2E_KOKORO=1 ...   # also downloads Kokoro int8 (132 MB) and checks its voices
//! HS_TTS_E2E_OUT=<folder>   # keeps the generated audio for listening
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets.

use std::sync::Arc;

use harness_core::audio::{sniff, wav_duration_secs, Container};
use harness_core::engine::{SHERPA_ONNX, SHERPA_ONNX_REPO};
use harness_core::gateway::GatewaySettings;
use harness_core::runtimes::github::{GithubInstall, ReleaseQuery};
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

async fn speak(
    http: &reqwest::Client,
    base: &str,
    body: Value,
) -> (u16, reqwest::header::HeaderMap, Vec<u8>) {
    let r = http
        .post(format!("{base}/v1/audio/speech"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let headers = r.headers().clone();
    (status, headers, r.bytes().await.unwrap().to_vec())
}

fn error_of(body: &[u8]) -> Value {
    serde_json::from_slice::<Value>(body).unwrap()["error"].clone()
}

fn keep(name: &str, bytes: &[u8]) {
    if let Some(dir) = std::env::var_os("HS_TTS_E2E_OUT") {
        let dir = std::path::PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(name), bytes).unwrap();
    }
}

#[tokio::test]
#[ignore]
async fn e2e_speech_wav_mp3_and_errors() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("gw-speech"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.server.port = 18830;
    s.control_api.port = 18839;
    s.save().unwrap();

    let ctx = GatewayContext {
        registry: Arc::new(ServerRegistry::default()),
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
                port: 18830,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    // Nothing installed: the answers say what to install.
    let (st, _, body) = speak(&http, &base, json!({"model": "tts-1", "input": "Merhaba"})).await;
    assert_eq!(st, 404, "{}", String::from_utf8_lossy(&body));
    let e = error_of(&body);
    assert_eq!(e["code"], "model_not_found");
    assert!(e["message"]
        .as_str()
        .unwrap()
        .contains("harness tts install"));

    // The models are installed, the runtime is not.
    let tr = harness_core::tts::catalog::install("vits-piper-tr_TR-fettah-medium-int8")
        .await
        .unwrap();
    assert_eq!(tr.languages, ["tr"]);
    let (st, _, body) = speak(&http, &base, json!({"input": "Merhaba"})).await;
    assert_eq!(st, 503, "{}", String::from_utf8_lossy(&body));
    assert_eq!(error_of(&body)["code"], "tts_runtime_missing");

    // The runtime, from the official release.
    let entry = harness_core::runtimes::github::install(
        &harness_core::runtimes::RuntimeRegistry::global(),
        &GithubInstall {
            query: ReleaseQuery {
                family: SHERPA_ONNX.into(),
                repo: SHERPA_ONNX_REPO.into(),
                ..Default::default()
            },
            name: Some("sherpa-onnx".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    eprintln!("runtime {} {:?}", entry.id, entry.version);

    // WAV, Turkish text with every Turkish letter.
    let text = "Şu çocuğun güzel öğüdü: İstanbul'da ışık yağıyor.";
    let (st, h, wav) = speak(
        &http,
        &base,
        json!({"model": "tts-1", "input": text, "voice": "alloy", "response_format": "wav"}),
    )
    .await;
    assert_eq!(st, 200, "{}", String::from_utf8_lossy(&wav));
    assert_eq!(h["content-type"], "audio/wav");
    assert_eq!(sniff(&wav), Container::Wav);
    let secs = wav_duration_secs(&wav).expect("a WAV with a duration");
    assert!(secs > 1.5, "{secs} s of audio");
    assert_eq!(h["x-harness-model"], "vits-piper-tr_TR-fettah-medium-int8");
    assert_eq!(h["x-harness-sample-rate"], "22050");
    assert!(h.contains_key("x-harness-request-id"));
    eprintln!("wav: {} bytes, {secs:.2} s", wav.len());
    keep("tr.wav", &wav);

    // Speed changes the length.
    let (st, _, fast) = speak(
        &http,
        &base,
        json!({"input": text, "response_format": "wav", "speed": 2.0}),
    )
    .await;
    assert_eq!(st, 200);
    assert!(wav_duration_secs(&fast).unwrap() < secs * 0.75);

    // PCM: the samples without a header.
    let (st, h, pcm) = speak(
        &http,
        &base,
        json!({"input": "Merhaba", "response_format": "pcm"}),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(h["content-type"], "audio/pcm");
    assert!(pcm.len() > 22_050 && sniff(&pcm) == Container::Unknown);

    // mp3 is the default format of OpenAI; it needs ffmpeg.
    if harness_core::audio::ffmpeg_path().is_some() {
        let (st, h, mp3) = speak(&http, &base, json!({"input": "Merhaba dünya"})).await;
        assert_eq!(st, 200, "{}", String::from_utf8_lossy(&mp3));
        assert_eq!(h["content-type"], "audio/mpeg");
        assert_eq!(sniff(&mp3), Container::Mp3);
        keep("tr.mp3", &mp3);
    } else {
        let (st, h, wav) = speak(&http, &base, json!({"input": "Merhaba dünya"})).await;
        assert_eq!(st, 200);
        assert_eq!(
            h["content-type"], "audio/wav",
            "no ffmpeg: the default falls back"
        );
        assert_eq!(sniff(&wav), Container::Wav);
        let (st, _, body) = speak(
            &http,
            &base,
            json!({"input": "Merhaba", "response_format": "mp3"}),
        )
        .await;
        assert_eq!(st, 400);
        assert_eq!(error_of(&body)["code"], "format_unavailable");
    }

    // Bad requests.
    for (body, code, param) in [
        (json!({"input": ""}), "invalid_request", "input"),
        (
            json!({"input": "x".repeat(4097)}),
            "invalid_request",
            "input",
        ),
        (
            json!({"input": "x", "speed": 9.0}),
            "invalid_request",
            "speed",
        ),
        (
            json!({"input": "x", "response_format": "ogg"}),
            "invalid_request",
            "response_format",
        ),
        (
            json!({"model": "whisper-1", "input": "x"}),
            "model_not_found",
            "model",
        ),
        (json!({"model": "tts-1"}), "invalid_request", "input"),
    ] {
        let (st, _, b) = speak(&http, &base, body.clone()).await;
        let e = error_of(&b);
        assert!(st == 400 || st == 404, "{body}: {st}");
        assert_eq!(e["code"], code, "{body}: {e}");
        assert_eq!(e["param"], param, "{body}: {e}");
    }
    let r = http
        .post(format!("{base}/v1/audio/speech"))
        .body("not json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);

    if std::env::var_os("HS_TTS_E2E_KOKORO").is_some() {
        harness_core::tts::catalog::install("kokoro-int8-multi-lang-v1_0")
            .await
            .unwrap();
        // The default is now Kokoro; OpenAI voice names map to its voices.
        let (st, h, wav) = speak(
            &http,
            &base,
            json!({"model": "tts-1", "input": "Harness Studio can read this aloud.", "voice": "nova", "response_format": "wav"}),
        )
        .await;
        assert_eq!(st, 200, "{}", String::from_utf8_lossy(&wav));
        assert_eq!(h["x-harness-voice"], "af_nova");
        assert_eq!(h["x-harness-sample-rate"], "24000");
        assert!(wav_duration_secs(&wav).unwrap() > 1.5);
        keep("en.wav", &wav);
        // The Turkish voice by name while Kokoro is the default.
        let (st, h, _) = speak(
            &http,
            &base,
            json!({"model": "fettah", "input": "Merhaba", "response_format": "wav"}),
        )
        .await;
        assert_eq!(st, 200);
        assert_eq!(h["x-harness-model"], "vits-piper-tr_TR-fettah-medium-int8");
        let (st, _, b) = speak(
            &http,
            &base,
            json!({"model": "kokoro", "input": "x", "voice": "morgan"}),
        )
        .await;
        assert_eq!(st, 400);
        assert_eq!(error_of(&b)["param"], "voice");
    }
    gw.stop().await;
}
