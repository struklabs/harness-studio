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
/// audio.cpp's errors when the GPU has no room left for a job's buffers (`SAM Audio decoder graph
/// allocation failed`, `Failed to allocate Sortformer diar backend tensors`, a Vulkan
/// `ErrorOutOfDeviceMemory`): long audio and other resident models both use it up.
fn gpu_memory(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&body[..body.len().min(4096)]).to_ascii_lowercase();
    // A bare "allocation failed" also covers disk, port and validation errors: require a
    // ggml object (graph, tensor, buffer) next to it.
    let ggml = ["graph", "tensor", "buffer"]
        .iter()
        .any(|object| text.contains(object));
    text.contains("outofdevicememory")
        || text.contains("out of memory")
        || (ggml && (text.contains("allocation failed") || text.contains("failed to allocate")))
}
/// Unloads the other audio.cpp models the gateway holds and nobody is using; whether any was.
async fn free_idle_audio_workers(state: &GwState, keep: &str) -> bool {
    let Ok(definitions) = state.scheduler.definitions().await else {
        return false;
    };
    let mut any = false;
    for slot in state.scheduler.snapshot().await {
        let audio = definitions.iter().any(|d| {
            d.id == slot.definition_id && d.base.local_path().is_some_and(audio_cpp::is_model)
        });
        if slot.definition_id != keep
            && slot.managed
            && !slot.pinned
            && audio
            && state
                .scheduler
                .unload_if_idle(&slot.definition_id)
                .await
                .unwrap_or(false)
        {
            any = true;
        }
    }
    any
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
            let value = if core::numeric_option(&name) {
                serde_json::from_str(&text).map_err(|_| bad(format!("Invalid `{name}`.")))?
            } else {
                json!(text)
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
    // With audio.cpp off, only an mlx-audio separator is served; any other request gets the
    // same error as before (also when its body or model is wrong).
    let disabled = crate::speech::check_audio_cpp_enabled().err();
    let (fields, audio) = match read_request(state, request).await {
        Ok(read) => read,
        Err(e) => return Err(disabled.unwrap_or(e)),
    };
    let model = fields
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let def = match model {
        Some(model) => state.scheduler.resolve(Some(model)).await,
        None => {
            return Err(disabled.unwrap_or_else(|| bad("model is required.")));
        }
    };
    let def = match def {
        Ok(def) => def,
        Err(e) => return Err(disabled.unwrap_or_else(|| GatewayError::from_sched(&e))),
    };
    let separator = def
        .base
        .local_path()
        .is_some_and(harness_core::engine::mlx::is_sts_dir)
        && !def.base.local_path().is_some_and(audio_cpp::is_model);
    if !separator {
        if let Some(e) = disabled {
            return Err(e);
        }
    }
    caller.check_model(&def.alias)?;
    GatewayError::reject_remote(&def)?;
    let (rebuilt, diarization) = if separator {
        (mlx_request(&def.alias, &fields, &audio)?, false)
    } else {
        let family = def
            .base
            .local_path()
            .and_then(audio_cpp::metadata)
            .and_then(|m| m.audiocpp_family)
            .ok_or_else(|| bad("Select an audio.cpp processing model."))?;
        let rebuilt = core::rebuild(&def.alias, &family, &fields, &audio).map_err(bad)?;
        (rebuilt, audio_cpp::is_diarization(&family))
    };
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
    let audio_secs = harness_core::audio::wav_duration_secs(&audio).unwrap_or(0.0);
    let timeout = process_timeout(state.scheduler.settings().request_timeout_secs, audio_secs);
    let mut freed = false;
    let response = loop {
        let mut request = state
            .http
            .post(format!("{}/v1/tasks/run", lease.upstream))
            .json(&rebuilt);
        // The mlx-audio worker checks its key; audio.cpp's request stays as it was.
        if separator {
            if let Some(key) = &lease.api_key {
                request = request.bearer_auth(key);
            }
        }
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
        if response.status().is_success() {
            break response;
        }
        let status = response.status().as_u16();
        let body = response.bytes().await.unwrap_or_default();
        if separator {
            return Err(mlx_error(status, &body));
        }
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
        if gpu_memory(&body) {
            // Other audio.cpp models stay resident after their last request and hold the GPU:
            // unload the idle ones and try once more.
            if !freed && free_idle_audio_workers(state, &def.id).await {
                freed = true;
                continue;
            }
            return Err(keyed(
                GatewayError::new(
                    502,
                    "upstream_error",
                    "The GPU ran out of memory for this audio; try a shorter clip or the CPU backend.",
                ),
                err("audio_process.gpu_memory"),
            ));
        }
        return Err(upstream());
    };
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
    if diarization {
        let result =
            core::diarization_result(&def.alias, &value, audio_secs).ok_or_else(upstream)?;
        guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
        guard.finish(200, None);
        return Ok(Json(result).into_response());
    }
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

/// The worker request of an mlx-audio separator (Mel-Band-RoFormer): the WAV alone, in
/// audio.cpp's task shape. It takes no options or prompt.
fn mlx_request(
    alias: &str,
    fields: &Map<String, Value>,
    audio: &[u8],
) -> Result<Value, GatewayError> {
    if let Some(name) = fields
        .keys()
        .find(|k| !matches!(k.as_str(), "model" | "audio_base64"))
    {
        return Err(bad(format!("Unknown field `{name}`.")));
    }
    Ok(json!({"model": alias, "request": {"audio_base64": STANDARD.encode(audio)}}))
}

/// An mlx-audio separator's refusal (400: audio too long or undecodable) or time limit (504),
/// with the worker's own message; anything else is an invalid result.
fn mlx_error(status: u16, body: &[u8]) -> GatewayError {
    let message = serde_json::from_slice::<Value>(body).ok().and_then(|v| {
        v.pointer("/error/message")
            .and_then(Value::as_str)
            .map(String::from)
    });
    match (status, message) {
        (400, Some(message)) => bad(message),
        (504, Some(message)) => GatewayError::new(504, "upstream_timeout", message),
        _ => GatewayError::new(
            502,
            "upstream_error",
            "mlx-audio could not separate this audio.",
        ),
    }
}

/// Names of the audio a worker may return: enhanced speech, the two stems of a vocal separator,
/// the target and the rest of SAM Audio, and the stems of HTDemucs (and its six-stem model).
const OUTPUT_NAMES: &[&str] = &[
    "enhanced",
    "vocals",
    "instrumental",
    "target",
    "residual",
    "drums",
    "bass",
    "other",
    "guitar",
    "piano",
];

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
        if !OUTPUT_NAMES.contains(&name) || outputs.len() >= 8 {
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
    /// An mlx-audio separator gets the WAV alone in audio.cpp's task shape; options, a prompt or
    /// unknown fields are refused, and its refusals keep their message.
    #[test]
    fn mlx_separators_take_the_wav_alone() {
        let wav = audio();
        let mut fields = Map::new();
        fields.insert("model".into(), json!("sep"));
        let request = mlx_request("sep", &fields, &wav).unwrap();
        assert_eq!(
            request,
            json!({"model":"sep","request":{"audio_base64":STANDARD.encode(&wav)}})
        );
        fields.insert("audio_base64".into(), json!("x"));
        assert!(mlx_request("sep", &fields, &wav).is_ok());
        for extra in ["prompt", "seed", "audio_chunk_duration_sec"] {
            let mut with = fields.clone();
            with.insert(extra.into(), json!(1));
            let e = mlx_request("sep", &with, &wav).unwrap_err();
            assert_eq!(e.status, 400, "{extra}");
        }
        let refusal = br#"{"error":{"message":"the audio is longer than 300 seconds."}}"#;
        let e = mlx_error(400, refusal);
        assert_eq!(
            (e.status, e.message.as_str()),
            (400, "the audio is longer than 300 seconds.")
        );
        assert_eq!(mlx_error(504, refusal).status, 504);
        assert_eq!(mlx_error(500, b"oops").status, 502);
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
    fn demucs_stems_are_returned_by_name_and_other_names_are_not() {
        let encoded = STANDARD.encode(audio());
        let stems = ["drums", "bass", "other", "vocals", "guitar", "piano"];
        let value = json!({"named_audio_outputs": stems.iter().map(|s| json!({"id": s, "audio": encoded})).collect::<Vec<_>>()});
        let result = normalize("htdemucs", &value, None).unwrap();
        let names: Vec<_> = result["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, stems);
        for bad in ["Drums", "drums.wav", "bass/../x", "", "synth"] {
            let value = json!({"named_audio_outputs": [{"id": bad, "audio": encoded}]});
            assert_eq!(
                normalize("m", &value, None).unwrap_err().status,
                502,
                "{bad}"
            );
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
        for message in [
            "SAM Audio decoder graph allocation failed",
            "Failed to allocate Sortformer diar backend tensors",
            "vk::Device::allocateMemory: ErrorOutOfDeviceMemory",
            "ggml_gallocr_reserve_n: failed to allocate Vulkan0 buffer of size 2147483648",
            "out of memory",
        ] {
            let body = format!(r#"{{"error":{{"message":"{message}","type":"server_error"}}}}"#);
            assert!(gpu_memory(body.as_bytes()), "{message}");
        }
        assert!(!gpu_memory(
            br#"{"error":{"message":"unknown model id: x"}}"#
        ));
        assert!(!gpu_memory(
            br#"{"error":{"message":"request exceeds fixed graph capacity"}}"#
        ));
        for message in [
            "disk allocation failed: no space left on device",
            "failed to allocate a worker port",
            "request allocation failed validation",
            "failed to allocate output file: permission denied",
        ] {
            assert!(!gpu_memory(message.as_bytes()), "{message}");
        }
        assert_eq!(
            key(&keyed(
                GatewayError::new(502, "upstream_error", "x"),
                err("audio_process.gpu_memory")
            )),
            json!("audio_process.gpu_memory")
        );
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
