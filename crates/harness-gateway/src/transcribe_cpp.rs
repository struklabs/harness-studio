//! Speech to text with transcribe.cpp models: the part of `POST /v1/audio/transcriptions` and
//! `/v1/audio/translations` that serves a definition whose file is a transcribe.cpp export
//! ([`harness_core::engine::transcribe_cpp`]).
//!
//! The model runs in the engine's worker, which the scheduler loads and unloads like any other
//! server (its memory counts in the same budget). The audio becomes a 16 kHz mono 16-bit WAV
//! here (resampled, or converted by ffmpeg), the worker answers its text and segments, and this
//! shapes them like OpenAI's (`json`, `text`, `verbose_json`, `srt`, `vtt`).

use axum::http::{header, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use harness_core::domain::ModelDefinition;
use harness_core::engine::transcribe_cpp;

use crate::audio::Task;
use crate::errors::GatewayError;
use crate::proxy::RecordGuard;
use crate::GwState;

fn bad(message: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

/// Whether a definition's file is a transcribe.cpp speech recognition model.
pub fn is_model(def: &ModelDefinition) -> bool {
    def.base.local_path().is_some_and(transcribe_cpp::is_model)
}

/// A request for [`transcribe`].
pub struct Request<'a> {
    pub task: Task,
    pub def: &'a ModelDefinition,
    pub audio: Bytes,
    pub language: Option<&'a str>,
    pub format: &'a str,
    /// Diarization was asked for (`diarized_json` or `speakers`).
    pub diarize: bool,
}

/// The refusals that need no audio work: diarization, and translation by a model that only
/// transcribes.
pub fn check(req: &Request<'_>) -> Result<(), GatewayError> {
    if req.diarize {
        return Err(bad(format!(
            "`{}` cannot tell speakers apart; diarized_json and speakers work with the sherpa-onnx speech recognition models (Parakeet, Nemotron...).",
            req.def.alias
        ))
        .param("response_format"));
    }
    if req.task == Task::Translate
        && !req
            .def
            .base
            .local_path()
            .is_some_and(transcribe_cpp::can_translate)
    {
        return Err(
            bad("This model cannot translate; use /v1/audio/transcriptions.").param("model"),
        );
    }
    Ok(())
}

/// The worker's form fields: the language, the task, and segment timestamps when the format
/// shows them.
fn fields(req: &Request<'_>) -> Vec<(&'static str, String)> {
    let mut fields = Vec::new();
    if let Some(language) = req.language.map(str::trim).filter(|l| !l.is_empty()) {
        fields.push(("language", language.to_string()));
    }
    if req.task == Task::Translate {
        fields.push(("task", "translate".to_string()));
    }
    if matches!(req.format, "verbose_json" | "srt" | "vtt") {
        fields.push(("timestamps", "segment".to_string()));
    }
    fields
}

/// Serves one request (after [`check`]).
pub async fn transcribe(
    state: &GwState,
    req: Request<'_>,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    check(&req)?;
    let (wav, seconds) = transcribe_cpp::prepare_audio(req.audio.to_vec())
        .await
        .map_err(|e| {
            let status = if e.contains("stt.needs_ffmpeg") {
                415
            } else {
                400
            };
            GatewayError::new(status, "unsupported_audio_format", e).param("file")
        })?;
    let (content_type, body) = crate::audio::multipart_body("audio.wav", &wav, &fields(&req));
    let answer = crate::pyhf::post(
        state,
        req.def,
        guard,
        "/v1/audio/transcriptions",
        &content_type,
        body,
    )
    .await?;
    let elapsed = guard.started.elapsed().as_millis() as u64;
    let transcript = transcribe_cpp::transcript_of(&answer.body, &req.def.alias, seconds, elapsed)
        .ok_or_else(|| {
            GatewayError::new(
                502,
                "upstream_error",
                "The transcribe.cpp worker's answer has no text.",
            )
        })?;
    let (content_type, out) = crate::sherpa_asr::shape(req.format, &transcript);
    guard.finish(200, None);
    crate::pyhf::lease_headers(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type),
        req.def,
        &answer.lease,
    )
    .body(axum::body::Body::from(out))
    .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::domain::ModelRef;

    fn def(dir: &std::path::Path, translate: bool) -> ModelDefinition {
        use std::io::Write;
        let path = dir.join(if translate {
            "canary.gguf"
        } else {
            "parakeet.gguf"
        });
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"GGUF").unwrap();
        f.write_all(&3u32.to_le_bytes()).unwrap();
        f.write_all(&0u64.to_le_bytes()).unwrap();
        f.write_all(&3u64.to_le_bytes()).unwrap();
        for (k, v) in [("general.architecture", "parakeet"), ("stt.variant", "v")] {
            f.write_all(&(k.len() as u64).to_le_bytes()).unwrap();
            f.write_all(k.as_bytes()).unwrap();
            f.write_all(&8u32.to_le_bytes()).unwrap();
            f.write_all(&(v.len() as u64).to_le_bytes()).unwrap();
            f.write_all(v.as_bytes()).unwrap();
        }
        let k = "stt.capability.translate";
        f.write_all(&(k.len() as u64).to_le_bytes()).unwrap();
        f.write_all(k.as_bytes()).unwrap();
        f.write_all(&7u32.to_le_bytes()).unwrap();
        f.write_all(&[u8::from(translate)]).unwrap();
        drop(f);
        ModelDefinition::new("asr", ModelRef::Local { path })
    }

    fn req<'a>(def: &'a ModelDefinition, task: Task, format: &'a str) -> Request<'a> {
        Request {
            task,
            def,
            audio: Bytes::new(),
            language: Some(" tr "),
            format,
            diarize: false,
        }
    }

    #[test]
    fn requests_are_checked_and_forwarded_with_the_fields_the_format_needs() {
        let dir = tempfile::tempdir().unwrap();
        let parakeet = def(dir.path(), false);
        let canary = def(dir.path(), true);
        assert!(is_model(&parakeet));

        assert!(check(&req(&parakeet, Task::Transcribe, "json")).is_ok());
        let e = check(&req(&parakeet, Task::Translate, "json")).unwrap_err();
        assert_eq!(e.status, 400);
        assert!(check(&req(&canary, Task::Translate, "json")).is_ok());
        let mut diarize = req(&canary, Task::Transcribe, "json");
        diarize.diarize = true;
        assert_eq!(check(&diarize).unwrap_err().status, 400);

        assert_eq!(
            fields(&req(&parakeet, Task::Transcribe, "json")),
            vec![("language", "tr".to_string())]
        );
        assert_eq!(
            fields(&req(&canary, Task::Translate, "srt")),
            vec![
                ("language", "tr".to_string()),
                ("task", "translate".to_string()),
                ("timestamps", "segment".to_string()),
            ]
        );
    }
}
