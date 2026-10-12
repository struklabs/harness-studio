//! Speech to text with sherpa-onnx models: the part of
//! `POST /v1/audio/transcriptions` that serves Parakeet, SenseVoice, Omnilingual and the other
//! ONNX models whisper.cpp cannot run ([`harness_core::asr`]).
//!
//! The request's `model` names a model folder of the library (`harness asr ls`). A whisper
//! definition of that name always wins, so the whisper path of [`crate::audio`] is untouched:
//! only a `model` the scheduler does not know is looked up here, by its whole name.
//!
//! sherpa-onnx has no server for these models: each request starts
//! `sherpa-onnx-vad-with-offline-asr`, which loads the model, cuts the audio at pauses, reads the
//! segments and exits, so the model is released at once, and requests run side by side up to the
//! runtime's limit. The audio is converted by ffmpeg when it is not a 16-bit mono WAV
//! ([`harness_core::asr::run::prepare`]).

use axum::http::{header, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use harness_core::asr::{self, catalog, run, AsrModel, Transcript};
use harness_core::error_key;

use crate::audio::Task;
use crate::auth::Caller;
use crate::errors::GatewayError;
use crate::proxy::RecordGuard;

fn bad(message: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

fn join_failed(e: tokio::task::JoinError) -> GatewayError {
    GatewayError::new(500, "gateway_error", e.to_string())
}

/// The gateway's answer to a keyed error of the speech-recognition core.
pub fn map_error(e: &str) -> GatewayError {
    let Some((key, params)) = error_key::parse(e) else {
        return GatewayError::new(500, "asr_failed", e.to_string());
    };
    let text = |name: &str| {
        params
            .get(name)
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_default()
    };
    match key {
        "stt.no_model" => GatewayError::new(
            404,
            "model_not_found",
            "No speech recognition model of sherpa-onnx is installed. Install one in the Playground (Speech tab) or with `harness asr install`.",
        )
        .param("model"),
        "stt.model_not_found" => GatewayError::new(
            404,
            "model_not_found",
            format!(
                "`{}` is not a speech recognition model. List them with `harness asr ls`.",
                text("model")
            ),
        )
        .param("model"),
        "stt.model_ambiguous" => bad(format!(
            "`{}` matches several speech recognition models: {}.",
            text("model"),
            text("candidates")
        ))
        .param("model"),
        "stt.needs_ffmpeg" | "audio.ffmpeg_missing" => GatewayError::new(
            415,
            "unsupported_audio_format",
            format!(
                "{} audio needs ffmpeg to be converted and none was found; install ffmpeg, or send a 16-bit mono WAV.",
                if key == "stt.needs_ffmpeg" { text("format") } else { "This".to_string() }
            ),
        )
        .param("file"),
        "audio.convert_failed" | "audio.convert_timeout" => GatewayError::new(
            400,
            "audio_convert_failed",
            format!("The audio could not be decoded: {}", text("error")),
        )
        .param("file"),
        "runtime.family_not_installed" | "runtime.not_found" | "stt.binary_not_found" => {
            GatewayError::new(
                503,
                "asr_runtime_missing",
                "The sherpa-onnx runtime is not installed. Install it in the Playground (Speech tab) or with `harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx`.",
            )
        }
        "stt.timeout" => GatewayError::new(
            504,
            "upstream_timeout",
            format!("Speech recognition took longer than {} seconds.", text("seconds")),
        ),
        "stt.run_failed" => GatewayError::new(
            422,
            "invalid_audio",
            format!("sherpa-onnx could not read the audio: {}", text("detail")),
        )
        .param("file"),
        _ => GatewayError::new(500, "asr_failed", format!("{key}: {e}")),
    }
}

/// The installed speech-recognition model a request's `model` names, by its whole name (or the
/// model file's path); `None` when it names none.
pub async fn find(wanted: Option<&str>) -> Result<Option<AsrModel>, GatewayError> {
    let Some(wanted) = wanted.map(str::trim).filter(|w| !w.is_empty()) else {
        return Ok(None);
    };
    let models = tokio::task::spawn_blocking(asr::installed_models)
        .await
        .map_err(join_failed)?;
    Ok(asr::find_model(&models, Some(wanted), true).ok().cloned())
}

/// What the gateway sends back for `format`: `(content type, body)`.
pub fn shape(format: &str, transcript: &Transcript) -> (&'static str, Vec<u8>) {
    match format {
        "text" => (
            "text/plain; charset=utf-8",
            format!("{}\n", transcript.text.trim()).into_bytes(),
        ),
        "srt" => (
            "application/x-subrip; charset=utf-8",
            transcript.to_srt().into_bytes(),
        ),
        "vtt" => ("text/vtt; charset=utf-8", transcript.to_vtt().into_bytes()),
        "verbose_json" => (
            "application/json",
            serde_json::to_vec(&transcript.to_verbose_json("transcribe")).unwrap_or_default(),
        ),
        _ => (
            "application/json",
            serde_json::to_vec(&transcript.to_json()).unwrap_or_default(),
        ),
    }
}

/// A request for [`serve`].
pub struct Request<'a> {
    pub task: Task,
    pub model: AsrModel,
    pub audio: Bytes,
    pub language: Option<String>,
    /// One of the `response_format` values the gateway accepts.
    pub format: &'a str,
    pub caller: &'a Caller,
}

/// Serves one transcription with `req.model`.
pub async fn serve(req: Request<'_>, guard: &mut RecordGuard) -> Result<Response, GatewayError> {
    if req.task == Task::Translate {
        return Err(bad(format!(
            "`{}` only transcribes; translation to English is a Whisper task (use a whisper model with {}).",
            req.model.name,
            Task::Translate.path()
        ))
        .param("model"));
    }
    {
        let r = guard.get();
        r.alias = Some(req.model.name.clone());
        r.model = Some(req.model.name.clone());
    }
    req.caller.check_model(&req.model.name)?;
    let binary = tokio::task::spawn_blocking(|| run::resolve_binary(None))
        .await
        .map_err(join_failed)?
        .map_err(|e| map_error(&e))?;
    let vad = catalog::ensure_vad().await.map_err(|e| map_error(&e))?;
    let transcript = run::transcribe(
        &binary,
        &run::TranscribeRequest {
            model: &req.model,
            vad: &vad,
            language: req.language.as_deref(),
            threads: None,
        },
        req.audio.to_vec(),
    )
    .await
    .map_err(|e| map_error(&e))?;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);

    let (content_type, body) = shape(req.format, &transcript);
    guard.finish(200, None);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header("x-harness-model", &req.model.name)
        .header(
            "x-harness-audio-seconds",
            format!("{:.2}", transcript.duration_secs),
        )
        .header("x-harness-asr-ms", transcript.elapsed_ms.to_string())
        .body(axum::body::Body::from(body))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::asr::Segment;

    fn transcript() -> Transcript {
        Transcript::new(
            vec![
                Segment {
                    start: 0.1,
                    end: 2.0,
                    text: "Merhaba dünya.".into(),
                },
                Segment {
                    start: 2.5,
                    end: 4.0,
                    text: "İkinci cümle.".into(),
                },
            ],
            4.5,
            Some("tr".into()),
            "m".into(),
            10,
        )
    }

    #[test]
    fn every_response_format_is_shaped() {
        let t = transcript();
        let (ct, body) = shape("json", &t);
        assert_eq!(ct, "application/json");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"text": "Merhaba dünya. İkinci cümle."})
        );
        let (_, body) = shape("verbose_json", &t);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["language"], "tr");
        assert_eq!(v["duration"], 4.5);
        assert_eq!(v["segments"][1]["start"], 2.5);
        assert_eq!(v["segments"][1]["text"], "İkinci cümle.");
        let (ct, body) = shape("text", &t);
        assert_eq!(ct, "text/plain; charset=utf-8");
        assert_eq!(body, "Merhaba dünya. İkinci cümle.\n".as_bytes());
        let (ct, body) = shape("srt", &t);
        assert!(ct.starts_with("application/x-subrip"));
        assert!(String::from_utf8(body)
            .unwrap()
            .starts_with("1\n00:00:00,100 --> 00:00:02,000\nMerhaba dünya.\n"));
        let (ct, body) = shape("vtt", &t);
        assert!(ct.starts_with("text/vtt"));
        assert!(String::from_utf8(body)
            .unwrap()
            .starts_with("WEBVTT\n\n00:00:00.100"));
    }

    #[test]
    fn core_errors_become_openai_errors() {
        let e = map_error(&asr::find_model(&[], None, true).unwrap_err());
        assert_eq!((e.status, e.code.as_str()), (404, "model_not_found"));
        assert!(e.message.contains("harness asr install"));
        let e = map_error(&asr::find_model(&[], Some("x"), true).unwrap_err());
        assert_eq!(e.param.as_deref(), Some("model"));
        assert!(e.message.contains("`x`"));
        let e = map_error(
            &error_key::err("stt.needs_ffmpeg")
                .with("format", "WebM")
                .to_string(),
        );
        assert_eq!(
            (e.status, e.code.as_str()),
            (415, "unsupported_audio_format")
        );
        assert!(e.message.starts_with("WebM audio needs ffmpeg"));
        let e = map_error("runtime.family_not_installed: sherpa-onnx");
        assert_eq!((e.status, e.code.as_str()), (503, "asr_runtime_missing"));
        let e = map_error(
            &error_key::err("stt.timeout")
                .num("seconds", 300)
                .to_string(),
        );
        assert_eq!(e.status, 504);
        let e = map_error(
            &error_key::err("stt.run_failed")
                .with("detail", "bad header")
                .to_string(),
        );
        assert_eq!((e.status, e.code.as_str()), (422, "invalid_audio"));
        assert!(e.message.contains("bad header"));
        assert_eq!(map_error("something else").status, 500);
    }
}
