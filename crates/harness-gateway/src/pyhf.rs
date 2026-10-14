//! Models of the Python engine (`python-hf`) behind the OpenAI-shaped audio and image routes.
//!
//! `/v1/audio/transcriptions`, `/v1/audio/speech` and `/v1/images/generations` / `edits` are served by
//! the native engines first (whisper.cpp, sherpa-onnx, stable-diffusion.cpp): a request reaches
//! a Python worker only when the definition it names is a Python engine model of the right task
//! (or, for speech, when no sherpa-onnx voice has that name). The worker answers these routes in
//! OpenAI's shape itself; this module leases the server, forwards the request with the run's key
//! and records the request like the other routes do.

use std::time::Duration;

use axum::http::header;
use bytes::Bytes;
use harness_core::domain::ModelDefinition;
use harness_core::gateway::scheduler::Lease;

use crate::errors::GatewayError;
use crate::proxy::RecordGuard;
use crate::GwState;

/// The task of a definition the Python engine serves (`automatic-speech-recognition`,
/// `text-to-speech`, `text-to-image`...), `None` for every other definition.
pub fn task_of(def: &ModelDefinition) -> Option<String> {
    def.base
        .local_path()
        .and_then(harness_core::engine::python_hf::detect)
}

/// What error messages call the worker of `def`: mlx-audio for its speech folders, else the
/// Python engine.
pub fn engine_name(def: &ModelDefinition) -> &'static str {
    if def
        .base
        .local_path()
        .is_some_and(harness_core::engine::mlx::is_audio_dir)
    {
        "mlx-audio"
    } else {
        "Python engine"
    }
}

/// Whether a Python engine task answers `/v1/audio/speech`.
pub fn is_speech_task(task: &str) -> bool {
    matches!(task, "text-to-speech" | "text-to-audio")
}

/// The worker's answer, with the lease it ran under (kept until the response is built, for its
/// headers).
pub struct Answer {
    pub status: u16,
    pub content_type: String,
    pub body: Bytes,
    pub lease: Lease,
    /// The worker stopped at its time limit and answered the audio made by then
    /// (`x-harness-truncated`).
    pub truncated: bool,
}

/// Leases the definition's server (loading it when needed) and posts `body` to `path` on it.
pub async fn post(
    state: &GwState,
    def: &ModelDefinition,
    guard: &mut RecordGuard,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<Answer, GatewayError> {
    let audio_cpp = crate::speech::is_audio_cpp_model(def);
    let engine = if audio_cpp {
        "audio.cpp"
    } else {
        engine_name(def)
    };
    let body = if audio_cpp {
        crate::speech::check_audio_cpp_enabled()?;
        serde_json::to_vec(&crate::speech::audio_cpp_body(def, "POST", path, &body)?)
            .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?
    } else {
        body
    };
    let scheduler = state.scheduler.clone();
    let settings = scheduler.settings();
    let acquired = if audio_cpp {
        scheduler.acquire_speech(def).await
    } else {
        scheduler.acquire_tokens(def, None).await
    };
    let lease = match acquired {
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
    let mut req = state
        .http
        .post(format!("{}{}", lease.upstream, path))
        .header(header::CONTENT_TYPE, content_type)
        .body(body);
    if let Some(key) = &lease.api_key {
        req = req.bearer_auth(key);
    }
    if settings.request_timeout_secs > 0 {
        req = req.timeout(Duration::from_secs(u64::from(
            settings.request_timeout_secs,
        )));
    }
    let upstream = req
        .send()
        .await
        .map_err(|e| transport_error(engine, &e, "upstream_unreachable"))?;
    let status = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let truncated = upstream.headers().contains_key("x-harness-truncated");
    let body = upstream
        .bytes()
        .await
        .map_err(|e| transport_error(engine, &e, "upstream_error"))?;
    guard.get().status = status;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
    if !(200..300).contains(&status) {
        return Err(upstream_error(engine, status, &body));
    }
    Ok(Answer {
        status,
        content_type,
        body,
        lease,
        truncated,
    })
}

fn transport_error(engine: &str, error: &reqwest::Error, code: &str) -> GatewayError {
    if error.is_timeout() {
        let message = if engine == "audio.cpp" {
            "The audio.cpp server did not answer in time.".to_string()
        } else {
            format!("{engine} did not answer in time.")
        };
        GatewayError::new(504, "upstream_timeout", message)
    } else {
        GatewayError::new(502, code, format!("{engine}: {error}"))
    }
}

/// The gateway's error for a failed worker answer: the worker's own OpenAI error message, a 400
/// kept as a 400 (the request was wrong), anything else a 502.
pub fn upstream_error(engine: &str, status: u16, body: &[u8]) -> GatewayError {
    let detail = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            String::from_utf8_lossy(body)
                .trim()
                .chars()
                .take(300)
                .collect()
        });
    let (status, code) = match status {
        400 | 413 => (status, "invalid_request"),
        404 => (400, "wrong_task"),
        503 => (503, "model_loading"),
        // The worker's own time limit (mlx-audio).
        504 => (504, "upstream_timeout"),
        _ => (502, "upstream_error"),
    };
    GatewayError::new(status, code, format!("{engine}: {detail}"))
}

/// The `x-harness-*` headers every answer of a leased model carries.
pub fn lease_headers(
    mut builder: axum::http::response::Builder,
    def: &ModelDefinition,
    lease: &Lease,
) -> axum::http::response::Builder {
    builder = builder
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
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_errors_keep_their_message() {
        let e = upstream_error(
            "Python engine",
            400,
            br#"{"error":{"message":"parameter 'n' must be between 0 and 4","type":"invalid_request_error"}}"#,
        );
        assert_eq!((e.status, e.code.as_str()), (400, "invalid_request"));
        assert!(
            e.message.contains("must be between 0 and 4"),
            "{}",
            e.message
        );
        let e = upstream_error(
            "Python engine",
            404,
            br#"{"error":{"message":"this model serves fill-mask"}}"#,
        );
        assert_eq!((e.status, e.code.as_str()), (400, "wrong_task"));
        assert_eq!(upstream_error("Python engine", 500, b"boom").status, 502);
        assert_eq!(upstream_error("Python engine", 503, b"{}").status, 503);
        let e = upstream_error("mlx-audio", 504, b"{}");
        assert_eq!(e.status, 504);
        assert!(e.message.starts_with("mlx-audio: "), "{}", e.message);
    }

    #[test]
    fn speech_tasks() {
        assert!(is_speech_task("text-to-speech") && is_speech_task("text-to-audio"));
        assert!(!is_speech_task("automatic-speech-recognition"));
    }
}
