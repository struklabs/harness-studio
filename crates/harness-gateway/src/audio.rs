//! Speech to text: `POST /v1/audio/transcriptions` and `/v1/audio/translations` (OpenAI's
//! audio API, multipart), served by whisper-server.
//!
//! The request names a speech recognition model (`model`, a definition whose file is a whisper
//! ggml `.bin`); the scheduler loads it on demand and unloads it after its TTL like any other
//! model, and its memory counts in the same VRAM budget. whisper-server itself has no OpenAI
//! surface (`POST /inference`, no authentication, loopback only), so this module
//!
//! * reads the multipart form (`file`, `model`, `language`, `prompt`, `response_format`,
//!   `temperature`),
//! * hands containers whisper-server cannot decode (M4A, WebM / Opus, Ogg Opus, AAC) to ffmpeg
//!   ([`harness_core::audio`]), or refuses them with a message that says ffmpeg is missing,
//! * forwards the audio under whisper-server's own form fields and
//! * shapes the answer like OpenAI's (`{"text": ...}` without the leading space whisper puts in
//!   front of a transcript, `verbose_json`, `text`, `srt`, `vtt`).
//!
//! `/v1/audio/translations` is whisper's `translate` task: the transcript in English.
//!
//! A `model` the scheduler does not know may be an installed sherpa-onnx recognition model
//! (Parakeet, SenseVoice, Omnilingual...): [`crate::sherpa_asr`] serves those, the whisper path
//! above is unchanged.

use std::time::Duration;

use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use harness_core::audio::{self, Container};
use harness_core::domain::ModelDefinition;
use harness_core::gateway::record::{ClientInfo, RequestRecord};
use harness_core::gateway::scheduler::SchedError;
use harness_core::token_policy::GatewayEndpoint;

use crate::errors::GatewayError;
use crate::proxy::{header_str, request_id, trace_context, unix_ms, RecordGuard};
use crate::GwState;

/// What the request asks whisper to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Transcribe,
    Translate,
}

impl Task {
    pub fn path(self) -> &'static str {
        match self {
            Task::Transcribe => "/v1/audio/transcriptions",
            Task::Translate => "/v1/audio/translations",
        }
    }
}

/// The form fields of a request, as OpenAI names them.
#[derive(Debug, Default)]
struct Form {
    file: Option<(String, Bytes)>,
    model: Option<String>,
    language: Option<String>,
    prompt: Option<String>,
    response_format: Option<String>,
    temperature: Option<String>,
    /// Harness's extra: how many speakers there are (diarization; sherpa-onnx models).
    speakers: Option<String>,
}

/// Response formats whisper-server can produce (`json`, `text`, `srt`, `vtt`, `verbose_json`).
/// `diarized_json` (who spoke each segment) is served by the sherpa-onnx models only.
const FORMATS: [&str; 6] = [
    "json",
    "text",
    "srt",
    "vtt",
    "verbose_json",
    "diarized_json",
];

fn bad(message: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

async fn read_form(mut mp: Multipart) -> Result<Form, GatewayError> {
    let mut form = Form::default();
    while let Some(field) = mp.next_field().await.map_err(|e| {
        bad(format!(
            "The multipart body could not be read: {}",
            e.body_text()
        ))
    })? {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                let filename = field.file_name().unwrap_or("audio").to_string();
                let bytes = field.bytes().await.map_err(|e| {
                    bad(format!(
                        "The audio file could not be read: {}",
                        e.body_text()
                    ))
                })?;
                form.file = Some((filename, bytes));
            }
            "model" | "language" | "prompt" | "response_format" | "temperature" | "speakers" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| bad(format!("Field {name}: {}", e.body_text())))?;
                let text = text.trim().to_string();
                let slot = match name.as_str() {
                    "model" => &mut form.model,
                    "language" => &mut form.language,
                    "prompt" => &mut form.prompt,
                    "response_format" => &mut form.response_format,
                    "speakers" => &mut form.speakers,
                    _ => &mut form.temperature,
                };
                if !text.is_empty() {
                    *slot = Some(text);
                }
            }
            // `timestamp_granularities[]`, `stream`, `chunking_strategy`...: not supported, and
            // not an error (clients send them by default).
            _ => {
                let _ = field.bytes().await;
            }
        }
    }
    Ok(form)
}

/// A multipart/form-data body: `file` first, then the text fields.
fn multipart_body(filename: &str, audio: &[u8], fields: &[(&str, String)]) -> (String, Vec<u8>) {
    let boundary = format!("harness-{:x}", unix_ms());
    let mut body = Vec::with_capacity(audio.len() + 512);
    let name = filename.replace(['"', '\r', '\n'], "_");
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(audio);
    body.extend_from_slice(b"\r\n");
    for (key, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Whether a definition serves speech recognition (its file is a whisper ggml model).
pub fn is_speech_model(def: &ModelDefinition) -> bool {
    use harness_core::domain::ModelRef;
    use harness_core::engine::ModelFormat;
    let path = match &def.base {
        ModelRef::Local { path } => path.clone(),
        ModelRef::Hf { file, .. } => std::path::PathBuf::from(file),
    };
    ModelFormat::of(&path) == ModelFormat::GgmlBin || harness_core::asr::is_onnx_ctc_file(&path)
}

/// Handles one transcription / translation request.
pub async fn handle(
    state: GwState,
    task: Task,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Response {
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: task.path().to_string(),
            ..Default::default()
        },
        trace_context(&headers),
    );
    let id = guard.get().id.clone();
    let result = run(&state, task, &headers, multipart, &mut guard).await;
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

async fn run(
    state: &GwState,
    task: Task,
    headers: &HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Audio)?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    let multipart = multipart.map_err(|e| {
        bad(format!(
            "Send the audio as multipart/form-data with a `file` field ({}).",
            e.body_text()
        ))
    })?;
    let form = read_form(multipart).await?;
    guard.get().model = form.model.clone();
    guard.admitted(caller.admit(state)?);
    let (filename, bytes) = form
        .file
        .clone()
        .filter(|(_, b)| !b.is_empty())
        .ok_or_else(|| {
            bad("The `file` field (the audio to transcribe) is required.").param("file")
        })?;
    let format = form.response_format.as_deref().unwrap_or("json");
    if !FORMATS.contains(&format) {
        return Err(bad(format!(
            "response_format `{format}` is not supported; use one of {}.",
            FORMATS.join(", ")
        ))
        .param("response_format"));
    }
    let speakers = match form.speakers.as_deref() {
        None => None,
        Some(s) => Some(
            s.parse::<u32>()
                .ok()
                .filter(|n| (1..=harness_core::asr::diarize::MAX_SPEAKERS).contains(n))
                .ok_or_else(|| {
                    bad(format!(
                        "speakers must be a whole number from 1 to {}.",
                        harness_core::asr::diarize::MAX_SPEAKERS
                    ))
                    .param("speakers")
                })?,
        ),
    };
    if let Some(t) = form.temperature.as_deref() {
        if t.parse::<f64>().map_or(true, |t| !(0.0..=1.0).contains(&t)) {
            return Err(bad("temperature must be a number between 0 and 1.").param("temperature"));
        }
    }

    let scheduler = state.scheduler.clone();
    let settings = scheduler.settings();
    let def = match scheduler.resolve(form.model.as_deref()).await {
        Ok(def) => def,
        Err(e) => {
            // A model the scheduler does not know may be a sherpa-onnx speech recognition
            // model (Parakeet, SenseVoice...): those have no definition.
            if matches!(e, SchedError::ModelNotFound { .. }) {
                if let Some(model) = crate::sherpa_asr::find(form.model.as_deref()).await? {
                    return crate::sherpa_asr::serve(
                        crate::sherpa_asr::Request {
                            task,
                            model,
                            audio: bytes,
                            language: form.language.clone(),
                            format,
                            speakers,
                            caller: &caller,
                        },
                        guard,
                    )
                    .await;
                }
            }
            return Err(caller.sched_error(&e));
        }
    };
    {
        let r = guard.get();
        r.alias = Some(def.alias.clone());
        r.definition_id = Some(def.id.clone());
    }
    caller.check_model(&def.alias)?;
    if format == "diarized_json" || speakers.is_some() {
        return Err(bad(format!(
            "`{}` cannot tell speakers apart; diarized_json and speakers work with the sherpa-onnx speech recognition models (Parakeet, Nemotron...).",
            def.alias
        ))
        .param("response_format"));
    }
    // A speech recognition model of the Python engine answers OpenAI's request itself (after the
    // audio is in a container it reads, below).
    let python = crate::pyhf::task_of(&def);
    if let Some(model_task) = &python {
        if model_task != "automatic-speech-recognition" {
            return Err(GatewayError::new(
                400,
                "model_not_speech",
                format!(
                    "`{}` is a Python engine model of {model_task}; {} needs a speech recognition model.",
                    def.alias,
                    task.path()
                ),
            )
            .param("model"));
        }
        if !PYTHON_FORMATS.contains(&format) {
            return Err(bad(format!(
                "response_format `{format}` is not available for this model; use json or text."
            ))
            .param("response_format"));
        }
    }
    let onnx_ctc = def
        .base
        .local_path()
        .is_some_and(harness_core::asr::is_onnx_ctc_file);
    if onnx_ctc && task == Task::Translate {
        return Err(
            bad("ONNX CTC models only transcribe; use /v1/audio/transcriptions.").param("model"),
        );
    }
    // A transcription model of the Lemonade engine: its own server takes the audio.
    let lemonade = harness_core::lemonade::model_of_def(&def.id).map(str::to_string);
    if let Some(model) = &lemonade {
        let kind = harness_core::lemonade::find_model(model).map(|m| m.kind);
        if kind != Some(harness_core::lemonade::ModelKind::Transcription) {
            return Err(GatewayError::new(
                400,
                "model_not_speech",
                format!(
                    "`{}` is not a speech recognition model of Lemonade; {} needs one (Whisper-Tiny, Whisper-Base...).",
                    def.alias,
                    task.path()
                ),
            )
            .param("model"));
        }
        if task == Task::Translate {
            return Err(bad(
                "Lemonade's transcription models do not translate; use /v1/audio/transcriptions.",
            )
            .param("model"));
        }
    } else if python.is_none() && !is_speech_model(&def) {
        return Err(GatewayError::new(
            400,
            "model_not_speech",
            format!(
                "`{}` is not a speech recognition model (Whisper ggml .bin or ONNX CTC); {} needs one.",
                def.alias,
                task.path()
            ),
        )
        .param("model"));
    }

    // whisper-server decodes WAV, MP3, FLAC and Ogg Vorbis; anything else goes through ffmpeg.
    let container = audio::sniff(&bytes);
    let audio_bytes = if onnx_ctc {
        Bytes::from(
            harness_core::asr::onnx::prepare_audio(bytes.to_vec())
                .await
                .map_err(|e| {
                    GatewayError::new(
                        if audio::ffmpeg_path().is_none() {
                            415
                        } else {
                            400
                        },
                        "unsupported_audio_format",
                        e,
                    )
                    .param("file")
                })?,
        )
    } else if container.is_native() {
        bytes
    } else if audio::ffmpeg_path().is_some() {
        Bytes::from(audio::to_wav(bytes.to_vec()).await.map_err(|e| {
            GatewayError::new(
                400,
                "audio_convert_failed",
                format!("The audio could not be decoded ({container:?}): {e}"),
            )
            .param("file")
        })?)
    } else if container == Container::Unknown {
        // Not a container Harness knows: let whisper-server say what it thinks of it.
        bytes
    } else {
        return Err(GatewayError::new(
            415,
            "unsupported_audio_format",
            format!(
                "{} audio needs ffmpeg to be converted and none was found; install ffmpeg, or send WAV, MP3, FLAC or Ogg Vorbis.",
                container.name()
            ),
        )
        .param("file"));
    };

    if python.is_some() {
        return python_transcribe(
            state,
            task,
            &def,
            &form,
            &filename,
            &audio_bytes,
            format,
            guard,
        )
        .await;
    }

    let lease = match scheduler.acquire_tokens(&def, None).await {
        Ok(lease) => lease,
        Err(e) => {
            guard.get().oom = e.is_oom();
            return Err(GatewayError::from_sched(&e));
        }
    };
    {
        let r = guard.get();
        r.server_id = Some(lease.server_id.clone());
        r.queue_wait_ms = lease.queue_wait.as_millis() as u64;
        r.slots = Some(lease.slots);
        r.waited = lease.waited.map(str::to_string);
        r.backend = lease.backend.clone();
        if let Some(load) = &lease.load {
            r.load_ms = Some(load.ms);
            r.evicted = load.evicted.clone();
            r.oom = load.recovered;
        }
    }

    let engine_name = if lemonade.is_some() {
        "Lemonade"
    } else if onnx_ctc {
        "ONNX CTC"
    } else {
        "whisper-server"
    };
    let mut fields: Vec<(&str, String)> = vec![
        ("response_format", format.to_string()),
        (
            "temperature",
            form.temperature.clone().unwrap_or_else(|| "0.0".into()),
        ),
    ];
    if let Some(l) = form.language.clone() {
        fields.push(("language", l));
    }
    if let Some(p) = form.prompt.clone() {
        fields.push(("prompt", p));
    }
    if task == Task::Translate {
        fields.push(("translate", "true".into()));
    }
    // Lemonade serves several models: the request says which (its own name for it).
    if let Some(model) = &lemonade {
        fields.push(("model", model.clone()));
    }
    if onnx_ctc {
        fields.retain(|(key, _)| *key == "response_format");
    }
    let filename = if container != Container::Wav && onnx_ctc {
        "audio.wav"
    } else {
        &filename
    };
    let (content_type, body) = if onnx_ctc {
        harness_core::asr::onnx::multipart(&audio_bytes, format, &def.alias)
    } else {
        multipart_body(filename, &audio_bytes, &fields)
    };
    let timeout = match settings.request_timeout_secs {
        0 => None,
        n => Some(Duration::from_secs(u64::from(n))),
    };
    let url = if lemonade.is_some() || onnx_ctc {
        format!("{}/v1/audio/transcriptions", lease.upstream)
    } else {
        format!("{}/inference", lease.upstream)
    };
    let mut req = state
        .http
        .post(url)
        .header(header::CONTENT_TYPE, content_type)
        .body(body);
    if let Some(key) = &lease.api_key {
        req = req.bearer_auth(key);
    }
    if let Some(t) = timeout {
        req = req.timeout(t);
    }
    let upstream = req.send().await.map_err(|e| {
        if e.is_timeout() {
            GatewayError::new(
                504,
                "upstream_timeout",
                format!("{engine_name} did not answer in time."),
            )
        } else {
            GatewayError::new(502, "upstream_unreachable", format!("{engine_name}: {e}"))
        }
    })?;
    let status = upstream.status().as_u16();
    guard.get().status = status;
    let bytes = upstream
        .bytes()
        .await
        .map_err(|e| GatewayError::new(502, "upstream_error", e.to_string()))?;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);

    if status != 200 {
        // whisper-server answers `{"error": "..."}` (or a bare text) with the failure.
        let detail = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(&bytes).trim().to_string());
        let code = if status == 400 {
            "invalid_audio"
        } else {
            "upstream_error"
        };
        let message = format!("{engine_name}: {detail}");
        return Err(GatewayError::new(
            if status == 400 { 400 } else { 502 },
            code,
            message,
        ));
    }

    let (content_type, out) = shape(format, &bytes);
    guard.finish(200, None);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header("x-harness-model", &def.alias)
        .header("x-harness-server", &lease.server_id)
        .header(
            "x-harness-queue-ms",
            lease.queue_wait.as_millis().to_string(),
        );
    if let Some(load) = &lease.load {
        builder = builder.header("x-harness-load-ms", load.ms.to_string());
        if !load.evicted.is_empty() {
            builder = builder.header("x-harness-evicted", load.evicted.join(","));
        }
    }
    drop(lease);
    builder
        .body(axum::body::Body::from(out))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

/// `response_format`s a Python engine model answers (its worker returns the text only).
const PYTHON_FORMATS: [&str; 2] = ["json", "text"];

/// A transcription (or a Whisper translation) on a Python engine model: the audio goes to the
/// worker's `/v1/audio/transcriptions`; `text` is made from its JSON here.
#[allow(clippy::too_many_arguments)]
async fn python_transcribe(
    state: &GwState,
    task: Task,
    def: &ModelDefinition,
    form: &Form,
    filename: &str,
    audio: &[u8],
    format: &str,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let mut fields: Vec<(&str, String)> = Vec::new();
    if let Some(l) = form.language.clone() {
        fields.push(("language", l));
    }
    if task == Task::Translate {
        fields.push(("task", "translate".into()));
    }
    let (content_type, body) = multipart_body(filename, audio, &fields);
    let answer = crate::pyhf::post(
        state,
        def,
        guard,
        "/v1/audio/transcriptions",
        &content_type,
        body,
    )
    .await?;
    let text = serde_json::from_slice::<serde_json::Value>(&answer.body)
        .ok()
        .and_then(|v| {
            v.get("text")
                .and_then(|t| t.as_str())
                .map(|t| t.trim().to_string())
        })
        .ok_or_else(|| {
            GatewayError::new(
                502,
                "upstream_error",
                "Python engine: the answer has no text.",
            )
        })?;
    let (content_type, out) = if format == "text" {
        (
            "text/plain; charset=utf-8",
            format!("{text}\n").into_bytes(),
        )
    } else {
        (
            "application/json",
            serde_json::json!({ "text": text }).to_string().into_bytes(),
        )
    };
    guard.finish(200, None);
    crate::pyhf::lease_headers(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type),
        def,
        &answer.lease,
    )
    .body(axum::body::Body::from(out))
    .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

/// OpenAI's shape of whisper-server's answer: no leading space in `text`, plain text with one
/// trailing newline, subtitles as they come.
fn shape(format: &str, bytes: &[u8]) -> (&'static str, Vec<u8>) {
    match format {
        "json" | "verbose_json" => {
            let mut v: serde_json::Value = match serde_json::from_slice(bytes) {
                Ok(v) => v,
                Err(_) => return ("application/json", bytes.to_vec()),
            };
            if let Some(text) = v.get("text").and_then(|t| t.as_str()).map(str::trim) {
                let text = text.to_string();
                v["text"] = serde_json::Value::String(text);
            }
            if format == "verbose_json" {
                if let Some(segments) = v.get_mut("segments").and_then(|s| s.as_array_mut()) {
                    for s in segments {
                        if let Some(t) = s.get("text").and_then(|t| t.as_str()).map(str::trim) {
                            let t = t.to_string();
                            s["text"] = serde_json::Value::String(t);
                        }
                    }
                }
            }
            (
                "application/json",
                serde_json::to_vec(&v).unwrap_or_else(|_| bytes.to_vec()),
            )
        }
        "text" => (
            "text/plain; charset=utf-8",
            format!("{}\n", String::from_utf8_lossy(bytes).trim()).into_bytes(),
        ),
        "srt" => ("application/x-subrip; charset=utf-8", bytes.to_vec()),
        _ => ("text/vtt; charset=utf-8", bytes.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_shaped_like_openais() {
        let (ct, out) = shape("json", br#"{"text":" Hello World.\n"}"#);
        assert_eq!(ct, "application/json");
        assert_eq!(out, br#"{"text":"Hello World."}"#);
        let (_, out) = shape(
            "verbose_json",
            br#"{"task":"transcribe","language":"english","duration":2.5,"text":" Hi.\n","segments":[{"id":0,"text":" Hi."}]}"#,
        );
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["text"], "Hi.");
        assert_eq!(v["segments"][0]["text"], "Hi.");
        assert_eq!(v["language"], "english");
        let (ct, out) = shape("text", b" Hello World.\n\n");
        assert_eq!(ct, "text/plain; charset=utf-8");
        assert_eq!(out, b"Hello World.\n");
        let (_, out) = shape("srt", b"1\n00:00:00,000 --> 00:00:01,000\nHi\n");
        assert!(out.starts_with(b"1\n"));
        // Not JSON: passed through.
        assert_eq!(shape("json", b"oops").1, b"oops");
    }

    #[test]
    fn multipart_body_has_the_file_and_the_fields() {
        let (ct, body) = multipart_body(
            "a\"b.wav",
            b"RIFFdata",
            &[
                ("response_format", "json".into()),
                ("language", "tr".into()),
            ],
        );
        let boundary = ct.split("boundary=").nth(1).unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{boundary}\r\n")));
        assert!(text.contains("filename=\"a_b.wav\""), "{text}");
        assert!(text.contains("name=\"response_format\"\r\n\r\njson\r\n"));
        assert!(text.contains("name=\"language\"\r\n\r\ntr\r\n"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
        assert!(body.windows(8).any(|w| w == b"RIFFdata"));
    }
}
