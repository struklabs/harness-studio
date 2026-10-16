//! The sole processing route; the upstream task/management surface stays private.
use crate::{
    errors::GatewayError,
    proxy::{header_str, request_id, trace_context, unix_ms, RecordGuard},
    GwState,
};
use axum::{
    extract::{FromRequest, Multipart, Request},
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use harness_core::{
    engine::{audio_cpp, audio_process as core},
    error_key::err,
    gateway::record::{ClientInfo, RequestRecord},
    token_policy::GatewayEndpoint,
};
use serde_json::{json, Map, Value};
use std::{path::Path, time::Duration};

/// `details.error_key` lets the Harness window show the error in its own language; the
/// English message stays for API clients.
fn keyed(e: GatewayError, key: harness_core::error_key::ErrorKey) -> GatewayError {
    e.details(json!({"error_key": key.to_string()}))
}
fn bad(message: impl Into<String>) -> GatewayError {
    let message = message.into();
    let key = core::error_key(&message);
    keyed(GatewayError::new(400, "invalid_request", message), key)
}
fn upstream() -> GatewayError {
    keyed(
        GatewayError::new(
            502,
            "upstream_error",
            "audio.cpp returned an invalid or unsafe audio result.",
        ),
        err("audio_process.invalid_result"),
    )
}
/// audio.cpp's own error for a lost GPU (`vk::Queue::submit: ErrorDeviceLost`): the driver
/// stopped the job, typically an integrated GPU's job timeout.
fn gpu_lost(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&body[..body.len().min(4096)]).to_ascii_lowercase();
    text.contains("devicelost") || text.contains("device lost") || text.contains("device_lost")
}
/// How long one processing request may take: the gateway's request timeout, raised for long
/// audio (CPU separation runs well below real time), at most an hour. `None`: no limit.
fn process_timeout(configured_secs: u32, audio_secs: f64) -> Option<Duration> {
    if configured_secs == 0 {
        return None;
    }
    let scaled = (120.0 + 90.0 * audio_secs.max(0.0)).min(3600.0) as u64;
    Some(Duration::from_secs(u64::from(configured_secs).max(scaled)))
}

pub async fn handle(state: GwState, request: Request) -> Response {
    let headers = request.headers().clone();
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: "/v1/audio/process".into(),
            ..Default::default()
        },
        trace_context(&headers),
    );
    let id = guard.get().id.clone();
    let mut response = match serve(&state, &headers, request, &mut guard).await {
        Ok(value) => value,
        Err(e) => {
            guard.finish(e.status, Some(&e.code));
            e.into_response()
        }
    };
    if let Ok(id) = HeaderValue::from_str(&id) {
        response.headers_mut().insert("x-harness-request-id", id);
    }
    if let Some(rate) = guard.rate {
        crate::auth::rate_headers(response.headers_mut(), &rate, false);
    }
    response
}

async fn read_request(
    state: &GwState,
    request: Request,
) -> Result<(Map<String, Value>, Vec<u8>), GatewayError> {
    let multipart = header_str(request.headers(), "content-type")
        .is_some_and(|s| s.starts_with("multipart/form-data"));
    if !multipart {
        let body = axum::body::to_bytes(request.into_body(), crate::BODY_LIMIT)
            .await
            .map_err(|_| GatewayError::new(413, "invalid_request", "Request body is too large."))?;
        let fields: Map<String, Value> = serde_json::from_slice(&body)
            .map_err(|_| bad("The body must be a JSON object or multipart form."))?;
        for name in fields.keys() {
            if !core::request_field(name) {
                return Err(bad(format!("Unknown field `{name}`.")));
            }
        }
        let data = fields
            .get("audio_base64")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("audio_base64 is required."))?;
        let audio = core::decode_audio(data).map_err(bad)?;
        return Ok((fields, audio));
    }
    let mut mp = Multipart::from_request(request, state)
        .await
        .map_err(|_| bad("Invalid multipart form."))?;
    let mut fields = Map::new();
    let mut audio = None;
    while let Some(mut field) = mp
        .next_field()
        .await
        .map_err(|_| bad("Invalid multipart field."))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if !core::request_field(&name) {
            return Err(bad(format!("Unknown field `{name}`.")));
        }
        if fields.contains_key(&name) || (name == "file" && audio.is_some()) {
            return Err(bad("Duplicate multipart fields are forbidden."));
        }
        let mut bytes = Vec::new();
        let limit = if name == "file" {
            core::MAX_BYTES
        } else {
            4096
        };
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| bad("Invalid multipart field."))?
        {
            if bytes.len() + chunk.len() > limit {
                return Err(GatewayError::new(
                    413,
                    "invalid_request",
                    "Multipart field is too large.",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if name == "file" {
            core::validate_wav(&bytes).map_err(bad)?;
            audio = Some(bytes);
        } else {
            let text = String::from_utf8(bytes).map_err(|_| bad("Form fields must be UTF-8."))?;
            let value = if !core::options("apollo")
                .into_iter()
                .chain(core::options("audiosr"))
                .chain(core::options("sam_audio"))
                .chain(core::options("universr"))
                .any(|spec| spec.name == name && spec.choices.is_empty())
            {
                json!(text)
            } else {
                serde_json::from_str(&text).map_err(|_| bad(format!("Invalid `{name}`.")))?
            };
            fields.insert(name, value);
        }
    }
    if fields.contains_key("audio_base64") {
        return Err(bad("Use only file in multipart requests."));
    }
    Ok((fields, audio.ok_or_else(|| bad("file is required."))?))
}

async fn serve(
    state: &GwState,
    headers: &HeaderMap,
    request: Request,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Audio)?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    crate::speech::check_audio_cpp_enabled()?;
    let (fields, audio) = read_request(state, request).await?;
    let model = fields
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad("model is required."))?;
    let def = state
        .scheduler
        .resolve(Some(model))
        .await
        .map_err(|e| GatewayError::from_sched(&e))?;
    caller.check_model(&def.alias)?;
    GatewayError::reject_remote(&def)?;
    let family = def
        .base
        .local_path()
        .and_then(audio_cpp::metadata)
        .and_then(|m| m.audiocpp_family)
        .ok_or_else(|| bad("Select an audio.cpp processing model."))?;
    let rebuilt = core::rebuild(&def.alias, &family, &fields, &audio).map_err(bad)?;
    guard.admitted(caller.admit(state)?);
    guard.get().model = Some(def.alias.clone());
    guard.get().alias = Some(def.alias.clone());
    guard.get().definition_id = Some(def.id.clone());
    let lease = state
        .scheduler
        .acquire_audio_process(&def)
        .await
        .map_err(|e| {
            let mut error = GatewayError::from_sched(&e);
            // Launch failures may include local model/configuration paths.
            error.message = "The audio.cpp processing worker could not be acquired.".into();
            error.details = None;
            keyed(error, err("audio_process.worker_failed"))
        })?;
    guard.get().server_id = Some(lease.server_id.clone());
    guard.get().queue_wait_ms = lease.queue_wait.as_millis() as u64;
    guard.get().backend = lease.backend.clone();
    guard.get().slots = Some(lease.slots);
    if let Some(load) = &lease.load {
        guard.get().load_ms = Some(load.ms);
        guard.get().evicted = load.evicted.clone();
        guard.get().oom = load.recovered;
    }
    let timeout = process_timeout(
        state.scheduler.settings().request_timeout_secs,
        harness_core::audio::wav_duration_secs(&audio).unwrap_or(0.0),
    );
    let mut request = state
        .http
        .post(format!("{}/v1/tasks/run", lease.upstream))
        .json(&rebuilt);
    if let Some(timeout) = timeout {
        request = request.timeout(timeout);
    }
    let response = request.send().await.map_err(|e| {
        if e.is_timeout() {
            let secs = timeout.map_or(0, |t| t.as_secs());
            keyed(
                GatewayError::new(
                    504,
                    "upstream_timeout",
                    format!("audio.cpp did not finish within {secs} s."),
                ),
                err("audio_process.timeout").with("seconds", secs),
            )
        } else {
            keyed(
                GatewayError::new(
                    502,
                    "upstream_error",
                    "The audio.cpp server did not answer.",
                ),
                err("audio_process.no_answer"),
            )
        }
    })?;
    if !response.status().is_success() {
        let body = response.bytes().await.unwrap_or_default();
        if gpu_lost(&body) {
            return Err(keyed(
                GatewayError::new(
                    502,
                    "upstream_error",
                    "The GPU driver stopped the job; try the CPU backend.",
                ),
                err("audio_process.gpu_lost"),
            ));
        }
        return Err(upstream());
    }
    // Bound the response while streaming, including dishonest/missing Content-Length.
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| upstream())? {
        if bytes.len() + chunk.len() > 256 * 1024 * 1024 {
            return Err(upstream());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| upstream())?;
    let root = state
        .scheduler
        .registry()
        .get(&lease.server_id)
        .and_then(|pm| pm.audio_output_dir());
    let result = normalize(&def.alias, &value, root.as_deref())?;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
    guard.finish(200, None);
    Ok(Json(result).into_response())
}

/// v0.9.1 runtime.cpp task_result_json_with_timing emits audio, named_audio_outputs,
/// or artifacts {id,kind,payload,meta}. Reconstruct only names and WAV bytes.
pub fn normalize(model: &str, value: &Value, root: Option<&Path>) -> Result<Value, GatewayError> {
    struct Cleanup(Vec<std::path::PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let mut outputs = Vec::new();
    let mut files = Cleanup(Vec::new());
    let mut total = 0usize;
    let mut add = |name: &str, bytes: Vec<u8>| -> Result<(), GatewayError> {
        if !["enhanced", "vocals", "instrumental", "target", "residual"].contains(&name)
            || outputs.len() >= 8
        {
            return Err(upstream());
        }
        core::validate_output(&bytes).map_err(|_| upstream())?;
        total += bytes.len();
        if total > 128 * 1024 * 1024 || outputs.iter().any(|v: &Value| v["name"] == name) {
            return Err(upstream());
        }
        outputs.push(json!({"name":name,"audio_base64":STANDARD.encode(bytes),"mime":"audio/wav"}));
        Ok(())
    };
    if let Some(audio) = value.get("audio") {
        add(
            "enhanced",
            core::decode_output(audio.as_str().ok_or_else(upstream)?).map_err(|_| upstream())?,
        )?;
    }
    if let Some(named) = value.get("named_audio_outputs") {
        for output in named.as_array().ok_or_else(upstream)? {
            add(
                output["id"].as_str().ok_or_else(upstream)?,
                core::decode_output(output["audio"].as_str().ok_or_else(upstream)?)
                    .map_err(|_| upstream())?,
            )?;
        }
    }
    if let Some(artifacts) = value.get("artifacts") {
        for artifact in artifacts.as_array().ok_or_else(upstream)? {
            let bytes = if let Some(path) = artifact
                .get("path")
                .or_else(|| artifact.pointer("/meta/path"))
            {
                let path = core::contained_artifact(
                    root.ok_or_else(upstream)?,
                    Path::new(path.as_str().ok_or_else(upstream)?),
                )
                .map_err(|_| upstream())?;
                if std::fs::metadata(&path).map_err(|_| upstream())?.len()
                    > core::MAX_OUTPUT_BYTES as u64
                {
                    return Err(upstream());
                }
                let bytes = std::fs::read(&path).map_err(|_| upstream())?;
                files.0.push(path);
                bytes
            } else {
                core::decode_output(artifact["payload"].as_str().ok_or_else(upstream)?)
                    .map_err(|_| upstream())?
            };
            add(artifact["id"].as_str().ok_or_else(upstream)?, bytes)?;
        }
    }
    if outputs.is_empty() {
        return Err(upstream());
    }
    for path in files.0.drain(..) {
        std::fs::remove_file(path).map_err(|_| upstream())?;
    }
    Ok(json!({"model": model,"outputs": outputs}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn audio() -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../harness-core/tests/data/audio/hello.wav"),
        )
        .unwrap()
    }
    #[test]
    fn inline_audio_and_artifact_payloads_are_rebuilt_without_metadata() {
        let encoded = STANDARD.encode(audio());
        for value in [
            json!({"audio":encoded,"path":"secret"}),
            json!({"artifacts":[{"id":"enhanced","kind":"audio","payload":encoded,"meta":{"secret":"private"}}]}),
        ] {
            let result = normalize("resolved", &value, None).unwrap();
            assert_eq!(
                result,
                json!({"model":"resolved","outputs":[{"name":"enhanced","audio_base64":encoded,"mime":"audio/wav"}]})
            );
        }
        for value in [
            json!({"audio":"bad"}),
            json!({}),
            json!({"artifacts":[{"id":"enhanced","path":"secret.wav"}]}),
            json!({"named_audio_outputs":[{"id":"../secret","audio":encoded}]}),
            json!({"artifacts":{"path":"secret"}}),
        ] {
            let error = normalize("m", &value, None).unwrap_err();
            assert_eq!(error.status, 502);
            assert!(!error.message.contains("secret"));
        }
    }
    #[test]
    fn only_owned_file_artifacts_are_read_inlined_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("outputs");
        std::fs::create_dir(&output).unwrap();
        let path = output.join("enhanced.wav");
        std::fs::write(&path, audio()).unwrap();
        let value = json!({"artifacts":[{"id":"enhanced","path":path}]});
        let result = normalize("m", &value, Some(&output)).unwrap();
        assert!(!path.exists());
        assert_eq!(
            result["outputs"][0]["audio_base64"],
            STANDARD.encode(audio())
        );
        std::fs::write(dir.path().join("secret.wav"), audio()).unwrap();
        assert!(normalize(
            "m",
            &json!({"artifacts":[{"id":"enhanced","path":"../secret.wav"}]}),
            Some(&output)
        )
        .is_err());
        assert!(dir.path().join("secret.wav").exists());
        std::fs::write(&path, audio()).unwrap();
        assert!(normalize("m", &json!({"artifacts":[{"id":"enhanced","path":path},{"id":"target","path":"../secret.wav"}]}), Some(&output)).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn errors_carry_keys_gpu_loss_and_long_audio_timeout() {
        let key = |e: &GatewayError| e.body()["error"]["harness"]["error_key"].clone();
        assert_eq!(key(&upstream()), json!("audio_process.invalid_result"));
        assert_eq!(
            key(&bad("This model requires a 44.1 kHz WAV.")),
            json!("audio_process.needs_44k")
        );
        assert!(gpu_lost(
            br#"{"error":{"message":"vk::Queue::submit: ErrorDeviceLost","type":"server_error"}}"#
        ));
        assert!(!gpu_lost(br#"{"error":{"message":"out of memory"}}"#));
        assert_eq!(process_timeout(0, 30.0), None);
        assert_eq!(process_timeout(600, 2.0), Some(Duration::from_secs(600)));
        assert_eq!(process_timeout(600, 11.0), Some(Duration::from_secs(1110)));
        assert_eq!(process_timeout(600, 600.0), Some(Duration::from_secs(3600)));
        assert_eq!(
            process_timeout(7200, 600.0),
            Some(Duration::from_secs(7200))
        );
    }
}
