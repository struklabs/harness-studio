//! HF CTC transcription through the resident ONNX sidecar.
use super::{Segment, Transcript};
use std::path::Path;

/// CTC accepts WAV; ffmpeg supplies 16 kHz mono PCM for other containers.
pub async fn prepare_audio(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if crate::audio::sniff(&bytes) == crate::audio::Container::Wav {
        return Ok(bytes);
    }
    if crate::audio::ffmpeg_path().is_none() {
        return Err("Only WAV audio is supported by the ONNX CTC engine without ffmpeg; install ffmpeg to convert other formats.".into());
    }
    crate::audio::to_wav(bytes).await
}

/// OpenAI multipart request, shared by the gateway and the local CLI client.
pub fn multipart(audio: &[u8], format: &str, model: &str) -> (String, Vec<u8>) {
    let boundary = "harness-onnx-ctc-audio";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n").into_bytes();
    body.extend_from_slice(audio);
    body.extend_from_slice(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"response_format\"\r\n\r\n{format}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Runs one local CLI transcription. The model stays loaded while the sidecar serves the
/// request, and the private server is stopped on success and on every error.
pub async fn transcribe(
    path: &Path,
    audio: Vec<u8>,
    threads: Option<u32>,
) -> Result<Transcript, String> {
    let audio = prepare_audio(audio).await?;
    let registry = crate::servers::ServerRegistry::default();
    let id = "cli-onnx-transcribe";
    let start = std::time::Instant::now();
    let launched = crate::orchestrator::launch(
        &registry,
        &crate::runtime::RuntimeManager::default(),
        &crate::operation_gate::OperationGate::default(),
        id.into(),
        crate::process::LaunchConfig {
            model_path: path.to_path_buf(),
            host: "127.0.0.1".into(),
            port: 0,
            port_auto: true,
            threads,
            ..Default::default()
        },
    )
    .await;
    if let Err(e) = launched {
        registry.stop_all().await;
        return Err(e);
    }
    let pm = registry.get_or_create(id);
    let result = async {
        loop {
            match pm.get_status() {
                crate::process::ServerStatus::Running => break,
                crate::process::ServerStatus::Starting if start.elapsed().as_secs() < 180 => {}
                status => {
                    return Err(format!(
                        "ONNX CTC server failed to load: {status:?}\n{}",
                        pm.get_logs(Some(20)).join("\n")
                    ))
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let active = pm.active_server().ok_or("ONNX CTC server stopped")?;
        let (ct, body) = multipart(&audio, "verbose_json", id);
        let res = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?
            .post(format!(
                "http://127.0.0.1:{}/v1/audio/transcriptions",
                active.port
            ))
            .header("content-type", ct)
            .body(body)
            .timeout(std::time::Duration::from_secs(600))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status();
        let body = res.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("ONNX CTC: {status}: {body}"));
        }
        #[derive(serde::Deserialize)]
        struct Answer {
            text: String,
            #[serde(default)]
            segments: Vec<Segment>,
            #[serde(default)]
            duration: f64,
            #[serde(default)]
            language: Option<String>,
        }
        let answer: Answer = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        Ok(Transcript {
            text: answer.text,
            segments: answer.segments,
            duration_secs: answer.duration,
            language: answer.language,
            model: path.display().to_string(),
            elapsed_ms: start.elapsed().as_millis() as u64,
        })
    }
    .await;
    let stopped = pm.stop().await;
    match result {
        Ok(transcript) => {
            stopped?;
            Ok(transcript)
        }
        Err(e) => Err(e),
    }
}
