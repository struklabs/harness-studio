//! Local CLI transcription with the managed native CrispASR server.
use super::{Segment, Transcript};
use std::path::Path;

/// Runs one local CLI transcription. The model stays loaded while the sidecar serves the
/// request, and the private server is stopped on success and on every error.
pub async fn transcribe(
    path: &Path,
    audio: Vec<u8>,
    threads: Option<u32>,
    runtime: Option<&str>,
    language: Option<&str>,
) -> Result<Transcript, String> {
    let audio = super::onnx::prepare_audio(audio).await?;
    let registry = crate::servers::ServerRegistry::default();
    let id = "cli-crispasr-transcribe";
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
            runtime: runtime.map(str::to_string),
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
                        "CrispASR server failed to load: {status:?}\n{}",
                        pm.get_logs(Some(20)).join("\n")
                    ))
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let active = pm.active_server().ok_or("CrispASR server stopped")?;
        let (ct, mut body) = super::onnx::multipart(&audio, "verbose_json", id);
        if let Some(language) = language {
            let boundary = ct.split("boundary=").nth(1).ok_or("Invalid multipart boundary")?;
            body.truncate(body.len() - format!("--{boundary}--\r\n").len());
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\n{language}\r\n--{boundary}--\r\n").as_bytes());
        }
        let res = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?
            .post(format!(
                "http://127.0.0.1:{}/v1/audio/transcriptions",
                active.port
            ))
            .bearer_auth(active.api_key.as_deref().ok_or("CrispASR worker key missing")?)
            .header("content-type", ct)
            .body(body)
            .timeout(std::time::Duration::from_secs(600))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status();
        let body = res.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("CrispASR: {status}: {body}"));
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
