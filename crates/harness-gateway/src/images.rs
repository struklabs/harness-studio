//! Image generation: `POST /v1/images/generations` (OpenAI's images API), served by
//! `sd-server` of stable-diffusion.cpp.
//!
//! The request names an image model (`model`, a definition whose file is a diffusion model);
//! the scheduler loads it on demand and unloads it after its TTL like any other model and its
//! memory counts in the same VRAM budget. `sd-server` has an OpenAI-looking route of its own,
//! but it takes the sampler settings in a side channel (`<sd_cpp_extra_args>` inside the
//! prompt); the native job API (`/sdcpp/v1/img_gen`, polled at `/sdcpp/v1/jobs/{id}`) takes
//! them as plain fields and can be cancelled, so this module
//!
//! * reads OpenAI's fields (`prompt`, `n`, `size`, `response_format`, `output_format`) and
//!   Harness's extras (`negative_prompt`, `steps`, `cfg_scale`, `seed`, `sampler`, `scheduler`),
//! * starts a job, polls it until it is done and cancels it when the client goes away, and
//! * answers `{"created": ..., "data": [{"b64_json": ...}]}` (`response_format: "url"` gives a
//!   `data:` URL: Harness keeps no files to link to).
//!
//! A request without `size` leaves the size out, so the model definition's `width` / `height`
//! (the size the model was made for) apply.

use std::time::{Duration, Instant};

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use harness_core::domain::ModelDefinition;
use harness_core::gateway::record::{ClientInfo, RequestRecord};
use harness_core::token_policy::GatewayEndpoint;
use serde_json::{json, Value};

use crate::errors::GatewayError;
use crate::proxy::{header_str, request_id, trace_context, unix_ms, RecordGuard};
use crate::GwState;

/// Most images one request may ask for (`n`).
pub const MAX_IMAGES: u64 = 8;
/// Smallest and largest side `size` may have.
const SIDE_RANGE: std::ops::RangeInclusive<u32> = 64..=4096;

fn bad(message: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

/// Whether a definition serves image generation (its file is a diffusion model).
pub fn is_image_model(def: &ModelDefinition) -> bool {
    use harness_core::domain::ModelRef;
    match &def.base {
        ModelRef::Local { path } => harness_core::engine::is_image_path(path),
        ModelRef::Hf { file, .. } => {
            let f = file.to_ascii_lowercase();
            f.ends_with(".safetensors") || f.ends_with(".ckpt")
        }
    }
}

/// A request, validated and mapped onto `sd-server`'s native `img_gen` body.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The `POST /sdcpp/v1/img_gen` body.
    pub body: Value,
    /// `png`, `jpeg` or `webp`.
    pub format: String,
    /// `response_format`: the image inline (`b64_json`) or as a `data:` URL (`url`).
    pub as_url: bool,
}

/// `"1024x768"` -> `(1024, 768)`.
pub fn parse_size(size: &str) -> Result<(u32, u32), GatewayError> {
    let err = || {
        bad(format!(
            "size `{size}` is not valid; use WIDTHxHEIGHT with both sides between {} and {} and a multiple of 8, like 512x512.",
            SIDE_RANGE.start(),
            SIDE_RANGE.end()
        ))
        .param("size")
    };
    let (w, h) = size.split_once(['x', 'X']).ok_or_else(err)?;
    let w: u32 = w.trim().parse().map_err(|_| err())?;
    let h: u32 = h.trim().parse().map_err(|_| err())?;
    if !SIDE_RANGE.contains(&w)
        || !SIDE_RANGE.contains(&h)
        || !w.is_multiple_of(8)
        || !h.is_multiple_of(8)
    {
        return Err(err());
    }
    Ok((w, h))
}

fn number(v: &Value, key: &str) -> Result<Option<f64>, GatewayError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(x) => x
            .as_f64()
            .map(Some)
            .ok_or_else(|| bad(format!("{key} must be a number.")).param(key)),
    }
}

fn text<'a>(v: &'a Value, key: &str) -> Result<Option<&'a str>, GatewayError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim()).filter(|s| !s.is_empty())),
        Some(_) => Err(bad(format!("{key} must be a string.")).param(key)),
    }
}

/// Validates a request body and maps it onto the native `img_gen` request.
pub fn plan(request: &Value) -> Result<Plan, GatewayError> {
    let prompt = text(request, "prompt")?
        .ok_or_else(|| bad("The `prompt` field (what to draw) is required.").param("prompt"))?;
    let n = match request.get("n") {
        None | Some(Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=MAX_IMAGES).contains(n))
            .ok_or_else(|| {
                bad(format!("n must be a whole number from 1 to {MAX_IMAGES}.")).param("n")
            })?,
    };
    let as_url = match text(request, "response_format")? {
        None | Some("b64_json") => false,
        Some("url") => true,
        Some(other) => {
            return Err(bad(format!(
                "response_format `{other}` is not supported; use b64_json or url (a data: URL)."
            ))
            .param("response_format"))
        }
    };
    let format = match text(request, "output_format")? {
        None => "png".to_string(),
        Some(f) if ["png", "jpeg", "webp"].contains(&f) => f.to_string(),
        Some("jpg") => "jpeg".to_string(),
        Some(other) => {
            return Err(bad(format!(
                "output_format `{other}` is not supported; use png, jpeg or webp."
            ))
            .param("output_format"))
        }
    };
    let mut body = json!({
        "prompt": prompt,
        "batch_count": n,
        "output_format": format,
    });
    let obj = body.as_object_mut().expect("a JSON object");
    if let Some(size) = text(request, "size")? {
        // OpenAI's "auto": the model's own size.
        if size != "auto" {
            let (w, h) = parse_size(size)?;
            obj.insert("width".into(), json!(w));
            obj.insert("height".into(), json!(h));
        }
    }
    if let Some(neg) = text(request, "negative_prompt")? {
        obj.insert("negative_prompt".into(), json!(neg));
    }
    if let Some(seed) = request.get("seed").filter(|s| !s.is_null()) {
        let seed = seed
            .as_i64()
            .ok_or_else(|| bad("seed must be a whole number (-1 = random).").param("seed"))?;
        obj.insert("seed".into(), json!(seed));
    }
    let mut sample = serde_json::Map::new();
    if let Some(steps) = number(request, "steps")? {
        if !(1.0..=500.0).contains(&steps) || steps.fract() != 0.0 {
            return Err(bad("steps must be a whole number from 1 to 500.").param("steps"));
        }
        sample.insert("sample_steps".into(), json!(steps as u32));
    }
    if let Some(cfg) = number(request, "cfg_scale")? {
        if !(0.0..=100.0).contains(&cfg) {
            return Err(bad("cfg_scale must be between 0 and 100.").param("cfg_scale"));
        }
        sample.insert("guidance".into(), json!({ "txt_cfg": cfg }));
    }
    if let Some(guidance) = number(request, "guidance")? {
        // Distilled guidance (FLUX dev), next to the classifier-free one.
        let g = sample.entry("guidance").or_insert_with(|| json!({}));
        g["distilled_guidance"] = json!(guidance);
    }
    if let Some(s) = text(request, "sampler")? {
        sample.insert("sample_method".into(), json!(s));
    }
    if let Some(s) = text(request, "scheduler")? {
        sample.insert("scheduler".into(), json!(s));
    }
    if !sample.is_empty() {
        obj.insert("sample_params".into(), Value::Object(sample));
    }
    Ok(Plan {
        body,
        format,
        as_url,
    })
}

/// The state of a polled job.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    /// `queued` / `generating`: ask again.
    Running,
    /// The encoded images (base64), in order.
    Done(Vec<String>),
    /// `failed` / `cancelled`, with sd-server's message.
    Failed(String),
}

/// Reads a `GET /sdcpp/v1/jobs/{id}` answer.
pub fn parse_job(v: &Value) -> Job {
    match v.get("status").and_then(Value::as_str).unwrap_or("") {
        "completed" => {
            let images: Vec<String> = v
                .pointer("/result/images")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|i| i.get("b64_json").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if images.is_empty() {
                Job::Failed("sd-server finished without an image.".into())
            } else {
                Job::Done(images)
            }
        }
        "failed" | "cancelled" => Job::Failed(
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("the generation failed")
                .to_string(),
        ),
        _ => Job::Running,
    }
}

/// Device memory (MB) a failed job needed and had, from sd-server's log lines of that job:
/// `model manager cannot make enough memory available on CUDA0: need 5788.60 MB device / ...,
/// available 3267.70 MB device / ...`. sd-server itself only answers "generate_image returned
/// no results" (measured: SD-Turbo at 1024x1024 on a 4 GB RTX 3050 Ti).
pub fn memory_shortfall(lines: &[String]) -> Option<(u64, u64)> {
    let mb_before = |s: &str, unit: &str| -> Option<u64> {
        let end = s.find(unit)?;
        let num = s[..end].trim_end().rsplit(' ').next()?;
        num.parse::<f64>().ok().map(|v| v.round() as u64)
    };
    lines.iter().rev().find_map(|l| {
        let rest = &l[l.find("cannot make enough memory available")?..];
        let need = mb_before(&rest[rest.find("need ")?..], " MB device")?;
        let avail_at = rest.find(", available ")?;
        let available = mb_before(&rest[avail_at..], " MB device")?;
        Some((need, available))
    })
}

/// The OpenAI answer for the finished images.
pub fn shape(images: &[String], format: &str, as_url: bool, created: u64) -> Value {
    let mime = if format == "jpeg" {
        "image/jpeg".to_string()
    } else {
        format!("image/{format}")
    };
    let data: Vec<Value> = images
        .iter()
        .map(|b64| {
            if as_url {
                json!({ "url": format!("data:{mime};base64,{b64}") })
            } else {
                json!({ "b64_json": b64 })
            }
        })
        .collect();
    json!({ "created": created, "data": data, "output_format": format })
}

/// Cancels the sd-server job when the request is dropped before it finished (the client went
/// away): a generation takes minutes on a CPU and nobody wants the result.
struct CancelOnDrop {
    http: reqwest::Client,
    url: String,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            let (http, url) = (self.http.clone(), self.url.clone());
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.spawn(async move {
                    let _ = http.post(url).timeout(Duration::from_secs(5)).send().await;
                });
            }
        }
    }
}

/// Handles one image generation request.
pub async fn handle(state: GwState, headers: HeaderMap, body: Bytes) -> Response {
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: "/v1/images/generations".to_string(),
            ..Default::default()
        },
        trace_context(&headers),
    );
    let id = guard.get().id.clone();
    let result = run(&state, &headers, body, &mut guard).await;
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
    headers: &HeaderMap,
    body: Bytes,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Images)?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    let request: Value = serde_json::from_slice(&body)
        .map_err(|e| bad(format!("The request body is not valid JSON: {e}")))?;
    let model = text(&request, "model")?.map(str::to_string);
    guard.get().model = model.clone();
    guard.admitted(caller.admit(state)?);
    let plan = plan(&request)?;

    let scheduler = state.scheduler.clone();
    let settings = scheduler.settings();
    let def = scheduler
        .resolve(model.as_deref())
        .await
        .map_err(|e| caller.sched_error(&e))?;
    {
        let r = guard.get();
        r.alias = Some(def.alias.clone());
        r.definition_id = Some(def.id.clone());
    }
    caller.check_model(&def.alias)?;
    if !is_image_model(&def) {
        return Err(GatewayError::new(
            400,
            "model_not_image",
            format!(
                "`{}` is not an image model (a diffusion model: safetensors, ckpt or a stable-diffusion.cpp GGUF); /v1/images/generations needs one.",
                def.alias
            ),
        )
        .param("model"));
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

    let upstream_err = |e: reqwest::Error| {
        if e.is_timeout() {
            GatewayError::new(
                504,
                "upstream_timeout",
                "sd-server did not answer in time.".to_string(),
            )
        } else {
            GatewayError::new(502, "upstream_unreachable", format!("sd-server: {e}"))
        }
    };
    // Where this job's log lines start: a failure is explained from those alone.
    let manager = state.scheduler.registry().get(&lease.server_id);
    let log_mark = manager.as_ref().map(|m| m.get_logs_since(u64::MAX).0);
    let started = state
        .http
        .post(format!("{}/sdcpp/v1/img_gen", lease.upstream))
        .json(&plan.body)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(upstream_err)?;
    let status = started.status().as_u16();
    let accepted: Value = started.json().await.unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = accepted
            .pointer("/error/message")
            .or_else(|| accepted.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("the request was refused");
        guard.get().status = status;
        return Err(GatewayError::new(
            if status == 400 { 400 } else { 502 },
            if status == 400 {
                "invalid_request"
            } else {
                "upstream_error"
            },
            format!("sd-server: {detail}"),
        ));
    }
    let job_id = accepted
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::new(502, "upstream_error", "sd-server gave no job id."))?
        .to_string();
    let mut cancel = CancelOnDrop {
        http: state.http.clone(),
        url: format!("{}/sdcpp/v1/jobs/{job_id}/cancel", lease.upstream),
        armed: true,
    };

    let limit = match settings.request_timeout_secs {
        0 => None,
        n => Some(Duration::from_secs(u64::from(n))),
    };
    let began = Instant::now();
    let mut wait = Duration::from_millis(150);
    let images = loop {
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(1));
        if limit.is_some_and(|l| began.elapsed() > l) {
            return Err(GatewayError::new(
                504,
                "upstream_timeout",
                "sd-server did not finish the image in time (raise the request timeout in the gateway settings).",
            ));
        }
        let polled = state
            .http
            .get(format!("{}/sdcpp/v1/jobs/{job_id}", lease.upstream))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(upstream_err)?;
        if polled.status().as_u16() == 404 || polled.status().as_u16() == 410 {
            return Err(GatewayError::new(
                502,
                "upstream_error",
                "sd-server lost the job.",
            ));
        }
        let value: Value = polled
            .json()
            .await
            .map_err(|e| GatewayError::new(502, "upstream_error", format!("sd-server: {e}")))?;
        match parse_job(&value) {
            Job::Running => {}
            Job::Done(images) => break images,
            Job::Failed(message) => {
                cancel.armed = false;
                let lines = match (&manager, log_mark) {
                    (Some(m), Some(mark)) => m.get_logs_since(mark).1,
                    _ => Vec::new(),
                };
                return Err(match memory_shortfall(&lines) {
                    Some((need, available)) => GatewayError::new(
                        500,
                        "image_out_of_memory",
                        format!(
                            "The image does not fit in GPU memory at this size: sd-server needs about {need} MB and {available} MB is free. Try a smaller size (SD 1.x and SD-Turbo models are made for 512x512)."
                        ),
                    )
                    .details(json!({ "need_mb": need, "available_mb": available })),
                    None => GatewayError::new(
                        500,
                        "generation_failed",
                        format!("sd-server: {message}"),
                    ),
                });
            }
        }
    };
    cancel.armed = false;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
    guard.get().status = 200;

    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = shape(&images, &plan.format, plan.as_url, created);
    guard.finish(200, None);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
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
        .body(axum::body::Body::from(out.to_string()))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_that_ran_out_of_device_memory_says_how_much_it_needed() {
        let lines = vec![
            "[+9.1s] [stdout] [W] model manager memory on CUDA0: reported free 3267.70 MB / total 4095.50 MB --- model_manager.cpp:1753".to_string(),
            "[+9.1s] [stdout] [W] model manager cannot make enough memory available on CUDA0: need 5788.60 MB device / 5276.60 MB budget, available 3267.70 MB device / 2783.70 MB budget --- model_manager.cpp:1962".to_string(),
            "[+9.1s] [stdout] [E] sampling for image 1/1 failed after 0.03s --- image.cpp:907".to_string(),
        ];
        assert_eq!(memory_shortfall(&lines), Some((5789, 3268)));
        assert_eq!(memory_shortfall(&lines[2..]), None);
        assert_eq!(memory_shortfall(&[]), None);
    }

    #[test]
    fn an_openai_request_maps_onto_the_native_job() {
        let p = plan(&json!({
            "model": "sd-turbo", "prompt": "a red fox", "n": 2, "size": "512x768",
            "response_format": "b64_json", "negative_prompt": "blurry", "steps": 4,
            "cfg_scale": 1.0, "seed": 7, "sampler": "euler_a"
        }))
        .unwrap();
        assert_eq!(
            p.body,
            json!({
                "prompt": "a red fox", "batch_count": 2, "output_format": "png",
                "width": 512, "height": 768, "negative_prompt": "blurry", "seed": 7,
                "sample_params": {
                    "sample_steps": 4, "guidance": {"txt_cfg": 1.0}, "sample_method": "euler_a"
                }
            })
        );
        assert!(!p.as_url && p.format == "png");
    }

    #[test]
    fn a_bare_prompt_leaves_the_model_defaults_alone() {
        let p = plan(&json!({"prompt": "a cat"})).unwrap();
        assert_eq!(
            p.body,
            json!({"prompt": "a cat", "batch_count": 1, "output_format": "png"})
        );
        // OpenAI's "auto" is the same thing.
        assert!(plan(&json!({"prompt": "a cat", "size": "auto"}))
            .unwrap()
            .body
            .get("width")
            .is_none());
        // Distilled guidance sits next to the CFG scale.
        let p = plan(&json!({"prompt": "x", "cfg_scale": 1.0, "guidance": 3.5})).unwrap();
        assert_eq!(
            p.body["sample_params"]["guidance"],
            json!({"txt_cfg": 1.0, "distilled_guidance": 3.5})
        );
    }

    #[test]
    fn bad_requests_name_the_field() {
        let param = |v: Value| {
            plan(&v)
                .unwrap_err()
                .param
                .map(|p| p.to_string())
                .unwrap_or_default()
        };
        assert_eq!(param(json!({})), "prompt");
        assert_eq!(param(json!({"prompt": "  "})), "prompt");
        assert_eq!(param(json!({"prompt": "x", "n": 0})), "n");
        assert_eq!(param(json!({"prompt": "x", "n": 99})), "n");
        assert_eq!(param(json!({"prompt": "x", "size": "512"})), "size");
        assert_eq!(param(json!({"prompt": "x", "size": "500x500"})), "size");
        assert_eq!(param(json!({"prompt": "x", "size": "16x16"})), "size");
        assert_eq!(param(json!({"prompt": "x", "steps": 0})), "steps");
        assert_eq!(param(json!({"prompt": "x", "steps": 2.5})), "steps");
        assert_eq!(param(json!({"prompt": "x", "seed": "a"})), "seed");
        assert_eq!(param(json!({"prompt": "x", "cfg_scale": "a"})), "cfg_scale");
        assert_eq!(
            param(json!({"prompt": "x", "response_format": "html"})),
            "response_format"
        );
        assert_eq!(
            param(json!({"prompt": "x", "output_format": "gif"})),
            "output_format"
        );
        assert_eq!(parse_size("1024x1024").unwrap(), (1024, 1024));
        assert_eq!(parse_size("640X480").unwrap(), (640, 480));
    }

    #[test]
    fn jobs_are_read_by_status() {
        assert_eq!(parse_job(&json!({"status": "queued"})), Job::Running);
        assert_eq!(parse_job(&json!({"status": "generating"})), Job::Running);
        let done = json!({"status": "completed", "result": {"images": [
            {"index": 0, "b64_json": "AAA"}, {"index": 1, "b64_json": "BBB"}]}});
        assert_eq!(
            parse_job(&done),
            Job::Done(vec!["AAA".into(), "BBB".into()])
        );
        let empty = json!({"status": "completed", "result": {"images": []}});
        assert!(matches!(parse_job(&empty), Job::Failed(_)));
        let failed = json!({"status": "failed", "error": {"code": "generation_failed", "message": "out of memory"}});
        assert_eq!(parse_job(&failed), Job::Failed("out of memory".into()));
    }

    #[test]
    fn the_answer_has_openai_shape() {
        let images = vec!["AAA".to_string()];
        let v = shape(&images, "png", false, 5);
        assert_eq!(v["created"], 5);
        assert_eq!(v["data"][0]["b64_json"], "AAA");
        let v = shape(&images, "jpeg", true, 5);
        assert_eq!(v["data"][0]["url"], "data:image/jpeg;base64,AAA");
    }
}
