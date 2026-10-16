//! Local CLI transcription with the managed transcribe.cpp worker.
use super::Transcript;
use std::path::Path;

/// Runs one local CLI transcription. The model stays loaded while the sidecar serves the
/// request, and the private server is stopped on success and on every error.
pub async fn transcribe(
    path: &Path,
    audio: Vec<u8>,
    language: Option<&str>,
    translate: bool,
    timestamps: bool,
) -> Result<Transcript, String> {
    if translate && !crate::engine::transcribe_cpp::can_translate(path) {
        return Err("This transcribe.cpp model does not support translation".into());
    }
    let (audio, secs) = crate::engine::transcribe_cpp::prepare_audio(audio).await?;
    let registry = crate::servers::ServerRegistry::default();
    let id = "cli-transcribe-cpp";
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
                        "transcribe.cpp server failed to load: {status:?}\n{}",
                        pm.get_logs(Some(20)).join("\n")
                    ))
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let active = pm.active_server().ok_or("transcribe.cpp server stopped")?;
        let (ct, body) = multipart(&audio, language, translate, timestamps);
        let res = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?
            .post(format!(
                "http://127.0.0.1:{}/v1/audio/transcriptions",
                active.port
            ))
            .bearer_auth(
                active
                    .api_key
                    .as_deref()
                    .ok_or("transcribe.cpp worker key missing")?,
            )
            .header("content-type", ct)
            .body(body)
            .timeout(std::time::Duration::from_secs(600))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status();
        let body = res.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("transcribe.cpp: {status}: {body}"));
        }
        crate::engine::transcribe_cpp::transcript_of(
            body.as_bytes(),
            &path.display().to_string(),
            secs,
            start.elapsed().as_millis() as u64,
        )
        .ok_or_else(|| "Invalid transcribe.cpp transcription response".into())
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

fn multipart(
    audio: &[u8],
    language: Option<&str>,
    translate: bool,
    timestamps: bool,
) -> (String, Vec<u8>) {
    let boundary = "harness-transcribe-cpp-audio";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n").into_bytes();
    body.extend_from_slice(audio);
    body.extend_from_slice(b"\r\n");
    let task = if translate { "translate" } else { "transcribe" };
    for (name, value) in [
        ("language", language),
        ("task", Some(task)),
        (
            "timestamps",
            Some(if timestamps { "segment" } else { "none" }),
        ),
    ] {
        if let Some(value) = value {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
        }
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_fields() {
        for (language, translate, timestamps) in [
            (None, false, false),
            (Some("tr"), false, true),
            (Some("en"), true, false),
        ] {
            let (ct, body) = multipart(b"WAV", language, translate, timestamps);
            let body = String::from_utf8(body).unwrap();
            assert!(ct.contains("boundary=harness-transcribe-cpp-audio"));
            assert!(body.contains(
                "name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nWAV\r\n"
            ));
            assert_eq!(body.contains("name=\"language\""), language.is_some());
            if let Some(language) = language {
                assert!(body.contains(&format!("name=\"language\"\r\n\r\n{language}\r\n")));
            }
            let task = if translate { "translate" } else { "transcribe" };
            assert!(body.contains(&format!("name=\"task\"\r\n\r\n{task}\r\n")));
            let timestamps = if timestamps { "segment" } else { "none" };
            assert!(body.contains(&format!("name=\"timestamps\"\r\n\r\n{timestamps}\r\n")));
            assert!(body.ends_with("--harness-transcribe-cpp-audio--\r\n"));
        }
    }

    #[tokio::test]
    async fn translation_is_refused_before_audio_preparation_or_launch() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = dir.path().join("parakeet.gguf");
        crate::engine::transcribe_cpp::tests::export(&path, "parakeet", Some("parakeet"));
        let error = transcribe(&path, Vec::new(), None, true, false)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            "This transcribe.cpp model does not support translation"
        );
    }
}
