//! `/v1/audio/transcriptions` (and `/translations`) on a llama.cpp chat model that hears: a
//! definition whose projector has an audio encoder (LFM2.5-Audio, Qwen3-ASR, Voxtral...).
//!
//! The audio goes to the model's llama-server as `input_audio` parts of chat completions, one
//! per window of at most [`harness_core::asr::llm::WINDOW_SECS`] (cut at a quiet moment), each
//! with the instruction of its projector family and a token limit for its length; the answers
//! are cleaned and joined into OpenAI's transcription answer. The windows are the segments of
//! `verbose_json`, `srt` and `vtt`.

use std::time::{Duration, Instant};

use axum::http::{header, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use bytes::Bytes;
use harness_core::asr::llm;
use harness_core::asr::{Segment, Transcript};
use harness_core::audio::{self, Container};
use harness_core::domain::ModelDefinition;

use crate::audio::Task;
use crate::errors::GatewayError;
use crate::proxy::RecordGuard;
use crate::GwState;

/// The request's fields this path uses.
pub struct Request<'a> {
    pub task: Task,
    pub def: &'a ModelDefinition,
    pub projector: &'a str,
    pub audio: Bytes,
    pub language: Option<&'a str>,
    pub prompt: Option<&'a str>,
    pub temperature: Option<f64>,
    pub format: &'a str,
}

/// The audio as the windows llama-server gets.
struct Prepared {
    /// `(wav or mp3 bytes, start secs, end secs)`.
    pieces: Vec<(Vec<u8>, f64, f64)>,
    format: &'static str,
    duration: Option<f64>,
}

/// Mono 16-bit PCM when the audio can be read as such (directly or through ffmpeg); otherwise
/// the original bytes go in one piece in a container llama.cpp decodes itself (WAV, MP3).
async fn prepare(bytes: Bytes) -> Result<Prepared, GatewayError> {
    let container = audio::sniff(&bytes);
    let mut pcm = (container == Container::Wav)
        .then(|| llm::pcm16(&bytes))
        .flatten();
    if pcm.is_none() && audio::ffmpeg_path().is_some() {
        let wav = audio::to_wav(bytes.to_vec()).await.map_err(|e| {
            GatewayError::new(
                400,
                "audio_convert_failed",
                format!("The audio could not be decoded ({container:?}): {e}"),
            )
            .param("file")
        })?;
        pcm = llm::pcm16(&wav);
    }
    if let Some((samples, rate)) = pcm {
        let secs = |n: usize| n as f64 / f64::from(rate);
        let pieces = llm::windows(&samples, rate)
            .into_iter()
            .map(|(a, b)| (llm::wav_of(&samples[a..b], rate), secs(a), secs(b)))
            .collect();
        return Ok(Prepared {
            pieces,
            format: "wav",
            duration: Some(secs(samples.len())),
        });
    }
    let format = match container {
        Container::Wav => "wav",
        Container::Mp3 => "mp3",
        _ => {
            return Err(GatewayError::new(
                415,
                "unsupported_audio_format",
                format!(
                    "{} audio needs ffmpeg to be converted and none was found; install ffmpeg, or send WAV or MP3.",
                    container.name()
                ),
            )
            .param("file"))
        }
    };
    let duration = audio::wav_duration_secs(&bytes);
    Ok(Prepared {
        pieces: vec![(bytes.to_vec(), 0.0, duration.unwrap_or(0.0))],
        format,
        duration,
    })
}

/// Serves one request; `guard` gets the lease's facts like the other speech paths.
pub async fn transcribe(
    state: &GwState,
    req: Request<'_>,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    if req.task == Task::Translate && !llm::translates(req.projector) {
        return Err(GatewayError::new(
            400,
            "invalid_request",
            format!(
                "`{}` only transcribes; use /v1/audio/transcriptions.",
                req.def.alias
            ),
        )
        .param("model"));
    }
    let prepared = prepare(req.audio).await?;
    if prepared.duration.is_none() && matches!(req.format, "srt" | "vtt") {
        return Err(GatewayError::new(
            400,
            "invalid_request",
            "Subtitles need the audio's timing: send WAV, or install ffmpeg to convert it.",
        )
        .param("response_format"));
    }

    let scheduler = state.scheduler.clone();
    let lease = match scheduler.acquire_tokens(req.def, None).await {
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

    let instruction = llm::instruction(
        req.projector,
        req.task == Task::Translate,
        req.language,
        req.prompt,
    );
    let timeout = match scheduler.settings().request_timeout_secs {
        0 => None,
        n => Some(Duration::from_secs(u64::from(n))),
    };
    let started = Instant::now();
    let mut segments = Vec::new();
    let mut language = None;
    let mut truncated = false;
    for (piece, start, end) in &prepared.pieces {
        if end - start < 0.05 && prepared.duration.is_some() {
            continue; // nothing to hear
        }
        let data = base64::engine::general_purpose::STANDARD.encode(piece);
        let body = serde_json::json!({
            "messages": llm::messages(&instruction, &data, prepared.format),
            "temperature": req.temperature.unwrap_or(0.0),
            "max_tokens": llm::max_tokens(end - start),
            "stream": false,
        });
        let mut call = state
            .http
            .post(format!("{}/v1/chat/completions", lease.upstream))
            .json(&body);
        if let Some(key) = &lease.api_key {
            call = call.bearer_auth(key);
        }
        if let Some(t) = timeout {
            call = call.timeout(t);
        }
        let upstream = call.send().await.map_err(|e| {
            if e.is_timeout() {
                GatewayError::new(
                    504,
                    "upstream_timeout",
                    "llama-server did not answer in time.",
                )
            } else {
                GatewayError::new(502, "upstream_unreachable", format!("llama-server: {e}"))
            }
        })?;
        let status = upstream.status().as_u16();
        let answer: serde_json::Value = upstream
            .json()
            .await
            .map_err(|e| GatewayError::new(502, "upstream_error", e.to_string()))?;
        if status != 200 {
            let detail = answer
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("no detail")
                .to_string();
            guard.get().status = status;
            return Err(GatewayError::new(
                if status == 400 { 400 } else { 502 },
                if status == 400 {
                    "invalid_audio"
                } else {
                    "upstream_error"
                },
                format!("llama-server: {detail}"),
            ));
        }
        let choice = &answer["choices"][0];
        truncated |= choice["finish_reason"] == "length";
        let content = choice["message"]["content"].as_str().unwrap_or_default();
        let (text, lang) = llm::clean(content);
        language = language.or(lang);
        if !text.is_empty() {
            segments.push(Segment {
                start: *start,
                end: *end,
                text,
                speaker: None,
            });
        }
        if guard.get().ttft_ms.is_none() {
            guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
        }
    }
    let transcript = Transcript::new(
        segments,
        prepared.duration.unwrap_or(0.0),
        language.or_else(|| req.language.map(str::to_string)),
        req.def.alias.clone(),
        started.elapsed().as_millis() as u64,
    );
    let task_name = if req.task == Task::Translate {
        "translate"
    } else {
        "transcribe"
    };
    let (content_type, out) = match req.format {
        "text" => (
            "text/plain; charset=utf-8",
            format!("{}\n", transcript.text).into_bytes(),
        ),
        "srt" => (
            "application/x-subrip; charset=utf-8",
            transcript.to_srt().into_bytes(),
        ),
        "vtt" => ("text/vtt; charset=utf-8", transcript.to_vtt().into_bytes()),
        "verbose_json" => (
            "application/json",
            transcript
                .to_verbose_json(task_name)
                .to_string()
                .into_bytes(),
        ),
        _ => (
            "application/json",
            serde_json::json!({ "text": transcript.text })
                .to_string()
                .into_bytes(),
        ),
    };
    guard.finish(200, None);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header("x-harness-model", &req.def.alias)
        .header("x-harness-server", &lease.server_id)
        .header(
            "x-harness-queue-ms",
            lease.queue_wait.as_millis().to_string(),
        );
    if truncated {
        // A window reached its token limit: the model was repeating itself (or the speech was
        // denser than the limit allows); its text was kept up to the limit.
        builder = builder.header("x-harness-truncated", "tokens");
    }
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
