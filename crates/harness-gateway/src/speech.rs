//! Text to speech: `POST /v1/audio/speech` (OpenAI's audio API, JSON in, audio out), served by
//! the sherpa-onnx runtime ([`harness_core::tts`]).
//!
//! The request names a text-to-speech model (`model`: the name of a model folder in the library
//! such as `kokoro-int8-multi-lang-v1_0`, a part of it, or an OpenAI name such as `tts-1`,
//! which means the default model) and a voice (`voice`: a voice of the model, one of OpenAI's
//! names, which Kokoro answers with a voice of its own, or a speaker number).
//!
//! sherpa-onnx has no server: each request starts `sherpa-onnx-offline-tts`, which loads the
//! model, writes a WAV and exits, so a model is loaded just in time and released at once (a
//! time-to-live of zero), and requests run side by side up to the runtime's limit. `wav` is
//! the tool's own output; `pcm` is its samples; `mp3`, `opus`, `aac` and `flac` are made by
//! ffmpeg when one is installed and refused with a message that says so when not.

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use harness_core::error_key;
use harness_core::gateway::record::{ClientInfo, RequestRecord};
use harness_core::token_policy::GatewayEndpoint;
use harness_core::tts::{self, run, AudioFormat};
use serde::Deserialize;

use crate::errors::GatewayError;
use crate::proxy::{header_str, request_id, trace_context, unix_ms, RecordGuard};
use crate::GwState;

/// The JSON body of a request, as OpenAI names the fields. Fields it has that a local model
/// cannot use (`instructions`, `stream_format`) are accepted and ignored: clients send them by
/// default.
#[derive(Debug, Default, Deserialize)]
pub struct SpeechRequest {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub input: Option<String>,
    #[serde(default)]
    pub voice: Option<serde_json::Value>,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub speed: Option<f64>,
}

impl SpeechRequest {
    /// `voice` is a string, or an object `{"id": "..."}` (the newer OpenAI shape).
    pub fn voice_name(&self) -> Option<String> {
        match self.voice.as_ref()? {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Object(o) => o.get("id").and_then(|v| v.as_str()).map(String::from),
            _ => None,
        }
    }
}

fn bad(message: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

/// The gateway's answer to a keyed error of the text-to-speech core.
pub fn map_error(e: &str) -> GatewayError {
    let Some((key, params)) = error_key::parse(e) else {
        return GatewayError::new(500, "tts_failed", e.to_string());
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
        "tts.input_empty" => bad("`input` must not be empty.").param("input"),
        "tts.input_too_long" => bad(format!(
            "`input` is {} characters; the limit is {}. Split the text and send the parts one after another.",
            text("length"),
            text("max")
        ))
        .param("input"),
        "tts.speed_out_of_range" => bad(format!(
            "`speed` must be between {} and {}.",
            text("min"),
            text("max")
        ))
        .param("speed"),
        "tts.no_model" => GatewayError::new(
            404,
            "model_not_found",
            "No text-to-speech model is installed. Install one in the Playground (Speech tab) or with `harness tts install kokoro-int8-multi-lang-v1_0`.",
        )
        .param("model"),
        "tts.model_not_found" => GatewayError::new(
            404,
            "model_not_found",
            format!(
                "`{}` is not a text-to-speech model. List them with `harness tts ls`.",
                text("model")
            ),
        )
        .param("model"),
        "tts.model_ambiguous" => bad(format!(
            "`{}` matches several text-to-speech models: {}.",
            text("model"),
            text("candidates")
        ))
        .param("model"),
        "tts.voice_not_found" => bad(format!(
            "`{}` is not a voice of {}. Voices: {}.",
            text("voice"),
            text("model"),
            text("available")
        ))
        .param("voice"),
        "tts.format_needs_ffmpeg" => GatewayError::new(
            400,
            "format_unavailable",
            format!(
                "{} output needs ffmpeg and none was found; install ffmpeg, or use wav or pcm.",
                text("format")
            ),
        )
        .param("response_format"),
        "tts.windows_codepage" => GatewayError::new(
            422,
            "unsupported_text",
            "This Windows installation uses a double-byte code page, which cannot carry the characters of the text to the speech engine. Turn on \"Beta: Use Unicode UTF-8 for worldwide language support\" in the Windows region settings, or use ASCII text.",
        )
        .param("input"),
        "runtime.family_not_installed" | "runtime.not_found" => GatewayError::new(
            503,
            "tts_runtime_missing",
            "The sherpa-onnx runtime is not installed. Install it in the Playground (Speech tab) or with `harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx`.",
        ),
        "tts.timeout" => GatewayError::new(
            504,
            "upstream_timeout",
            format!("Speech synthesis took longer than {} seconds.", text("seconds")),
        ),
        _ => GatewayError::new(500, "tts_failed", format!("{key}: {}", e)),
    }
}

/// `POST /v1/audio/speech`.
pub async fn handle(state: GwState, headers: HeaderMap, body: Bytes) -> Response {
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: "/v1/audio/speech".to_string(),
            ..Default::default()
        },
        trace_context(&headers),
    );
    let id = guard.get().id.clone();
    let result = serve(&state, &headers, body, &mut guard).await;
    let mut resp = match result {
        Ok(resp) => resp,
        Err(err) => {
            guard.finish(err.status, Some(&err.code));
            err.into_response()
        }
    };
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert("x-harness-request-id", v);
    }
    if let Some(rate) = guard.rate {
        crate::auth::rate_headers(resp.headers_mut(), &rate, false);
    }
    resp
}

async fn serve(
    state: &GwState,
    headers: &HeaderMap,
    body: Bytes,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Audio)?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    let req: SpeechRequest =
        serde_json::from_slice(&body).map_err(|e| bad(format!("The body must be JSON ({e}).")))?;
    guard.get().model = req.model.clone();
    guard.admitted(caller.admit(state)?);

    let input = req
        .input
        .as_deref()
        .ok_or_else(|| bad("`input` (the text to speak) is required.").param("input"))?;
    let (text, speed) =
        tts::check_request(input, req.speed.map(|s| s as f32)).map_err(|e| map_error(&e))?;
    let format = match req
        .response_format
        .as_deref()
        .filter(|f| !f.trim().is_empty())
    {
        None => AudioFormat::Mp3,
        Some(f) => AudioFormat::parse(f).ok_or_else(|| {
            bad(format!(
                "response_format `{f}` is not supported; use one of {}.",
                AudioFormat::ALL.map(AudioFormat::as_str).join(", ")
            ))
            .param("response_format")
        })?,
    };
    // OpenAI's default is mp3; without ffmpeg an unnamed format falls back to wav, a named one
    // that cannot be made is an error.
    let format = if req.response_format.is_none() && !run::format_available(format) {
        AudioFormat::Wav
    } else {
        format
    };
    if !run::format_available(format) {
        return Err(map_error(
            &error_key::err("tts.format_needs_ffmpeg")
                .with("format", format.as_str())
                .to_string(),
        ));
    }

    let wanted = req.model.clone();
    let models = tokio::task::spawn_blocking(tts::installed_models)
        .await
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?;
    let model = tts::find_model(&models, wanted.as_deref()).map_err(|e| map_error(&e))?;
    {
        let r = guard.get();
        r.alias = Some(model.name.clone());
        r.model = Some(model.name.clone());
    }
    caller.check_model(&model.name)?;
    let binary = tokio::task::spawn_blocking(|| run::resolve_binary(None))
        .await
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?
        .map_err(|e| map_error(&e))?;

    let voice = req.voice_name();
    let speech = run::synthesize(
        &binary,
        &run::SynthesisRequest {
            model,
            voice: voice.as_deref(),
            input: &text,
            speed,
            threads: None,
        },
    )
    .await
    .map_err(|e| map_error(&e))?;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);

    let (seconds, rate, voice_used) = (
        speech.duration_secs,
        speech.sample_rate,
        speech.voice.clone(),
    );
    let audio = run::encode(speech.wav, format)
        .await
        .map_err(|e| GatewayError::new(500, "encode_failed", e))?;
    guard.finish(200, None);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, format.content_type())
        .header("x-harness-model", &model.name)
        .header("x-harness-voice", voice_used)
        .header("x-harness-audio-seconds", format!("{seconds:.2}"))
        .header("x-harness-sample-rate", rate.to_string())
        .header("x-harness-synthesis-ms", speech.elapsed_ms.to_string())
        .body(axum::body::Body::from(audio))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_voice_may_be_a_name_a_number_or_an_object() {
        let parse = |json: &str| -> SpeechRequest { serde_json::from_str(json).unwrap() };
        assert_eq!(
            parse(r#"{"voice":"alloy"}"#).voice_name().as_deref(),
            Some("alloy")
        );
        assert_eq!(parse(r#"{"voice":3}"#).voice_name().as_deref(), Some("3"));
        assert_eq!(
            parse(r#"{"voice":{"id":"af_heart"}}"#)
                .voice_name()
                .as_deref(),
            Some("af_heart")
        );
        assert_eq!(parse(r#"{}"#).voice_name(), None);
        assert_eq!(parse(r#"{"voice":null}"#).voice_name(), None);
        // Fields a local model cannot use are accepted.
        let req = parse(
            r#"{"model":"tts-1","input":"Hi","voice":"nova","instructions":"cheerful","stream_format":"audio","response_format":"wav","speed":1.5}"#,
        );
        assert_eq!(req.model.as_deref(), Some("tts-1"));
        assert_eq!(req.input.as_deref(), Some("Hi"));
        assert_eq!(req.response_format.as_deref(), Some("wav"));
        assert_eq!(req.speed, Some(1.5));
    }

    #[test]
    fn core_errors_become_openai_errors() {
        let e = map_error(&tts::check_request("", None).unwrap_err());
        assert_eq!((e.status, e.code.as_str()), (400, "invalid_request"));
        assert_eq!(e.param.as_deref(), Some("input"));
        let e = map_error(&tts::check_request(&"x".repeat(5000), None).unwrap_err());
        assert!(
            e.message.contains("5000") && e.message.contains("4096"),
            "{}",
            e.message
        );
        let e = map_error(&tts::check_request("x", Some(9.0)).unwrap_err());
        assert_eq!(e.param.as_deref(), Some("speed"));
        let e = map_error(&tts::find_model(&[], None).unwrap_err());
        assert_eq!((e.status, e.code.as_str()), (404, "model_not_found"));
        assert!(e.message.contains("harness tts install"));
        let e = map_error("runtime.family_not_installed: sherpa-onnx");
        assert_eq!((e.status, e.code.as_str()), (503, "tts_runtime_missing"));
        let e = map_error(
            &error_key::err("tts.voice_not_found")
                .with("voice", "morgan")
                .with("model", "kokoro")
                .with("available", "af_heart, af_bella")
                .to_string(),
        );
        assert_eq!(e.param.as_deref(), Some("voice"));
        assert!(e.message.contains("af_heart, af_bella"));
        let e = map_error(
            &error_key::err("tts.format_needs_ffmpeg")
                .with("format", "mp3")
                .to_string(),
        );
        assert_eq!(e.code, "format_unavailable");
        assert_eq!(map_error("something else").status, 500);
    }
}
