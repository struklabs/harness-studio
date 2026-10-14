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
    #[serde(default)]
    pub voice_ref: Option<serde_json::Value>,
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
    if let Ok(def) = state.scheduler.resolve(req.model.as_deref()).await {
        if crate::audio::is_crispasr_model(&def) {
            return Err(
                bad("CrispASR only accepts audio transcription and translation requests")
                    .param("model"),
            );
        }
    }
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
    let model = match tts::find_model(&models, wanted.as_deref()) {
        Ok(model) => model,
        Err(e) => {
            // No sherpa-onnx voice by that name: a text-to-speech model of the Python engine or
            // of mlx-audio may have it (sherpa-onnx comes first, the workers are the fallback).
            if let Some(def) = python_speech_model(state, wanted.as_deref()).await {
                caller.check_model(&def.alias)?;
                return python_speak(state, &def, &text, &req, speed, format, guard).await;
            }
            return Err(map_error(&e));
        }
    };
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

/// The definition `model` names when it is a text-to-speech model of the Python engine or of
/// mlx-audio.
async fn python_speech_model(
    state: &GwState,
    model: Option<&str>,
) -> Option<harness_core::domain::ModelDefinition> {
    let def = match model.filter(|m| !m.is_empty() && !matches!(*m, "tts-1" | "tts-1-hd")) {
        Some(model) => state.scheduler.resolve(Some(model)).await.ok()?,
        None => tokio::task::spawn_blocking(|| {
            harness_core::domain::store::Domain::global()
                .definitions()
                .ok()?
                .into_iter()
                .find(is_audio_cpp_speech)
        })
        .await
        .ok()??,
    };
    if is_mlx_speech(&def) || is_audio_cpp_speech(&def) {
        return Some(def);
    }
    crate::pyhf::task_of(&def)
        .filter(|t| crate::pyhf::is_speech_task(t))
        .map(|_| def)
}

fn is_audio_cpp_speech(def: &harness_core::domain::ModelDefinition) -> bool {
    def.base
        .local_path()
        .is_some_and(harness_core::engine::audio_cpp::is_tts)
}

pub(crate) fn is_audio_cpp_model(def: &harness_core::domain::ModelDefinition) -> bool {
    def.base
        .local_path()
        .is_some_and(harness_core::engine::audio_cpp::is_model)
}

pub(crate) fn check_audio_cpp_enabled() -> Result<(), GatewayError> {
    harness_core::engine::audio_cpp::require_enabled()
        .map_err(|e| GatewayError::new(403, "engine_disabled", e))
}

/// The entire audio.cpp HTTP surface exposed by Harness. Never pass a caller's object
/// through: even nested reference fields are rebuilt, since the worker accepts local paths.
pub(crate) fn audio_cpp_body(
    def: &harness_core::domain::ModelDefinition,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<serde_json::Value, GatewayError> {
    if method != "POST" || path != "/v1/audio/speech" {
        return Err(bad(
            "audio.cpp only accepts POST /v1/audio/speech through the gateway.",
        ));
    }
    let req: SpeechRequest = serde_json::from_slice(body)
        .map_err(|e| bad(format!("The body must be speech JSON ({e}).")))?;
    let voices = def
        .base
        .local_path()
        .map(harness_core::engine::audio_cpp::voices)
        .unwrap_or_default();
    let supports_speed = def
        .base
        .local_path()
        .and_then(harness_core::engine::audio_cpp::metadata)
        .and_then(|m| m.audiocpp_family)
        .is_some_and(|f| harness_core::engine::audio_cpp::supports_speed(&f));
    rebuild_audio_cpp_body(&def.alias, &voices, &req, supports_speed)
}

fn rebuild_audio_cpp_body(
    model: &str,
    voices: &[String],
    req: &SpeechRequest,
    supports_speed: bool,
) -> Result<serde_json::Value, GatewayError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let input = req
        .input
        .as_deref()
        .ok_or_else(|| bad("`input` is required."))?;
    let (input, speed) =
        tts::check_request(input, req.speed.map(|s| s as f32)).map_err(|e| map_error(&e))?;
    let mut out = serde_json::json!({"model": model, "input": input,
        "response_format": "wav"});
    if req.speed.unwrap_or(1.0) != 1.0 {
        if !supports_speed {
            return Err(bad(
                "This audio.cpp model does not support runtime speed controls; use speed 1.0.",
            )
            .param("speed"));
        }
        out["speed"] = serde_json::json!(speed);
    }
    let voice = match &req.voice {
        None | Some(serde_json::Value::Null) => voices.first().cloned(),
        Some(_) => Some(
            req.voice_name()
                .ok_or_else(|| bad("Invalid `voice`.").param("voice"))?,
        ),
    };
    if let Some(voice) = voice {
        if !voices.contains(&voice) {
            return Err(
                bad("`voice` must be one of this model's discovered voices.").param("voice"),
            );
        }
        out["voice"] = serde_json::json!(voice);
    }
    if let Some(reference) = &req.voice_ref {
        if reference.get("type").and_then(|v| v.as_str()) != Some("base64") {
            return Err(bad(
                "`voice_ref` only accepts a base64 object; server file paths are forbidden.",
            )
            .param("voice_ref"));
        }
        let data = reference
            .get("data")
            .and_then(|v| v.as_str())
            .ok_or_else(|| bad("`voice_ref.data` must be base64."))?;
        let data = data.strip_prefix("data:audio/wav;base64,").unwrap_or(data);
        const LIMIT: usize = 5 * 1024 * 1024;
        if data.len() > LIMIT.div_ceil(3) * 4 {
            return Err(bad("`voice_ref` decoded size must not exceed 5 MiB."));
        }
        let decoded = STANDARD
            .decode(data)
            .map_err(|_| bad("Invalid `voice_ref` base64."))?;
        if decoded.len() > LIMIT {
            return Err(bad("`voice_ref` decoded size must not exceed 5 MiB."));
        }
        out["voice_ref"] = serde_json::json!({"type":"base64", "data":STANDARD.encode(decoded)});
    }
    Ok(out)
}

/// Whether a definition is an mlx-audio text-to-speech folder.
fn is_mlx_speech(def: &harness_core::domain::ModelDefinition) -> bool {
    def.base
        .local_path()
        .is_some_and(harness_core::engine::mlx::is_tts_dir)
}

/// Speech from a Python engine or mlx-audio model: the worker's WAV, encoded to the asked
/// format like sherpa-onnx's. The Python engine's models have one voice and one speed; an
/// mlx-audio model takes `voice` (one of its speakers) and `speed`.
async fn python_speak(
    state: &GwState,
    def: &harness_core::domain::ModelDefinition,
    text: &str,
    req: &SpeechRequest,
    speed: f32,
    format: AudioFormat,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    {
        let r = guard.get();
        r.alias = Some(def.alias.clone());
        r.definition_id = Some(def.id.clone());
    }
    let voice_name = req.voice_name();
    let voice = voice_name.as_deref();
    let mut request = serde_json::json!({ "input": text });
    if is_audio_cpp_speech(def) {
        request["model"] = serde_json::json!(def.alias);
        request["response_format"] = serde_json::json!("wav");
        if req.speed.unwrap_or(1.0) != 1.0 {
            request["speed"] = serde_json::json!(req.speed);
        }
        // These are untrusted fields. pyhf::post validates the speech-only route and
        // rebuilds the audio.cpp body (including voice/voice_ref) before acquiring a worker.
        request["voice"] = req.voice.clone().unwrap_or(serde_json::Value::Null);
        if let Some(reference) = &req.voice_ref {
            request["voice_ref"] = reference.clone();
        }
    }
    if is_mlx_speech(def) {
        request["speed"] = serde_json::json!(speed);
        if let Some(voice) = voice {
            request["voice"] = serde_json::json!(voice);
        }
    }
    let body = serde_json::to_vec(&request)
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?;
    let answer = crate::pyhf::post(
        state,
        def,
        guard,
        "/v1/audio/speech",
        "application/json",
        body,
    )
    .await?;
    let audio = run::encode(answer.body.to_vec(), format)
        .await
        .map_err(|e| GatewayError::new(500, "encode_failed", e))?;
    guard.finish(200, None);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, format.content_type());
    if answer.truncated {
        builder = builder.header("x-harness-truncated", "time");
    }
    crate::pyhf::lease_headers(builder, def, &answer.lease)
        .body(axum::body::Body::from(audio))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_cpp_omits_default_speed_and_rejects_unsupported_speed() {
        for speed in [None, Some(1.0)] {
            let req = serde_json::from_value(serde_json::json!({"input":"Hello", "speed":speed}))
                .unwrap();
            for supported in [false, true] {
                assert!(rebuild_audio_cpp_body("voice", &[], &req, supported)
                    .unwrap()
                    .get("speed")
                    .is_none());
            }
        }
        let req =
            serde_json::from_value(serde_json::json!({"input":"Hello", "speed":1.25})).unwrap();
        let err = rebuild_audio_cpp_body("voice", &[], &req, false).unwrap_err();
        assert_eq!(err.status, 400);
        assert_eq!(err.param.as_deref(), Some("speed"));
        assert!(err.message.contains("does not support"));
        assert_eq!(
            rebuild_audio_cpp_body("voice", &[], &req, true).unwrap()["speed"],
            1.25
        );
    }

    #[test]
    fn audio_cpp_rejects_every_other_method_and_path() {
        use harness_core::domain::{ModelDefinition, ModelRef};
        let def = ModelDefinition::new(
            "voice",
            ModelRef::Local {
                path: "unused.gguf".into(),
            },
        );
        for path in [
            "/v1/audio/transcriptions",
            "/v1/audio/translations",
            "/v1/audio/alignments",
            "/v1/tasks/run",
            "/v1/tasks/batch",
            "/v1/tasks/unload_all_models",
            "/v1/models/load",
            "/v1/models",
            "/v1/chat/completions",
            "/health",
            "/v1/audio/speech/live",
            "/v1/audio/speech?path=file",
            "/v1/audio/speech/",
        ] {
            assert_eq!(
                audio_cpp_body(&def, "POST", path, b"{}")
                    .unwrap_err()
                    .status,
                400,
                "{path}"
            );
        }
        for method in ["GET", "PUT", "DELETE", "PATCH", "OPTIONS"] {
            assert_eq!(
                audio_cpp_body(&def, method, "/v1/audio/speech", b"{}")
                    .unwrap_err()
                    .status,
                400
            );
        }
    }

    #[test]
    fn audio_cpp_rebuilds_only_allowed_fields_and_discovered_voices() {
        use serde_json::json;
        let req = serde_json::from_value(json!({"model":"untrusted", "input":"Hello", "voice":"alba",
            "speed":1.5, "response_format":"wav", "voice_ref":{"type":"base64","data":"YQ==", "path":"secret"},
            "audio":"secret", "options":{"audio":"secret"}, "reference_text":"ignored"})).unwrap();
        let out = rebuild_audio_cpp_body("pocket", &["alba".into()], &req, true).unwrap();
        assert_eq!(
            out,
            json!({"model":"pocket","input":"Hello","voice":"alba","speed":1.5,
            "response_format":"wav","voice_ref":{"type":"base64","data":"YQ=="}})
        );
        for voice in [
            json!("../secret"),
            json!("unknown"),
            json!({"path":"secret"}),
            json!([]),
        ] {
            let req = serde_json::from_value(json!({"input":"Hi","voice":voice})).unwrap();
            assert_eq!(
                rebuild_audio_cpp_body("pocket", &["alba".into()], &req, true)
                    .unwrap_err()
                    .status,
                400
            );
        }
    }

    #[test]
    fn audio_cpp_rejects_path_invalid_and_oversized_references() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use serde_json::json;
        for reference in [
            json!("secret.wav"),
            json!({"type":"path","path":"secret.wav"}),
            json!({"type":"base64","data":"!"}),
            json!({"type":"base64"}),
            json!(false),
            json!({"type":"base64","data":STANDARD.encode(vec![0; 5 * 1024 * 1024 + 1])}),
        ] {
            let req = serde_json::from_value(json!({"input":"Hi","voice_ref":reference})).unwrap();
            assert_eq!(
                rebuild_audio_cpp_body("pocket", &[], &req, true)
                    .unwrap_err()
                    .status,
                400
            );
        }
        let req = serde_json::from_value(json!({"input":"Hi","voice_ref":{
            "type":"base64","data":STANDARD.encode(vec![0; 5 * 1024 * 1024])}}))
        .unwrap();
        assert!(rebuild_audio_cpp_body("pocket", &[], &req, true).is_ok());
    }

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
