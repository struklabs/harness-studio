//! `/v1/audio/speech` with a llama.cpp text-to-speech model (Qwen3-TTS) through `llama-tts`
//! ([`harness_core::tts::llama`]): one request at a time, the text in a file, a frame limit per
//! run of sentences and the gateway's request timeout; a partial answer carries
//! `x-harness-truncated: frames` or `time`.

use std::time::Duration;

use axum::http::{header, StatusCode};
use axum::response::Response;
use harness_core::tts::llama::{self, TtsModel};
use harness_core::tts::{run, AudioFormat};

use crate::errors::GatewayError;
use crate::proxy::RecordGuard;
use crate::speech::SpeechRequest;
use crate::GwState;

fn bad(message: impl Into<String>, param: &'static str) -> GatewayError {
    GatewayError::new(400, "invalid_request", message).param(param)
}

/// What a request asks of a llama-tts model, checked before anything starts.
#[derive(Debug, PartialEq)]
pub struct Checked {
    pub text: String,
    pub language: &'static str,
    pub format: AudioFormat,
}

/// `language` is the request's `language` field (an extension; OpenAI's API has none).
pub fn check(
    req: &SpeechRequest,
    language: Option<&str>,
    alias: &str,
) -> Result<Checked, GatewayError> {
    let input = req
        .input
        .as_deref()
        .ok_or_else(|| bad("`input` (the text to speak) is required.", "input"))?;
    let (text, speed) = harness_core::tts::check_request(input, req.speed.map(|s| s as f32))
        .map_err(|e| crate::speech::map_error(&e))?;
    if speed != 1.0 {
        return Err(bad(
            format!("`{alias}` speaks at one speed; use speed 1."),
            "speed",
        ));
    }
    if req.voice_ref.is_some() || !llama::voice_ok(req.voice_name().as_deref()) {
        return Err(bad(
            format!(
                "`{alias}` speaks with its own voice: send no voice, `default` or an OpenAI voice name. Voice cloning is not offered."
            ),
            "voice",
        ));
    }
    let language = llama::language_code(language).map_err(|e| bad(e, "language"))?;
    let format = match req
        .response_format
        .as_deref()
        .filter(|f| !f.trim().is_empty())
    {
        None => AudioFormat::Wav,
        Some(f) => AudioFormat::parse(f).ok_or_else(|| {
            bad(
                format!(
                    "response_format `{f}` is not supported; use one of {}.",
                    AudioFormat::ALL.map(AudioFormat::as_str).join(", ")
                ),
                "response_format",
            )
        })?,
    };
    if !run::format_available(format) {
        return Err(bad(
            "This output format requires ffmpeg; use wav or pcm.",
            "response_format",
        ));
    }
    Ok(Checked {
        text,
        language,
        format,
    })
}

/// Speaks one request.
pub async fn speak(
    state: &GwState,
    model: TtsModel,
    alias: &str,
    checked: Checked,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let runtime = state.scheduler.runtime().clone();
    let server = tokio::task::spawn_blocking(move || {
        harness_core::runtimes::resolve_binary(
            &runtime,
            &harness_core::config::AppSettings::load(),
            None,
        )
    })
    .await
    .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?
    .map_err(|e| GatewayError::new(503, "runtime_missing", e))?;
    let binary = llama::binary_next_to(&server).ok_or_else(|| {
        GatewayError::new(
            503,
            "runtime_missing",
            "The llama.cpp runtime has no llama-tts (it needs llama.cpp b10270 or newer); update the runtime.",
        )
    })?;
    let limit = match state.scheduler.settings().request_timeout_secs {
        0 => None,
        n => Some(Duration::from_secs(u64::from(n))),
    };
    let speech = llama::speak(&binary, &model, &checked.text, checked.language, limit)
        .await
        .map_err(|e| {
            if e.contains("in time") {
                GatewayError::new(504, "upstream_timeout", e)
            } else {
                GatewayError::new(502, "upstream_error", e)
            }
        })?;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
    let audio = run::encode(speech.wav, checked.format)
        .await
        .map_err(|e| GatewayError::new(500, "encode_failed", e))?;
    guard.finish(200, None);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, checked.format.content_type())
        .header("x-harness-model", alias)
        .header("x-harness-voice", "default")
        .header(
            "x-harness-audio-seconds",
            format!("{:.2}", speech.duration_secs),
        )
        .header("x-harness-sample-rate", speech.sample_rate.to_string())
        .header("x-harness-synthesis-ms", speech.elapsed_ms.to_string());
    if let Some(cut) = speech.cut {
        builder = builder.header("x-harness-truncated", cut.as_str());
    }
    builder
        .body(axum::body::Body::from(audio))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(body: serde_json::Value) -> SpeechRequest {
        serde_json::from_value(body).unwrap()
    }

    #[test]
    fn requests_are_checked_before_llama_tts_starts() {
        let ok = check(&req(serde_json::json!({"input": " Hello. "})), None, "q").unwrap();
        assert_eq!(
            ok,
            Checked {
                text: "Hello.".into(),
                language: "en",
                format: AudioFormat::Wav
            }
        );
        let de = check(
            &req(serde_json::json!({"input": "Hallo", "voice": "alloy", "response_format": "pcm"})),
            Some("de"),
            "q",
        )
        .unwrap();
        assert_eq!((de.language, de.format), ("de", AudioFormat::Pcm));
        let cases = [
            (serde_json::json!({}), None, "input"),
            (
                serde_json::json!({"input": "a", "speed": 1.5}),
                None,
                "speed",
            ),
            (
                serde_json::json!({"input": "a", "voice": "Vivian"}),
                None,
                "voice",
            ),
            (
                serde_json::json!({"input": "a", "voice_ref": {"file": "me.wav"}}),
                None,
                "voice",
            ),
            (serde_json::json!({"input": "a"}), Some("tr"), "language"),
            (
                serde_json::json!({"input": "a", "response_format": "midi"}),
                None,
                "response_format",
            ),
        ];
        for (body, language, param) in cases {
            let err = check(&req(body.clone()), language, "q").unwrap_err();
            assert_eq!(err.status, 400, "{body}");
            assert_eq!(err.param.as_deref(), Some(param), "{body}");
        }
    }
}
