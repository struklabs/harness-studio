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

/// What error messages call the worker of `def`: mflux for its image folders, transcribe.cpp for
/// its speech recognition GGUFs, mlx-audio for its speech folders, else the Python engine.
pub fn engine_name(def: &ModelDefinition) -> &'static str {
    if crate::images::is_mflux_model(def) {
        return "mflux";
    }
    if crate::transcribe_cpp::is_model(def) {
        return "transcribe.cpp";
    }
    if def
        .base
        .local_path()
        .is_some_and(harness_core::engine::python_hf::is_liquid_audio_dir)
    {
        return "Liquid Audio";
    }
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
    /// The worker answered part of the audio (`x-harness-truncated`): `time` (its time limit
    /// stopped it) or `frames` (a call used its whole frame budget).
    pub truncated: Option<String>,
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
    GatewayError::reject_remote(def)?;
    let audio_cpp = crate::speech::is_audio_cpp_model(def);
    let crisp_tts = crate::speech::is_crispasr_tts(def);
    let engine = if crisp_tts {
        "CrispASR"
    } else if audio_cpp {
        "audio.cpp"
    } else {
        engine_name(def)
    };
    let body = if crisp_tts {
        serde_json::to_vec(&crate::crispasr_tts::body(def, "POST", path, &body)?)
            .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?
    } else if audio_cpp {
        crate::speech::check_audio_cpp_enabled()?;
        serde_json::to_vec(&crate::speech::audio_cpp_body(def, "POST", path, &body)?)
            .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?
    } else {
        body
    };
    let scheduler = state.scheduler.clone();
    let settings = scheduler.settings();
    let acquired = if audio_cpp || crisp_tts {
        scheduler.acquire_speech(def).await
    } else {
        scheduler.acquire_tokens(def, None).await
    };
    let lease = match acquired {
        Ok(lease) => lease,
        Err(e) => {
            if (audio_cpp || crisp_tts)
                && matches!(
                    e,
                    harness_core::gateway::scheduler::SchedError::LoadFailed { .. }
                )
            {
                let _ = scheduler.unload(&def.id).await;
            }
            guard.get().oom = e.is_oom();
            if audio_cpp
                && matches!(
                    e,
                    harness_core::gateway::scheduler::SchedError::LoadFailed { .. }
                )
            {
                let message = if e.message().to_ascii_lowercase().contains("espeak") {
                    "tts.espeak_missing"
                } else {
                    "tts.worker_failed"
                };
                let detail = e.message();
                let mut logs = Vec::new();
                for (_, pm) in scheduler.registry().all() {
                    if pm
                        .launch_config()
                        .is_some_and(|c| c.definition_id.as_deref() == Some(def.id.as_str()))
                    {
                        let (_, lines) = pm.get_run_logs_since(0);
                        logs.extend(
                            lines
                                .into_iter()
                                .rev()
                                .take(20)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev(),
                        );
                        pm.append_log(format!("[audio.cpp error] {detail}"));
                    }
                }
                return Err(GatewayError::new(502, "upstream_error", message).details(
                    serde_json::json!({"engine":"audio.cpp", "detail":detail, "worker_log":logs}),
                ));
            }
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
    if crisp_tts {
        let configured = u64::from(settings.request_timeout_secs);
        req = req.timeout(Duration::from_secs(if configured == 0 {
            120
        } else {
            configured.min(120)
        }));
    } else if settings.request_timeout_secs > 0 {
        req = req.timeout(Duration::from_secs(u64::from(
            settings.request_timeout_secs,
        )));
    }
    let upstream = match req.send().await {
        Ok(upstream) => upstream,
        Err(e) => {
            if audio_cpp {
                log_audio_cpp_error(state, &lease, &e.to_string());
            }
            if audio_cpp || crisp_tts {
                let _ = scheduler.unload(&def.id).await;
            }
            return Err(transport_error(engine, &e, "upstream_unreachable"));
        }
    };
    let status = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let truncated = upstream
        .headers()
        .get("x-harness-truncated")
        .map(|v| match v.to_str() {
            Ok("frames") => "frames".to_string(),
            _ => "time".to_string(),
        });
    let body = if crisp_tts {
        // Bounded buffering even when an upstream sends no Content-Length.
        let mut upstream = upstream;
        let mut bytes = Vec::new();
        while let Some(chunk) = match upstream.chunk().await {
            Ok(chunk) => chunk,
            Err(e) => {
                if e.is_timeout() {
                    let _ = scheduler.unload(&def.id).await;
                }
                return Err(transport_error(engine, &e, "upstream_error"));
            }
        } {
            if bytes.len() + chunk.len() > 24 * 1024 * 1024 {
                let _ = scheduler.unload(&def.id).await;
                return Err(GatewayError::new(
                    502,
                    "speech_limit",
                    "Speech output exceeds 24 MiB",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if (200..300).contains(&status)
            && !harness_core::audio::wav_duration_secs(&bytes)
                .is_some_and(|seconds| seconds > 0.0 && seconds <= 120.0)
        {
            return Err(GatewayError::new(
                502,
                "speech_limit",
                "Speech output must be WAV and at most 120 seconds",
            ));
        }
        Bytes::from(bytes)
    } else {
        match upstream.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                if audio_cpp {
                    log_audio_cpp_error(state, &lease, &e.to_string());
                    let _ = scheduler.unload(&def.id).await;
                }
                return Err(transport_error(engine, &e, "upstream_error"));
            }
        }
    };
    guard.get().status = status;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
    if !(200..300).contains(&status) {
        let error = upstream_error(engine, status, &body);
        if audio_cpp {
            log_audio_cpp_error(state, &lease, &error.body().to_string());
        }
        if audio_cpp && (status >= 500 || error.code == "espeak_missing") {
            let _ = scheduler.unload(&def.id).await;
        }
        return Err(error);
    }
    Ok(Answer {
        status,
        content_type,
        body,
        lease,
        truncated,
    })
}

fn log_audio_cpp_error(state: &GwState, lease: &Lease, detail: &str) {
    if let Some(pm) = state.scheduler.registry().get(&lease.server_id) {
        pm.append_log(format!("[audio.cpp error] {detail}"));
    }
}

fn transport_error(engine: &str, error: &reqwest::Error, code: &str) -> GatewayError {
    if engine == "audio.cpp" {
        return GatewayError::new(
            if error.is_timeout() { 504 } else { 502 },
            code,
            "tts.worker_failed",
        )
        .details(serde_json::json!({"engine":engine, "detail":error.to_string()}));
    }
    if error.is_timeout() {
        let message = format!("{engine} did not answer in time.");
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
    if engine == "audio.cpp" && detail.to_ascii_lowercase().contains("espeak") {
        return GatewayError::new(502, "espeak_missing", "tts.espeak_missing").details(
            serde_json::json!({"engine":engine, "upstream_status":status, "detail":detail}),
        );
    }
    if engine == "audio.cpp" && status >= 500 {
        return GatewayError::new(502, "upstream_error", "tts.worker_failed").details(
            serde_json::json!({"engine":engine, "upstream_status":status, "detail":detail}),
        );
    }
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
    fn audio_cpp_dependency_errors_have_localizable_messages() {
        let e = upstream_error("audio.cpp", 500,
            br#"{"error":{"message":"Could not load eSpeak-ng; install the shared library or provide its path"}}"#);
        assert_eq!(e.status, 502);
        assert_eq!(e.message, "tts.espeak_missing");
        assert!(e.details.as_ref().unwrap()["detail"]
            .as_str()
            .unwrap()
            .contains("Could not load eSpeak"));
        let e = upstream_error("audio.cpp", 500, b"failed to load");
        assert_eq!(e.message, "tts.worker_failed");
        assert_eq!(e.body()["error"]["harness"]["detail"], "failed to load");
        assert_eq!(e.body()["error"]["harness"]["upstream_status"], 500);
        assert_eq!(
            upstream_error("audio.cpp", 400, b"invalid voice").status,
            400
        );
    }

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
