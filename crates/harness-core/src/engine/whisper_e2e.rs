//! Real whisper-server runs, all `#[ignore]`: they need a real binary and
//! a real ggml model.
//!
//! ```text
//! HS_WHISPER_SERVER=<whisper-server(.exe)> HS_WHISPER_MODEL=<ggml-*.bin> \
//! HS_WHISPER_WAV=<speech.wav> HS_WHISPER_TEXT="harness studio" \
//! cargo test -p harness-core --lib whisper_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `HS_WHISPER_TEXT` is a lower-case fragment the transcript of the wav must contain.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::failure::FailureKind;
use crate::process::{LaunchConfig, ProcessManager, ServerStatus};
use crate::serve_mode::ServeMode;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

struct Inputs {
    binary: PathBuf,
    model: PathBuf,
    wav: PathBuf,
    text: String,
}

fn inputs() -> Option<Inputs> {
    let (Some(binary), Some(model), Some(wav)) = (
        env("HS_WHISPER_SERVER"),
        env("HS_WHISPER_MODEL"),
        env("HS_WHISPER_WAV"),
    ) else {
        eprintln!("HS_WHISPER_SERVER / HS_WHISPER_MODEL / HS_WHISPER_WAV not set, skipping");
        return None;
    };
    Some(Inputs {
        binary: binary.into(),
        model: model.into(),
        wav: wav.into(),
        text: env("HS_WHISPER_TEXT").unwrap_or_default().to_lowercase(),
    })
}

fn config(i: &Inputs, port: u16) -> LaunchConfig {
    LaunchConfig {
        binary_path: i.binary.clone(),
        model_path: i.model.clone(),
        host: "127.0.0.1".into(),
        port,
        gpu_layers: 999,
        ..Default::default()
    }
}

/// A multipart/form-data body (`file` plus text fields), the way a client sends it.
pub(crate) fn multipart(
    file_name: &str,
    bytes: &[u8],
    fields: &[(&str, &str)],
) -> (String, Vec<u8>) {
    let boundary = "----harness-e2e-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
    for (k, v) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n")
                .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn wait_settled(mgr: &ProcessManager, secs: u64) -> ServerStatus {
    let t = Instant::now();
    loop {
        let s = mgr.get_status();
        if s != ServerStatus::Starting {
            return s;
        }
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "still starting: {:?}",
            mgr.get_logs(Some(20))
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn port_free(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(300),
    )
    .is_err()
}

/// Start, health, transcribe a real wav, stop; then the failure classes of a bad model and a
/// taken port.
#[tokio::test]
#[ignore]
async fn whisper_e2e_server_lifecycle() {
    let Some(i) = inputs() else { return };
    let port = crate::ports::pick_auto("127.0.0.1", None, &[]).expect("a free port");
    let mgr = ProcessManager::with_id("whisper-e2e");
    let started = Instant::now();
    mgr.start(config(&i, port)).await.unwrap();
    assert!(
        mgr.start(config(&i, port)).await.is_err(),
        "a second start is refused"
    );
    let status = wait_settled(&mgr, 120).await;
    assert_eq!(
        status,
        ServerStatus::Running,
        "{:?}",
        mgr.get_logs(Some(30))
    );
    eprintln!("running after {:.1?}", started.elapsed());
    let active = mgr.active_server().unwrap();
    assert_eq!(active.serve_mode, ServeMode::Transcription);
    assert_eq!(active.port, port);
    let logs = mgr.get_logs(None).join("\n");
    assert!(logs.contains("Starting whisper-server"), "{logs}");
    assert!(
        logs.contains("whisper_init_from_file"),
        "server output is logged"
    );
    // The arguments that were built (language auto by default, no `--no-gpu` with GPU layers).
    let cfg = mgr.launch_config().unwrap();
    assert_eq!(cfg.port, port);

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let health = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(health.status(), 200);
    eprintln!("health: {}", health.text().await.unwrap());

    let wav = std::fs::read(&i.wav).unwrap();
    let (content_type, body) = multipart("speech.wav", &wav, &[("response_format", "json")]);
    let t = Instant::now();
    let res = client
        .post(format!("{base}/inference"))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let v: serde_json::Value = res.json().await.unwrap();
    eprintln!("transcribed in {:.1?}: {v}", t.elapsed());
    let text = v["text"].as_str().unwrap().to_lowercase();
    assert!(text.contains(&i.text), "{text:?} lacks {:?}", i.text);

    // Telemetry sees it online through `/health` (there is no `/metrics`).
    let mut poller = crate::telemetry::TelemetryPoller::new(None);
    poller
        .poll(&[("whisper-e2e".to_string(), mgr.active_server().unwrap())])
        .await;
    assert!(poller.any_live(), "the whisper server counts as live");

    mgr.stop().await.unwrap();
    assert_eq!(mgr.get_status(), ServerStatus::Stopped);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(port_free(port), "the port is released after stop");

    // A file that is not a ggml model: the load fails and is classified as a corrupt model.
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("ggml-bad.bin");
    std::fs::write(&bad, vec![7u8; 4096]).unwrap();
    let mgr = ProcessManager::with_id("whisper-e2e-bad");
    let mut cfg = config(&i, port);
    cfg.model_path = bad;
    mgr.start(cfg).await.unwrap();
    let status = wait_settled(&mgr, 60).await;
    let failure = mgr.last_failure();
    eprintln!("bad model: {status:?} failure {failure:?}");
    assert!(matches!(status, ServerStatus::Error(_)), "{status:?}");
    let failure = failure.expect("a recorded failure");
    assert_eq!(failure.kind, FailureKind::CorruptModel, "{failure:?}");

    // A taken port. whisper-server itself would bind it a second time on Windows (the HTTP
    // library sets SO_REUSEADDR, and two sockets then share the port), so the launch checks
    // the port before it starts anything (`orchestrator::launch_with_binary`).
    let mgr1 = ProcessManager::with_id("whisper-e2e-1");
    mgr1.start(config(&i, port)).await.unwrap();
    assert_eq!(wait_settled(&mgr1, 120).await, ServerStatus::Running);
    let taken = crate::servers::check_port_free("127.0.0.1", port).unwrap_err();
    eprintln!("taken port check: {taken}");
    assert!(
        taken.starts_with(crate::orchestrator::ERR_PORT_IN_USE),
        "{taken}"
    );
    mgr1.stop().await.unwrap();
    assert!(port_free(port));
    assert!(crate::servers::check_port_free("127.0.0.1", port).is_ok());
}
