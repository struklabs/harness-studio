//! Video generation: OpenAI's videos API (`POST /v1/videos`, `GET /v1/videos`,
//! `GET /v1/videos/{id}`, `GET /v1/videos/{id}/content`, `DELETE /v1/videos/{id}`), served by
//! `sd-server` of stable-diffusion.cpp (Wan and the other video models it runs).
//!
//! A video takes minutes (Wan2.1 1.3B: about 9 minutes for 9 frames at 416x240 on an Apple M1),
//! longer than a client keeps a request open, so a video is a job: `POST` answers at once with
//! the job (`status: "queued"`), the client polls `GET /v1/videos/{id}` and downloads the file
//! from `/content` when it is `completed`. The gateway
//!
//! * runs one video at a time (a queue), each with a scheduler lease held for the whole job, so
//!   the model is loaded, counted in the memory budget, evicts other models like any request
//!   and is not unloaded by its TTL while it works;
//! * maps the request onto sd-server's native `POST /sdcpp/v1/vid_gen` (`video_frames`, `fps`,
//!   `sample_params`, `init_image`) and polls `/sdcpp/v1/jobs/{id}` (its preview step gives the
//!   progress);
//! * keeps each job as `<data>/videos/<id>.json` and the finished file next to it, so a
//!   finished video survives a restart; a job that was queued or running when the gateway
//!   stopped is `failed` after it. Jobs older than [`KEEP_DAYS`] are removed at start;
//! * shows a job only to the token that made it, and `DELETE` cancels a job that has not
//!   finished and removes its file. sd-server cannot stop a generation it already started
//!   (`cancel_generating: false`): that one runs to its end and its result is thrown away.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{FromRequest, Multipart};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use bytes::Bytes;
use harness_core::gateway::record::{ClientInfo, RequestRecord};
use harness_core::token_policy::GatewayEndpoint;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{Notify, Semaphore};

use crate::errors::GatewayError;
use crate::images::{is_image_model, number, parse_size, text};
use crate::proxy::{header_str, request_id, trace_context, unix_ms, RecordGuard};
use crate::GwState;

/// Finished jobs older than this many days are removed when the gateway starts.
pub const KEEP_DAYS: u64 = 7;
/// Frames when the request names neither `frames` nor `seconds` (Wan's own examples: 33).
pub const DEFAULT_FRAMES: u32 = 33;
/// Playback rate when the request names none (Wan was trained at 16).
pub const DEFAULT_FPS: u32 = 16;
/// Most frames one video may have.
pub const MAX_FRAMES: u32 = 257;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

fn bad(message: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// OpenAI's job states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    InProgress,
    Completed,
    Failed,
}

/// One video job: what `GET /v1/videos/{id}` shows (plus the owner, kept private) and what is
/// stored as `<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoJob {
    pub id: String,
    pub model: String,
    pub prompt: String,
    pub status: Status,
    /// 0..=100.
    pub progress: u32,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    /// `WIDTHxHEIGHT`, when the request named it (else the model's own size).
    pub size: Option<String>,
    pub frames: u32,
    pub fps: u32,
    /// `webm`, `webp` or `avi`.
    pub output_format: String,
    pub error: Option<JobError>,
    /// Token that made the job (`None` = no token); only it sees the job.
    #[serde(default)]
    pub owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobError {
    pub code: String,
    pub message: String,
}

impl VideoJob {
    /// Length in seconds as OpenAI shows it (a string).
    fn seconds(&self) -> String {
        let s = f64::from(self.frames.saturating_sub(1)) / f64::from(self.fps.max(1));
        format!("{}", (s * 10.0).round() / 10.0)
    }

    /// The public object.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "object": "video",
            "model": self.model,
            "prompt": self.prompt,
            "status": self.status,
            "progress": self.progress,
            "created_at": self.created_at,
            "completed_at": self.completed_at,
            "expires_at": self.created_at + KEEP_DAYS * 86_400,
            "size": self.size,
            "seconds": self.seconds(),
            "frames": self.frames,
            "fps": self.fps,
            "output_format": self.output_format,
            "error": self.error,
        })
    }

    fn mime(&self) -> &'static str {
        mime(&self.output_format)
    }
}

/// The media type of a container format.
pub fn mime(format: &str) -> &'static str {
    match format {
        "webp" => "image/webp",
        "avi" => "video/x-msvideo",
        _ => "video/webm",
    }
}

/// A request, validated and mapped onto sd-server's `vid_gen` body.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub body: Value,
    pub size: Option<String>,
    pub frames: u32,
    pub fps: u32,
    pub format: String,
}

/// sd-server's frame count: the largest `4n + 1` that is not more than `frames`.
pub fn wan_frames(frames: u32) -> u32 {
    frames.saturating_sub(1) / 4 * 4 + 1
}

/// Validates a `POST /v1/videos` body (OpenAI's `prompt`, `size`, `seconds`, plus Harness's
/// `frames`, `fps`, `negative_prompt`, `steps`, `cfg_scale`, `flow_shift`, `seed`,
/// `output_format` and `image` / `input_reference` for a start frame).
pub fn plan(request: &Value) -> Result<Plan, GatewayError> {
    let prompt = text(request, "prompt")?
        .ok_or_else(|| bad("The `prompt` field (what to film) is required.").param("prompt"))?;
    let whole =
        |key: &str, range: std::ops::RangeInclusive<f64>| -> Result<Option<u32>, GatewayError> {
            match number(request, key)? {
                None => Ok(None),
                Some(v) if range.contains(&v) && v.fract() == 0.0 => Ok(Some(v as u32)),
                Some(_) => Err(bad(format!(
                    "{key} must be a whole number from {} to {}.",
                    range.start(),
                    range.end()
                ))
                .param(key)),
            }
        };
    let fps = whole("fps", 1.0..=60.0)?.unwrap_or(DEFAULT_FPS);
    // OpenAI sends `seconds` as a string ("4").
    let seconds = match request.get("seconds") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            s.trim()
                .parse::<f64>()
                .map_err(|_| bad("seconds must be a number.").param("seconds"))?,
        ),
        Some(v) => Some(
            v.as_f64()
                .ok_or_else(|| bad("seconds must be a number.").param("seconds"))?,
        ),
    };
    let frames = match (whole("frames", 1.0..=f64::from(MAX_FRAMES))?, seconds) {
        (Some(f), _) => f,
        (None, Some(s)) if s > 0.0 && s * f64::from(fps) < f64::from(MAX_FRAMES) => {
            (s * f64::from(fps)).round() as u32 + 1
        }
        (None, Some(_)) => {
            return Err(bad(format!(
                "seconds must be above 0 and give at most {MAX_FRAMES} frames at {fps} fps."
            ))
            .param("seconds"))
        }
        (None, None) => DEFAULT_FRAMES,
    };
    let frames = wan_frames(frames);
    let format = match text(request, "output_format")? {
        None => "webm".to_string(),
        Some(f) if ["webm", "webp", "avi"].contains(&f) => f.to_string(),
        Some(other) => {
            return Err(bad(format!(
                "output_format `{other}` is not supported; use webm, webp (animated) or avi."
            ))
            .param("output_format"))
        }
    };
    let mut body = json!({
        "prompt": prompt,
        "video_frames": frames,
        "fps": fps,
        "output_format": format,
        // The latent projection is cheap and its step counter is the progress.
        "preview": "proj",
    });
    let obj = body.as_object_mut().expect("a JSON object");
    let mut size = None;
    if let Some(s) = text(request, "size")?.filter(|s| *s != "auto") {
        let (w, h) = parse_size(s)?;
        obj.insert("width".into(), json!(w));
        obj.insert("height".into(), json!(h));
        size = Some(format!("{w}x{h}"));
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
    for key in ["image", "input_reference"] {
        if let Some(image) = text(request, key)? {
            obj.insert("init_image".into(), json!(image));
        }
    }
    let mut sample = serde_json::Map::new();
    if let Some(steps) = whole("steps", 1.0..=500.0)? {
        sample.insert("sample_steps".into(), json!(steps));
    }
    if let Some(cfg) = number(request, "cfg_scale")? {
        if !(0.0..=100.0).contains(&cfg) {
            return Err(bad("cfg_scale must be between 0 and 100.").param("cfg_scale"));
        }
        sample.insert("guidance".into(), json!({ "txt_cfg": cfg }));
    }
    if let Some(shift) = number(request, "flow_shift")? {
        if !(0.0..=100.0).contains(&shift) {
            return Err(bad("flow_shift must be between 0 and 100.").param("flow_shift"));
        }
        sample.insert("flow_shift".into(), json!(shift));
    }
    if !sample.is_empty() {
        obj.insert("sample_params".into(), Value::Object(sample));
    }
    Ok(Plan {
        body,
        size,
        frames,
        fps,
        format,
    })
}

/// Progress (0..=99) of a polled sd-server job from its preview step; `None` without one.
pub fn job_progress(v: &Value) -> Option<u32> {
    let step = v.pointer("/preview/step")?.as_u64()?;
    let total = v.pointer("/preview/total_steps")?.as_u64()?.max(1);
    // Decoding the frames comes after the last step: a finished sampling is not done yet.
    Some(((step.min(total) * 99) / total) as u32)
}

/// The jobs, in memory and on disk.
pub struct VideoStore {
    dir: PathBuf,
    jobs: Mutex<HashMap<String, VideoJob>>,
    /// Cancel signals of the jobs that are not finished.
    cancels: Mutex<HashMap<String, Arc<Notify>>>,
    /// One video at a time.
    queue: Semaphore,
}

impl VideoStore {
    /// Opens `<dir>`: finished jobs come back, unfinished ones are `failed` (the gateway
    /// stopped under them), jobs older than [`KEEP_DAYS`] are removed.
    pub fn open(dir: PathBuf) -> Self {
        let mut jobs = HashMap::new();
        let cutoff = now().saturating_sub(KEEP_DAYS * 86_400);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Some(mut job) = std::fs::read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<VideoJob>(&b).ok())
                else {
                    continue;
                };
                if job.created_at < cutoff {
                    remove_files(&dir, &job);
                    continue;
                }
                if matches!(job.status, Status::Queued | Status::InProgress) {
                    job.status = Status::Failed;
                    job.error = Some(JobError {
                        code: "interrupted".into(),
                        message: "The gateway stopped before the video was finished.".into(),
                    });
                    write_job(&dir, &job);
                }
                jobs.insert(job.id.clone(), job);
            }
        }
        VideoStore {
            dir,
            jobs: Mutex::new(jobs),
            cancels: Mutex::new(HashMap::new()),
            queue: Semaphore::new(1),
        }
    }

    pub fn file_of(&self, job: &VideoJob) -> PathBuf {
        file_path(&self.dir, job)
    }

    fn file_for(&self, id: &str) -> Option<PathBuf> {
        let jobs = self.jobs.lock().unwrap();
        jobs.get(id).map(|j| file_path(&self.dir, j))
    }

    /// A job the caller may see.
    pub fn get(&self, id: &str, owner: Option<&str>) -> Option<VideoJob> {
        let jobs = self.jobs.lock().unwrap();
        jobs.get(id)
            .filter(|j| j.owner.as_deref() == owner)
            .cloned()
    }

    /// The caller's jobs, newest first.
    pub fn list(&self, owner: Option<&str>) -> Vec<VideoJob> {
        let jobs = self.jobs.lock().unwrap();
        let mut out: Vec<VideoJob> = jobs
            .values()
            .filter(|j| j.owner.as_deref() == owner)
            .cloned()
            .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        out
    }

    fn insert(&self, job: VideoJob) -> Arc<Notify> {
        let _ = std::fs::create_dir_all(&self.dir);
        write_job(&self.dir, &job);
        let cancel = Arc::new(Notify::new());
        self.cancels
            .lock()
            .unwrap()
            .insert(job.id.clone(), cancel.clone());
        self.jobs.lock().unwrap().insert(job.id.clone(), job);
        cancel
    }

    /// Changes a job (still there: not deleted) and stores it.
    fn update(&self, id: &str, f: impl FnOnce(&mut VideoJob)) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        let Some(job) = jobs.get_mut(id) else {
            return false;
        };
        f(job);
        write_job(&self.dir, job);
        true
    }

    fn finish(&self, id: &str) {
        self.cancels.lock().unwrap().remove(id);
    }

    /// Removes a job the caller owns: cancels it when it is not finished and deletes its
    /// files.
    pub fn delete(&self, id: &str, owner: Option<&str>) -> Option<VideoJob> {
        let job = {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.get(id)?.owner.as_deref() != owner {
                return None;
            }
            jobs.remove(id)?
        };
        if let Some(cancel) = self.cancels.lock().unwrap().remove(id) {
            cancel.notify_one();
        }
        remove_files(&self.dir, &job);
        Some(job)
    }
}

fn file_path(dir: &Path, job: &VideoJob) -> PathBuf {
    dir.join(format!("{}.{}", job.id, job.output_format))
}

fn write_job(dir: &Path, job: &VideoJob) {
    if let Ok(bytes) = serde_json::to_vec_pretty(job) {
        let tmp = dir.join(format!("{}.json.tmp", job.id));
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(format!("{}.json", job.id)));
        }
    }
}

fn remove_files(dir: &Path, job: &VideoJob) {
    let _ = std::fs::remove_file(file_path(dir, job));
    let _ = std::fs::remove_file(dir.join(format!("{}.json", job.id)));
}

/// Fields of a form that are numbers (a form sends everything as text).
const NUMBER_FIELDS: [&str; 7] = [
    "frames",
    "fps",
    "steps",
    "cfg_scale",
    "flow_shift",
    "seed",
    "seconds",
];

/// The body as JSON: OpenAI's SDK sends a multipart form (`input_reference` as a file).
async fn read_request(headers: &HeaderMap, body: Bytes) -> Result<Value, GatewayError> {
    let multipart = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("multipart/form-data"));
    if !multipart {
        return serde_json::from_slice(&body)
            .map_err(|e| bad(format!("The request body is not valid JSON: {e}")));
    }
    let mut req = axum::http::Request::new(axum::body::Body::from(body));
    if let Some(ct) = headers.get(header::CONTENT_TYPE) {
        req.headers_mut().insert(header::CONTENT_TYPE, ct.clone());
    }
    let mut mp = Multipart::from_request(req, &())
        .await
        .map_err(|e| bad(format!("The multipart body could not be read: {e}")))?;
    let mut out = serde_json::Map::new();
    while let Some(field) = mp.next_field().await.map_err(|e| {
        bad(format!(
            "The multipart body could not be read: {}",
            e.body_text()
        ))
    })? {
        let name = field.name().unwrap_or_default().to_string();
        let is_file = field.file_name().is_some()
            || field
                .content_type()
                .is_some_and(|ct| !ct.starts_with("text/"));
        let bytes = field
            .bytes()
            .await
            .map_err(|e| bad(format!("Field {name}: {}", e.body_text())))?;
        let value = if is_file {
            Value::String(B64.encode(&bytes))
        } else {
            let text = String::from_utf8_lossy(&bytes).trim().to_string();
            match serde_json::from_str::<Value>(&text) {
                Ok(n @ Value::Number(_)) if NUMBER_FIELDS.contains(&name.as_str()) => n,
                _ => Value::String(text),
            }
        };
        out.insert(name, value);
    }
    Ok(Value::Object(out))
}

fn not_found(id: &str) -> GatewayError {
    GatewayError::new(404, "video_not_found", format!("There is no video `{id}`."))
}

/// `POST /v1/videos`.
pub async fn create(state: GwState, headers: HeaderMap, body: Bytes) -> Response {
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: "/v1/videos".to_string(),
            ..Default::default()
        },
        trace_context(&headers),
    );
    let result = start(&state, &headers, body, &mut guard).await;
    match result {
        Ok(job) => {
            guard.finish(200, None);
            (StatusCode::OK, axum::Json(job.to_json())).into_response()
        }
        Err(err) => {
            guard.finish(err.status, Some(&err.code));
            err.into_response()
        }
    }
}

async fn start(
    state: &GwState,
    headers: &HeaderMap,
    body: Bytes,
    guard: &mut RecordGuard,
) -> Result<VideoJob, GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Videos)?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    let request = read_request(headers, body).await?;
    let model = text(&request, "model")?.map(str::to_string);
    guard.get().model = model.clone();
    guard.admitted(caller.admit(state)?);
    let plan = plan(&request)?;
    let def = state
        .scheduler
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
            "model_not_video",
            format!(
                "`{}` is not a video model (a stable-diffusion.cpp diffusion model such as Wan); /v1/videos needs one.",
                def.alias
            ),
        )
        .param("model"));
    }
    let job = VideoJob {
        id: format!("video_{}", request_id().trim_start_matches("req_")),
        model: def.alias.clone(),
        prompt: plan.body["prompt"].as_str().unwrap_or_default().to_string(),
        status: Status::Queued,
        progress: 0,
        created_at: now(),
        completed_at: None,
        size: plan.size.clone(),
        frames: plan.frames,
        fps: plan.fps,
        output_format: plan.format.clone(),
        error: None,
        owner: caller.token_name.clone(),
    };
    let cancel = state.videos.insert(job.clone());
    tokio::spawn(run(state.clone(), job.id.clone(), def, plan.body, cancel));
    Ok(job)
}

/// Runs one job to its end (or until it is deleted).
async fn run(
    state: GwState,
    id: String,
    def: harness_core::domain::ModelDefinition,
    body: Value,
    cancel: Arc<Notify>,
) {
    let store = state.videos.clone();
    let outcome = tokio::select! {
        r = generate(&state, &id, &def, &body) => Some(r),
        _ = cancel.notified() => None,
    };
    store.finish(&id);
    let Some(result) = outcome else {
        return;
    };
    match result {
        Ok(bytes) => {
            let Some(path) = store.file_for(&id) else {
                return;
            };
            let written = std::fs::write(&path, &bytes).is_ok();
            // Deleted while the file was being written: no job keeps it.
            let kept = store.update(&id, |j| {
                if written {
                    j.status = Status::Completed;
                    j.progress = 100;
                } else {
                    j.status = Status::Failed;
                    j.error = Some(JobError {
                        code: "write_failed".into(),
                        message: "The video could not be written to the data folder.".into(),
                    });
                }
                j.completed_at = Some(now());
            });
            if !kept {
                let _ = std::fs::remove_file(&path);
            }
        }
        Err(e) => {
            store.update(&id, |j| {
                j.status = Status::Failed;
                j.error = Some(JobError {
                    code: e.code.to_string(),
                    message: e.message.clone(),
                });
                j.completed_at = Some(now());
            });
        }
    }
}

/// Cancels the sd-server job when the generation is dropped (the job was deleted).
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

async fn generate(
    state: &GwState,
    id: &str,
    def: &harness_core::domain::ModelDefinition,
    body: &Value,
) -> Result<Vec<u8>, GatewayError> {
    let store = &state.videos;
    let _turn = store
        .queue
        .acquire()
        .await
        .map_err(|_| GatewayError::new(503, "shutting_down", "The gateway is stopping."))?;
    let lease = state
        .scheduler
        .acquire_tokens(def, None)
        .await
        .map_err(|e| GatewayError::from_sched(&e))?;
    store.update(id, |j| j.status = Status::InProgress);
    let upstream_err = |e: reqwest::Error| {
        GatewayError::new(502, "upstream_unreachable", format!("sd-server: {e}"))
    };
    let started = state
        .http
        .post(format!("{}/sdcpp/v1/vid_gen", lease.upstream))
        .json(body)
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
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let polled = state
            .http
            .get(format!("{}/sdcpp/v1/jobs/{job_id}", lease.upstream))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(upstream_err)?;
        if matches!(polled.status().as_u16(), 404 | 410) {
            return Err(GatewayError::new(
                502,
                "upstream_error",
                "sd-server lost the job.",
            ));
        }
        let v: Value = polled
            .json()
            .await
            .map_err(|e| GatewayError::new(502, "upstream_error", format!("sd-server: {e}")))?;
        match v.get("status").and_then(Value::as_str).unwrap_or("") {
            "completed" => {
                cancel.armed = false;
                let b64 = v
                    .pointer("/result/b64_json")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        GatewayError::new(
                            500,
                            "generation_failed",
                            "sd-server finished without a video.",
                        )
                    })?;
                return B64.decode(b64).map_err(|_| {
                    GatewayError::new(502, "upstream_error", "sd-server sent a broken video.")
                });
            }
            "failed" | "cancelled" => {
                cancel.armed = false;
                let message = v
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("the generation failed");
                return Err(GatewayError::new(
                    500,
                    "generation_failed",
                    format!("sd-server: {message}"),
                ));
            }
            _ => {
                if let Some(p) = job_progress(&v) {
                    store.update(id, |j| j.progress = j.progress.max(p));
                }
            }
        }
    }
}

/// The caller of a read / delete route, or the error to answer.
fn caller_of(state: &GwState, headers: &HeaderMap) -> Result<Option<String>, GatewayError> {
    crate::auth::check(state, headers, false, GatewayEndpoint::Videos).map(|c| c.token_name)
}

/// `GET /v1/videos`.
pub async fn list(state: GwState, headers: HeaderMap) -> Response {
    match caller_of(&state, &headers) {
        Ok(owner) => {
            let data: Vec<Value> = state
                .videos
                .list(owner.as_deref())
                .iter()
                .map(VideoJob::to_json)
                .collect();
            axum::Json(json!({ "object": "list", "data": data })).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// `GET /v1/videos/{id}`.
pub async fn retrieve(state: GwState, headers: HeaderMap, id: String) -> Response {
    let found = caller_of(&state, &headers).and_then(|owner| {
        state
            .videos
            .get(&id, owner.as_deref())
            .ok_or_else(|| not_found(&id))
    });
    match found {
        Ok(job) => axum::Json(job.to_json()).into_response(),
        Err(e) => e.into_response(),
    }
}

/// `GET /v1/videos/{id}/content`: the video file.
pub async fn content(state: GwState, headers: HeaderMap, id: String) -> Response {
    let found = caller_of(&state, &headers).and_then(|owner| {
        state
            .videos
            .get(&id, owner.as_deref())
            .ok_or_else(|| not_found(&id))
    });
    let job = match found {
        Ok(job) => job,
        Err(e) => return e.into_response(),
    };
    if job.status != Status::Completed {
        return GatewayError::new(
            409,
            "video_not_ready",
            format!("The video `{id}` is {:?}, not completed.", job.status).to_lowercase(),
        )
        .into_response();
    }
    match tokio::fs::read(state.videos.file_of(&job)).await {
        Ok(bytes) => {
            let mut resp = (StatusCode::OK, bytes).into_response();
            resp.headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(job.mime()));
            if let Ok(v) = HeaderValue::from_str(&format!(
                "attachment; filename=\"{}.{}\"",
                job.id, job.output_format
            )) {
                resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
            }
            resp
        }
        Err(_) => not_found(&id).into_response(),
    }
}

/// `DELETE /v1/videos/{id}`.
pub async fn delete(state: GwState, headers: HeaderMap, id: String) -> Response {
    let deleted = caller_of(&state, &headers).and_then(|owner| {
        state
            .videos
            .delete(&id, owner.as_deref())
            .ok_or_else(|| not_found(&id))
    });
    match deleted {
        Ok(job) => axum::Json(json!({ "id": job.id, "object": "video.deleted", "deleted": true }))
            .into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_maps_onto_the_native_video_job() {
        let p = plan(&json!({
            "model": "wan", "prompt": "a fox running", "size": "832x480", "seconds": "2",
            "negative_prompt": "blurry", "steps": 20, "cfg_scale": 6.0, "flow_shift": 3.0,
            "seed": 7
        }))
        .unwrap();
        // 2 s at 16 fps = 33 frames (a 4n + 1 count already).
        assert_eq!((p.frames, p.fps, p.format.as_str()), (33, 16, "webm"));
        assert_eq!(p.size.as_deref(), Some("832x480"));
        assert_eq!(
            p.body,
            json!({
                "prompt": "a fox running", "video_frames": 33, "fps": 16, "output_format": "webm",
                "preview": "proj", "width": 832, "height": 480, "negative_prompt": "blurry",
                "seed": 7,
                "sample_params": {"sample_steps": 20, "guidance": {"txt_cfg": 6.0}, "flow_shift": 3.0}
            })
        );
    }

    #[test]
    fn frames_follow_sd_cpp_and_the_defaults_are_wans() {
        assert_eq!(
            [
                wan_frames(33),
                wan_frames(34),
                wan_frames(32),
                wan_frames(1)
            ],
            [33, 33, 29, 1]
        );
        let p = plan(&json!({"prompt": "x"})).unwrap();
        assert_eq!(
            (p.frames, p.fps, p.size),
            (DEFAULT_FRAMES, DEFAULT_FPS, None)
        );
        assert!(p.body.get("width").is_none() && p.body.get("sample_params").is_none());
        // `frames` wins over `seconds`; a start frame from either field.
        let p = plan(&json!({"prompt": "x", "frames": 10, "seconds": 4, "fps": 8,
                             "input_reference": "AAAA", "output_format": "webp"}))
        .unwrap();
        assert_eq!((p.frames, p.fps, p.format.as_str()), (9, 8, "webp"));
        assert_eq!(p.body["init_image"], json!("AAAA"));
        assert_eq!(
            plan(&json!({"prompt": "x", "seconds": 1.5, "fps": 16}))
                .unwrap()
                .frames,
            25
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
        assert_eq!(param(json!({"prompt": "x", "seconds": "a"})), "seconds");
        assert_eq!(param(json!({"prompt": "x", "seconds": 0})), "seconds");
        assert_eq!(param(json!({"prompt": "x", "seconds": 60})), "seconds");
        assert_eq!(param(json!({"prompt": "x", "frames": 0})), "frames");
        assert_eq!(param(json!({"prompt": "x", "frames": 1000})), "frames");
        assert_eq!(param(json!({"prompt": "x", "fps": 2.5})), "fps");
        assert_eq!(param(json!({"prompt": "x", "size": "100x100"})), "size");
        assert_eq!(
            param(json!({"prompt": "x", "output_format": "mp4"})),
            "output_format"
        );
        assert_eq!(param(json!({"prompt": "x", "cfg_scale": -1})), "cfg_scale");
    }

    #[test]
    fn progress_comes_from_the_preview_step() {
        assert_eq!(job_progress(&json!({"status": "generating"})), None);
        let v = json!({"preview": {"step": 5, "total_steps": 10}});
        assert_eq!(job_progress(&v), Some(49));
        let v = json!({"preview": {"step": 10, "total_steps": 10}});
        assert_eq!(job_progress(&v), Some(99));
    }

    fn job(id: &str, owner: Option<&str>, status: Status, created_at: u64) -> VideoJob {
        VideoJob {
            id: id.into(),
            model: "wan".into(),
            prompt: "x".into(),
            status,
            progress: 0,
            created_at,
            completed_at: None,
            size: None,
            frames: 33,
            fps: 16,
            output_format: "webm".into(),
            error: None,
            owner: owner.map(str::to_string),
        }
    }

    #[test]
    fn jobs_survive_a_restart_and_unfinished_ones_fail() {
        let dir = tempfile::tempdir().unwrap();
        let store = VideoStore::open(dir.path().to_path_buf());
        let t = now();
        store.insert(job("video_done", Some("a"), Status::Queued, t));
        store.update("video_done", |j| j.status = Status::Completed);
        std::fs::write(
            store.file_of(&store.get("video_done", Some("a")).unwrap()),
            b"webm",
        )
        .unwrap();
        store.insert(job("video_running", Some("a"), Status::InProgress, t + 1));
        store.insert(job(
            "video_old",
            Some("a"),
            Status::Completed,
            t - (KEEP_DAYS + 1) * 86_400,
        ));
        std::fs::write(dir.path().join("video_old.webm"), b"old").unwrap();

        let again = VideoStore::open(dir.path().to_path_buf());
        let done = again.get("video_done", Some("a")).unwrap();
        assert_eq!(done.status, Status::Completed);
        assert!(again.file_of(&done).is_file());
        let running = again.get("video_running", Some("a")).unwrap();
        assert_eq!(running.status, Status::Failed);
        assert_eq!(running.error.unwrap().code, "interrupted");
        // Too old: gone with its file.
        assert!(again.get("video_old", Some("a")).is_none());
        assert!(!dir.path().join("video_old.webm").exists());
        assert_eq!(
            again
                .list(Some("a"))
                .iter()
                .map(|j| j.id.as_str())
                .collect::<Vec<_>>(),
            ["video_running", "video_done"]
        );
    }

    #[test]
    fn a_job_is_seen_and_deleted_only_by_its_owner() {
        let dir = tempfile::tempdir().unwrap();
        let store = VideoStore::open(dir.path().to_path_buf());
        let cancel = store.insert(job("video_1", Some("alice"), Status::Queued, now()));
        let file = dir.path().join("video_1.webm");
        std::fs::write(&file, b"webm").unwrap();
        assert!(store.get("video_1", Some("bob")).is_none());
        assert!(store.get("video_1", None).is_none());
        assert!(store.list(Some("bob")).is_empty());
        assert!(store.delete("video_1", Some("bob")).is_none());
        // The owner's delete cancels the job and removes its files.
        let notified = cancel.notified();
        assert!(store.delete("video_1", Some("alice")).is_some());
        let woke = futures_util::FutureExt::now_or_never(notified);
        assert!(woke.is_some(), "the running job was told to stop");
        assert!(!file.exists() && !dir.path().join("video_1.json").exists());
        assert!(store.get("video_1", Some("alice")).is_none());
    }

    #[test]
    fn the_public_object_has_openai_shape() {
        let mut j = job("video_1", Some("a"), Status::InProgress, 100);
        j.progress = 40;
        let v = j.to_json();
        assert_eq!(v["object"], "video");
        assert_eq!(v["status"], "in_progress");
        assert_eq!(v["seconds"], "2");
        assert_eq!(v["expires_at"], 100 + KEEP_DAYS * 86_400);
        assert!(v.get("owner").is_none());
        assert_eq!(mime("webp"), "image/webp");
        assert_eq!(mime("avi"), "video/x-msvideo");
    }
}
