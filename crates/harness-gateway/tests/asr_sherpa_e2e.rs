//! Speech to text with sherpa-onnx models through the gateway, with the real runtime and real
//! models (no mocks). It downloads the runtime (20 MB), a Turkish Piper voice (21 MB) to make
//! speech with, Silero VAD and Omnilingual ASR 300M int8 (292 MB) from the official releases,
//! each checked against its SHA-256:
//!
//! ```text
//! cargo test -p harness-gateway --test asr_sherpa_e2e -- --ignored --nocapture
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets.

use std::sync::Arc;

use harness_core::engine::{SHERPA_ONNX, SHERPA_ONNX_REPO};
use harness_core::gateway::GatewaySettings;
use harness_core::runtimes::github::{GithubInstall, ReleaseQuery};
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::Value;

const OMNILINGUAL: &str = "sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-v2-int8-2026-02-05";
const TEXT: &str = "Merhaba, bu bir deneme konuşmasıdır. Yapay zekâ ile ses tanıma artık tamamen bilgisayarınızda çalışıyor.";

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
    file: &[u8],
    fields: &[(&str, &str)],
) -> (u16, reqwest::header::HeaderMap, String) {
    let mut parts: Vec<(&str, Option<&str>, Vec<u8>)> =
        vec![("file", Some("speech.wav"), file.to_vec())];
    for (k, v) in fields {
        parts.push((k, None, v.as_bytes().to_vec()));
    }
    let (ct, body) = form(&parts);
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

/// The fields of a request for the model under test.
fn t<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut all = vec![("model", OMNILINGUAL), ("language", "tr")];
    all.extend_from_slice(extra);
    all
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

#[tokio::test]
#[ignore]
async fn e2e_sherpa_transcription_formats_and_routing() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("gw-asr"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.server.port = 18850;
    s.control_api.port = 18859;
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
                port: 18850,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let wav_stub = vec![0u8; 100];

    // Nothing installed: a name nobody knows is the scheduler's own `model_not_found`.
    let (st, _, body) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &wav_stub,
        &[("model", OMNILINGUAL)],
    )
    .await;
    assert_eq!(st, 404, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
        "model_not_found"
    );

    // The runtime, the speech to read back, the recognition model.
    harness_core::runtimes::github::install(
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
    let voice = harness_core::tts::catalog::install("vits-piper-tr_TR-fettah-medium-int8")
        .await
        .unwrap();
    let speech = harness_core::tts::run::synthesize(
        &harness_core::tts::run::resolve_binary(None).unwrap(),
        &harness_core::tts::run::SynthesisRequest {
            model: &voice,
            voice: None,
            input: TEXT,
            speed: 1.0,
            threads: None,
        },
    )
    .await
    .unwrap()
    .wav;
    // Installed before the runtime would be a 503: the model is there, the tool is not.
    let model = harness_core::asr::catalog::install(OMNILINGUAL)
        .await
        .unwrap();
    assert_eq!(model.name, OMNILINGUAL);

    // json: the text, as OpenAI answers.
    let (st, h, body) = post(&http, &base, "/v1/audio/transcriptions", &speech, &t(&[])).await;
    assert_eq!(st, 200, "{body}");
    assert!(h["content-type"]
        .to_str()
        .unwrap()
        .starts_with("application/json"));
    assert_eq!(h["x-harness-model"], OMNILINGUAL);
    assert!(h.contains_key("x-harness-request-id"));
    let text = serde_json::from_str::<Value>(&body).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    let (want, got) = (words(TEXT), words(&text));
    let hits = want.iter().filter(|w| got.contains(w)).count();
    eprintln!("tr: {hits}/{} words: {text}", want.len());
    assert!(hits * 10 >= want.len() * 6, "{text}");

    // text, verbose_json with timed segments, srt, vtt.
    let (st, h, plain) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &speech,
        &t(&[("response_format", "text")]),
    )
    .await;
    assert_eq!(st, 200);
    assert!(h["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    assert_eq!(plain.trim(), text);
    let (st, _, verbose) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &speech,
        &t(&[("response_format", "verbose_json")]),
    )
    .await;
    assert_eq!(st, 200);
    let v: Value = serde_json::from_str(&verbose).unwrap();
    assert_eq!(v["task"], "transcribe");
    assert_eq!(v["language"], "tr");
    assert!(v["duration"].as_f64().unwrap() > 5.0);
    let seg = &v["segments"][0];
    assert!(seg["start"].as_f64().unwrap() < 1.0 && seg["end"].as_f64().unwrap() > 3.0);
    assert_eq!(v["text"], text.as_str());
    let (_, h, srt) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &speech,
        &t(&[("response_format", "srt")]),
    )
    .await;
    assert!(h["content-type"]
        .to_str()
        .unwrap()
        .starts_with("application/x-subrip"));
    assert!(srt.starts_with("1\n00:00:00,"), "{srt}");
    let (_, h, vtt) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &speech,
        &t(&[("response_format", "vtt")]),
    )
    .await;
    assert!(h["content-type"].to_str().unwrap().starts_with("text/vtt"));
    assert!(vtt.starts_with("WEBVTT\n\n00:00:00."), "{vtt}");

    // A container the tool cannot read is converted by ffmpeg (or refused when there is none).
    if harness_core::audio::ffmpeg_path().is_some() {
        let mp3 =
            harness_core::tts::run::encode(speech.clone(), harness_core::tts::AudioFormat::Mp3)
                .await
                .unwrap();
        let (st, _, body) = post(&http, &base, "/v1/audio/transcriptions", &mp3, &t(&[])).await;
        assert_eq!(st, 200, "{body}");
        let mp3_text = serde_json::from_str::<Value>(&body).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string();
        let got = words(&mp3_text);
        assert!(
            want.iter().filter(|w| got.contains(w)).count() * 10 >= want.len() * 6,
            "{mp3_text}"
        );
    }

    // Bad requests: the format, a translation (Whisper's task), audio nobody can read.
    let (st, _, body) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &speech,
        &t(&[("response_format", "yaml")]),
    )
    .await;
    assert_eq!(st, 400, "{body}");
    let (st, _, body) = post(&http, &base, "/v1/audio/translations", &speech, &t(&[])).await;
    assert_eq!(st, 400, "{body}");
    assert!(body.contains("only transcribes"), "{body}");
    let (st, _, body) = post(
        &http,
        &base,
        "/v1/audio/transcriptions",
        &vec![7u8; 4000],
        &t(&[]),
    )
    .await;
    assert!(st == 400 || st == 415 || st == 422, "{st} {body}");

    // A part of a name is not enough, and an OpenAI name still finds no whisper definition.
    for name in ["omnilingual", "whisper-1"] {
        let (st, _, body) = post(
            &http,
            &base,
            "/v1/audio/transcriptions",
            &speech,
            &[("model", name)],
        )
        .await;
        assert_eq!(st, 404, "{name}: {body}");
    }
}
