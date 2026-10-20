//! ComfyUI bridge: run image workflows on the user's own ComfyUI.
//!
//! Harness never installs, ships or starts ComfyUI. It only talks to an instance the user
//! already runs, at the address saved in Settings, through ComfyUI's HTTP API:
//!
//! * `GET /system_stats`: version and devices (the "detect" check);
//! * `GET /api/userdata?dir=workflows`, `GET /api/userdata/workflows%2F<file>`: saved workflows;
//! * `GET /templates/index.json`, `GET /workflow_templates`: core and custom-node templates;
//! * `GET /object_info`: node definitions (checkpoint list; converting editor-format workflows);
//! * `POST /prompt`, `GET /history/<id>`, `GET /queue`, `GET /view`: queue a run, follow it and
//!   download its pictures;
//! * `POST /queue` (`delete`) and `POST /interrupt`: cancel;
//! * `GET /ws?clientId=`: progress while a run executes. Polling `/history` and `/queue` decides
//!   the outcome, so a missing socket (or `https`, which the socket client does not speak) only
//!   costs the step counter.
//!
//! Pictures are saved to `<data dir>/outputs/comfyui`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error_key::err;

pub mod workflow;

#[cfg(test)]
mod tests;

pub use workflow::{Graph, ImageRef, Txt2ImgParams};

pub const ERR_URL_INVALID: &str = "comfyui.url_invalid";
pub const ERR_NOT_CONFIGURED: &str = "comfyui.not_configured";

/// Address ComfyUI listens on by default (shown as the form's placeholder).
pub const DEFAULT_URL: &str = "http://127.0.0.1:8188";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How often `/history` and `/queue` are read while a run is in flight.
const POLL_INTERVAL: Duration = Duration::from_millis(800);
/// A run that has not finished by then is given up (the ComfyUI job keeps running).
const RUN_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
/// Polls in a row that find the prompt neither queued nor in the history: it was removed.
const GONE_AFTER: u32 = 3;
/// Largest picture downloaded from `/view`, and pictures kept from one run.
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGES: usize = 16;
/// Longest error text handed back from ComfyUI.
const MESSAGE_CHARS: usize = 600;

/// `settings.json` -> `comfyui`. Changed only through [`save`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComfySettings {
    /// Base URL of the user's ComfyUI (`http://127.0.0.1:8188`), empty = not set.
    pub url: String,
}

/// What the detect check found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComfyStatus {
    /// The normalized address that answered.
    pub url: String,
    pub version: Option<String>,
    pub python_version: Option<String>,
    pub os: Option<String>,
    pub devices: Vec<ComfyDevice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComfyDevice {
    pub name: String,
    /// `cuda`, `mps`, `cpu`, ...
    pub kind: String,
    pub vram_total: u64,
    pub vram_free: u64,
}

/// A workflow saved in ComfyUI's user data (`user/default/workflows`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWorkflow {
    /// Path under `workflows/`, e.g. `portraits/sdxl.json`.
    pub path: String,
    pub size: Option<u64>,
    /// Seconds since the epoch.
    pub modified: Option<f64>,
}

/// A workflow template: ComfyUI's own (`core`) or one shipped by a custom node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    /// `core` or the custom node's module name.
    pub source: String,
    pub name: String,
    pub title: String,
    pub description: String,
    /// The template list's category (`Image`, `Image Tools`...), empty for custom nodes.
    pub category: String,
}

/// Everything the Playground lists for a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub workflows: Vec<SavedWorkflow>,
    pub templates: Vec<Template>,
    /// Files `CheckpointLoaderSimple` offers (the built-in graph needs one).
    pub checkpoints: Vec<String>,
}

/// Which graph a run uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowSource {
    /// Harness's own checkpoint -> KSampler -> save graph.
    Builtin,
    Saved {
        path: String,
    },
    Template {
        source: String,
        name: String,
    },
}

/// A step of a run, sent to the UI while it is in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEvent {
    pub prompt_id: String,
    /// `queued`, `running` or `progress`.
    pub stage: String,
    /// Sampler step (`progress` only).
    pub value: u64,
    pub max: u64,
    /// Node being executed, when known.
    pub node: Option<String>,
}

/// A picture of a finished run, downloaded from ComfyUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunImage {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub prompt_id: String,
    pub images: Vec<RunImage>,
}

/// A picture written to the outputs folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedImage {
    pub path: String,
    /// `png`, `jpeg` or `webp`.
    pub format: String,
}

// --- URL and settings ---

/// Validates the address: `http(s)://host[:port][/prefix]`, no credentials, query or fragment.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let bad = || -> String { err(ERR_URL_INVALID).into() };
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| bad())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bad());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub fn settings() -> ComfySettings {
    crate::config::AppSettings::load().comfyui
}

/// Saves the address; an empty one clears it.
pub fn save(url: &str) -> Result<ComfySettings, String> {
    let mut all = crate::config::AppSettings::load();
    all.comfyui = ComfySettings {
        url: if url.trim().is_empty() {
            String::new()
        } else {
            normalize_url(url)?
        },
    };
    all.save()?;
    Ok(all.comfyui)
}

/// Where run pictures are saved.
pub fn outputs_dir() -> PathBuf {
    crate::paths::data_root().join("outputs").join("comfyui")
}

/// A path the UI may open or reveal: an existing file inside [`outputs_dir`].
pub fn openable_output(path: &str) -> Result<PathBuf, String> {
    let not_found = || String::from(err("comfyui.output_not_found"));
    let file = Path::new(path).canonicalize().map_err(|_| not_found())?;
    let root = outputs_dir().canonicalize().map_err(|_| not_found())?;
    if !file.starts_with(&root) || !file.is_file() {
        return Err(not_found());
    }
    Ok(file)
}

/// Writes a run's pictures to `dir` (never over an existing file).
pub fn save_images(dir: &Path, images: &[RunImage]) -> Result<Vec<SavedImage>, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| String::from(err("comfyui.save_failed").with("error", e)))?;
    let mut out = Vec::new();
    for image in images {
        let format = image_format(&image.bytes)
            .ok_or_else(|| String::from(err("comfyui.invalid_image")))?;
        let stem = crate::paths::sanitize_file_name(&image.filename)
            .map(|n| match n.rsplit_once('.') {
                Some((s, _)) if !s.is_empty() => s.to_string(),
                _ => n,
            })
            .unwrap_or_else(|| "comfyui".into());
        let ext = if format == "jpeg" { "jpg" } else { format };
        let mut n = 1;
        loop {
            let name = if n == 1 {
                format!("{stem}.{ext}")
            } else {
                format!("{stem} ({n}).{ext}")
            };
            let path = dir.join(name);
            match std::fs::File::create_new(&path) {
                Ok(mut f) => {
                    use std::io::Write as _;
                    f.write_all(&image.bytes).map_err(|e| {
                        String::from(err("comfyui.save_failed").with("error", e))
                    })?;
                    out.push(SavedImage {
                        path: path.to_string_lossy().into_owned(),
                        format: format.to_string(),
                    });
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 10_000 => n += 1,
                Err(e) => return Err(err("comfyui.save_failed").with("error", e).into()),
            }
        }
    }
    Ok(out)
}

fn image_format(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [0xff, 0xd8, 0xff, ..] => Some("jpeg"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        _ => None,
    }
}

/// A random seed in the range ComfyUI's widgets accept.
pub fn random_seed() -> u64 {
    let mut b = [0u8; 8];
    if getrandom::fill(&mut b).is_err() {
        b = uuid::Uuid::new_v4().as_bytes()[..8].try_into().unwrap_or_default();
    }
    u64::from_le_bytes(b) >> 14
}

// --- Client ---

pub struct ComfyClient {
    base: reqwest::Url,
    http: reqwest::Client,
}

/// What one poll of a queued prompt found.
enum Poll {
    Pending,
    Done(Value),
    Gone,
}

impl ComfyClient {
    pub fn new(url: &str) -> Result<Self, String> {
        let normalized = normalize_url(url)?;
        let base = reqwest::Url::parse(&normalized).map_err(|_| String::from(err(ERR_URL_INVALID)))?;
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| crate::http_error::transport(&e, &normalized))?;
        Ok(Self { base, http })
    }

    /// A client for the saved address.
    pub fn saved() -> Result<Self, String> {
        let url = settings().url;
        if url.is_empty() {
            return Err(err(ERR_NOT_CONFIGURED).into());
        }
        Self::new(&url)
    }

    /// A client for a form's (possibly unsaved) address; empty falls back to the saved one.
    pub fn from_form(url: Option<&str>) -> Result<Self, String> {
        match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) => Self::new(u),
            None => Self::saved(),
        }
    }

    pub fn base_url(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_string()
    }

    fn endpoint(&self, segments: &[&str]) -> reqwest::Url {
        let mut url = self.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        url
    }

    async fn send(&self, req: reqwest::RequestBuilder, url: &str) -> Result<reqwest::Response, String> {
        let resp = req
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, url))?;
        checked(resp)
    }

    async fn get_json(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Value, String> {
        self.get_json_opt(segments, query)
            .await?
            .ok_or_else(|| err("comfyui.http_error").num("status", 404).into())
    }

    /// `None` when the server answers 404 (a folder or route that does not exist there).
    async fn get_json_opt(
        &self,
        segments: &[&str],
        query: &[(&str, &str)],
    ) -> Result<Option<Value>, String> {
        let url = self.endpoint(segments);
        let resp = self
            .http
            .get(url.clone())
            .query(query)
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, url.as_str()))?;
        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        let resp = checked(resp)?;
        resp.json::<Value>()
            .await
            .map(Some)
            .map_err(|_| err("comfyui.invalid_response").into())
    }

    /// The detect check: `/system_stats` must answer like ComfyUI.
    pub async fn status(&self) -> Result<ComfyStatus, String> {
        let not_comfyui = || String::from(err("comfyui.not_comfyui"));
        let v = match self.get_json_opt(&["system_stats"], &[]).await {
            Ok(Some(v)) => v,
            Ok(None) => return Err(not_comfyui()),
            // Something answered, but not like ComfyUI.
            Err(e) if e.contains("comfyui.") => return Err(not_comfyui()),
            Err(e) => return Err(e),
        };
        let system = v
            .get("system")
            .filter(|s| s.is_object())
            .ok_or_else(not_comfyui)?;
        let text = |key: &str| system.get(key).and_then(Value::as_str).map(str::to_string);
        let devices = v
            .get("devices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|d| ComfyDevice {
                name: d.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                kind: d.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
                vram_total: d.get("vram_total").and_then(Value::as_u64).unwrap_or(0),
                vram_free: d.get("vram_free").and_then(Value::as_u64).unwrap_or(0),
            })
            .collect();
        Ok(ComfyStatus {
            url: self.base_url(),
            version: text("comfyui_version"),
            python_version: text("python_version").map(|p| {
                p.split_whitespace().next().unwrap_or_default().to_string()
            }),
            os: text("os"),
            devices,
        })
    }

    /// Saved workflows (none when the folder does not exist yet).
    pub async fn workflows(&self) -> Result<Vec<SavedWorkflow>, String> {
        let Some(v) = self
            .get_json_opt(
                &["api", "userdata"],
                &[("dir", "workflows"), ("recurse", "true"), ("full_info", "true"), ("split", "false")],
            )
            .await?
        else {
            return Ok(Vec::new());
        };
        let list = v
            .as_array()
            .ok_or_else(|| String::from(err("comfyui.invalid_response")))?;
        let mut out: Vec<SavedWorkflow> = list
            .iter()
            .filter_map(|w| match w {
                Value::String(p) => Some(SavedWorkflow {
                    path: p.clone(),
                    size: None,
                    modified: None,
                }),
                Value::Object(o) => Some(SavedWorkflow {
                    path: o.get("path")?.as_str()?.to_string(),
                    size: o.get("size").and_then(Value::as_u64),
                    modified: o.get("modified").and_then(Value::as_f64),
                }),
                _ => None,
            })
            .filter(|w| w.path.ends_with(".json") && saved_path_ok(&w.path))
            .collect();
        out.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
        Ok(out)
    }

    /// Core templates (`/templates/index.json`, when the templates package is installed) and
    /// custom-node templates (`/workflow_templates`). A missing list is an empty one. Only
    /// image categories are kept, without partner-API (`api_*`, paid) and app-mode templates.
    pub async fn templates(&self) -> Vec<Template> {
        let mut out = Vec::new();
        if let Ok(Value::Array(categories)) = self.get_json(&["templates", "index.json"], &[]).await {
            for category in &categories {
                if category
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t != "image")
                {
                    continue;
                }
                let category_title = category
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let source = category
                    .get("moduleName")
                    .and_then(Value::as_str)
                    .filter(|m| !m.is_empty() && *m != "default")
                    .unwrap_or("core");
                for t in category
                    .get("templates")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(name) = t.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    if !template_name_ok(name) || name.starts_with("api_") || name.ends_with(".app") {
                        continue;
                    }
                    let text = |k: &str| t.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    let title = text("title");
                    out.push(Template {
                        source: source.to_string(),
                        name: name.to_string(),
                        title: if title.is_empty() { name.to_string() } else { title },
                        description: text("description"),
                        category: category_title.clone(),
                    });
                }
            }
        }
        if let Ok(Value::Object(modules)) = self.get_json(&["workflow_templates"], &[]).await {
            for (module, names) in &modules {
                if !template_name_ok(module) {
                    continue;
                }
                for name in names.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    if template_name_ok(name) {
                        out.push(Template {
                            source: module.clone(),
                            name: name.to_string(),
                            title: name.to_string(),
                            description: String::new(),
                            category: String::new(),
                        });
                    }
                }
            }
        }
        out
    }

    /// Checkpoint files `CheckpointLoaderSimple` can load.
    pub async fn checkpoints(&self) -> Result<Vec<String>, String> {
        let v = self
            .get_json(&["object_info", "CheckpointLoaderSimple"], &[])
            .await?;
        let entry = v.pointer("/CheckpointLoaderSimple/input/required/ckpt_name");
        // Older servers: `[[choices], {...}]`; newer: `["COMBO", {"options": [choices]}]`.
        let choices = entry
            .and_then(|e| e.get(0))
            .and_then(Value::as_array)
            .or_else(|| {
                entry
                    .and_then(|e| e.get(1))
                    .and_then(|o| o.get("options"))
                    .and_then(Value::as_array)
            });
        Ok(choices
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect())
    }

    pub async fn catalog(&self) -> Result<Catalog, String> {
        let (workflows, templates, checkpoints) =
            tokio::join!(self.workflows(), self.templates(), self.checkpoints());
        Ok(Catalog {
            workflows: workflows?,
            templates,
            checkpoints: checkpoints?,
        })
    }

    async fn load(&self, source: &WorkflowSource) -> Result<Value, String> {
        match source {
            WorkflowSource::Builtin => Ok(Value::Null),
            WorkflowSource::Saved { path } => {
                if !saved_path_ok(path) {
                    return Err(err("comfyui.workflow_path_invalid").into());
                }
                let file = format!("workflows/{path}");
                self.get_json(&["api", "userdata", &file], &[]).await
            }
            WorkflowSource::Template { source, name } => {
                if !template_name_ok(source) || !template_name_ok(name) {
                    return Err(err("comfyui.workflow_path_invalid").into());
                }
                let file = format!("{name}.json");
                if source == "core" {
                    self.get_json(&["templates", &file], &[]).await
                } else {
                    self.get_json(&["api", "workflow_templates", source, &file], &[])
                        .await
                }
            }
        }
    }

    /// The graph a run sends: the workflow (converted to the API format when needed) with the
    /// run's parameters written in.
    pub async fn prepare(
        &self,
        source: &WorkflowSource,
        params: &Txt2ImgParams,
        seed: u64,
    ) -> Result<Graph, String> {
        params.check()?;
        let mut graph = match source {
            WorkflowSource::Builtin => {
                let ckpt = match params.checkpoint.as_deref().filter(|c| !c.is_empty()) {
                    Some(c) => c.to_string(),
                    None => self
                        .checkpoints()
                        .await?
                        .into_iter()
                        .next()
                        .ok_or_else(|| String::from(err("comfyui.no_checkpoint")))?,
                };
                workflow::builtin_txt2img(&ckpt)
            }
            other => {
                let file = self.load(other).await?;
                if workflow::is_api_format(&file) {
                    file.as_object().cloned().unwrap_or_default()
                } else if workflow::is_ui_format(&file) {
                    let info = self.get_json(&["object_info"], &[]).await?;
                    workflow::ui_to_api(&file, &info)?
                } else {
                    return Err(err("comfyui.workflow_invalid").into());
                }
            }
        };
        workflow::apply_params(&mut graph, params, seed)?;
        Ok(graph)
    }

    async fn queue_prompt(&self, graph: &Graph, client_id: &str) -> Result<String, String> {
        let url = self.endpoint(&["prompt"]);
        let resp = self
            .http
            .post(url.clone())
            .json(&json!({"prompt": graph, "client_id": client_id}))
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, url.as_str()))?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            if status.as_u16() == 400 {
                return Err(err("comfyui.prompt_rejected")
                    .with("error", rejection_text(&body))
                    .into());
            }
            return Err(err("comfyui.http_error").num("status", status.as_u16()).into());
        }
        body.get("prompt_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| err("comfyui.invalid_response").into())
    }

    fn socket_url(&self, client_id: &str) -> Option<String> {
        let mut url = self.endpoint(&["ws"]);
        // The socket client is built without TLS: `https` instances are followed by polling.
        if url.scheme() != "http" || url.set_scheme("ws").is_err() {
            return None;
        }
        url.query_pairs_mut().append_pair("clientId", client_id);
        Some(url.to_string())
    }

    async fn poll(&self, prompt_id: &str) -> Result<Poll, String> {
        let history = self.get_json(&["history", prompt_id], &[]).await?;
        if let Some(entry) = history.get(prompt_id) {
            let status = entry.pointer("/status/status_str").and_then(Value::as_str);
            let messages = entry.pointer("/status/messages").and_then(Value::as_array);
            let message = |kind: &str| {
                messages
                    .into_iter()
                    .flatten()
                    .find(|m| m.get(0).and_then(Value::as_str) == Some(kind))
                    .and_then(|m| m.get(1))
            };
            if message("execution_interrupted").is_some() {
                return Err(err("comfyui.cancelled").into());
            }
            if status == Some("error") {
                let text = message("execution_error")
                    .map(execution_error_text)
                    .unwrap_or_default();
                return Err(err("comfyui.execution_failed").with("error", text).into());
            }
            let completed = entry
                .pointer("/status/completed")
                .and_then(Value::as_bool)
                .unwrap_or(status == Some("success"));
            if completed {
                return Ok(Poll::Done(entry.clone()));
            }
            return Ok(Poll::Pending);
        }
        let queue = self.get_json(&["queue"], &[]).await?;
        let queued = ["queue_running", "queue_pending"].iter().any(|k| {
            queue
                .get(*k)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|item| item.get(1).and_then(Value::as_str) == Some(prompt_id))
        });
        Ok(if queued { Poll::Pending } else { Poll::Gone })
    }

    /// Queues a graph and waits for it; `on_event` sees the queue and step progress. Fails
    /// with `comfyui.cancelled` when the run is interrupted or removed from the queue.
    pub async fn run(
        &self,
        graph: &Graph,
        on_event: &(dyn Fn(RunEvent) + Send + Sync),
    ) -> Result<RunOutcome, String> {
        let client_id = uuid::Uuid::new_v4().to_string();
        // Connected before queueing, so no progress message is missed.
        let mut socket = match self.socket_url(&client_id) {
            Some(url) => tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
                .await
                .ok()
                .and_then(Result::ok)
                .map(|(s, _)| s),
            None => None,
        };
        let prompt_id = self.queue_prompt(graph, &client_id).await?;
        let event = |stage: &str, value: u64, max: u64, node: Option<String>| RunEvent {
            prompt_id: prompt_id.clone(),
            stage: stage.to_string(),
            value,
            max,
            node,
        };
        on_event(event("queued", 0, 0, None));
        let deadline = Instant::now() + RUN_TIMEOUT;
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        let mut gone = 0;
        let entry = loop {
            if Instant::now() > deadline {
                return Err(err("comfyui.timeout").into());
            }
            let next = async {
                match socket.as_mut() {
                    Some(s) => s.next().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                msg = next => match msg {
                    Some(Ok(m)) => {
                        if let Ok(text) = m.to_text() {
                            if let Some(e) = socket_event(text, &prompt_id) {
                                on_event(event(&e.0, e.1, e.2, e.3));
                            }
                        }
                    }
                    // Closed or broken: polling carries on alone.
                    _ => socket = None,
                },
                _ = ticker.tick() => match self.poll(&prompt_id).await? {
                    Poll::Done(entry) => break entry,
                    Poll::Pending => gone = 0,
                    Poll::Gone => {
                        gone += 1;
                        if gone >= GONE_AFTER {
                            return Err(err("comfyui.cancelled").into());
                        }
                    }
                },
            }
        };
        let refs = workflow::output_images(&entry);
        if refs.is_empty() {
            return Err(err("comfyui.no_outputs").into());
        }
        let mut images = Vec::new();
        for r in refs.into_iter().take(MAX_IMAGES) {
            images.push(self.download(&r).await?);
        }
        Ok(RunOutcome { prompt_id, images })
    }

    async fn download(&self, r: &ImageRef) -> Result<RunImage, String> {
        let url = self.endpoint(&["view"]);
        let resp = self
            .send(
                self.http.get(url.clone()).query(&[
                    ("filename", r.filename.as_str()),
                    ("subfolder", r.subfolder.as_str()),
                    ("type", r.kind.as_str()),
                ]),
                url.as_str(),
            )
            .await?;
        if resp.content_length().is_some_and(|n| n > MAX_IMAGE_BYTES) {
            return Err(err("comfyui.output_too_large").into());
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| crate::http_error::transport(&e, url.as_str()))?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(err("comfyui.output_too_large").into());
        }
        Ok(RunImage {
            filename: r.filename.clone(),
            bytes: bytes.to_vec(),
        })
    }

    /// Cancels a run: removes it from the queue and, when it is the one executing, interrupts it.
    pub async fn cancel(&self, prompt_id: &str) -> Result<(), String> {
        if !prompt_id_ok(prompt_id) {
            return Err(err("comfyui.invalid_response").into());
        }
        let url = self.endpoint(&["queue"]);
        self.send(
            self.http.post(url.clone()).json(&json!({"delete": [prompt_id]})),
            url.as_str(),
        )
        .await?;
        let queue = self.get_json(&["queue"], &[]).await?;
        let running = queue
            .get("queue_running")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|item| item.get(1).and_then(Value::as_str) == Some(prompt_id));
        if running {
            let url = self.endpoint(&["interrupt"]);
            self.send(
                self.http.post(url.clone()).json(&json!({"prompt_id": prompt_id})),
                url.as_str(),
            )
            .await?;
        }
        Ok(())
    }
}

fn checked(resp: reqwest::Response) -> Result<reqwest::Response, String> {
    let status = resp.status();
    if status.is_success() {
        Ok(resp)
    } else if status.is_redirection() {
        Err(err("comfyui.redirect").into())
    } else {
        Err(err("comfyui.http_error").num("status", status.as_u16()).into())
    }
}

/// `(stage, value, max, node)` of a socket message about this prompt.
fn socket_event(text: &str, prompt_id: &str) -> Option<(String, u64, u64, Option<String>)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let data = v.get("data")?;
    if data.get("prompt_id").and_then(Value::as_str) != Some(prompt_id) {
        return None;
    }
    let node = data.get("node").and_then(Value::as_str).map(str::to_string);
    match v.get("type")?.as_str()? {
        "progress" => Some((
            "progress".into(),
            data.get("value").and_then(Value::as_u64).unwrap_or(0),
            data.get("max").and_then(Value::as_u64).unwrap_or(0),
            node,
        )),
        "execution_start" => Some(("running".into(), 0, 0, None)),
        "executing" if node.is_some() => Some(("running".into(), 0, 0, node)),
        _ => None,
    }
}

/// ComfyUI's answer to a rejected prompt: the main message and the first node error.
fn rejection_text(body: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(m) = body.pointer("/error/message").and_then(Value::as_str) {
        parts.push(m.to_string());
    }
    if let Some(d) = body
        .pointer("/error/details")
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
    {
        parts.push(d.to_string());
    }
    let first_node = body
        .get("node_errors")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next());
    if let Some(node) = first_node {
        let class = node.get("class_type").and_then(Value::as_str).unwrap_or("");
        if let Some(e) = node.pointer("/errors/0") {
            let msg = e.get("message").and_then(Value::as_str).unwrap_or("");
            let details = e.get("details").and_then(Value::as_str).unwrap_or("");
            parts.push(format!("{class}: {msg} {details}").trim().to_string());
        }
    }
    clip(&parts.join(" - "), MESSAGE_CHARS)
}

fn execution_error_text(data: &Value) -> String {
    let node = data.get("node_type").and_then(Value::as_str).unwrap_or("");
    let msg = data
        .get("exception_message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    clip(
        &if node.is_empty() {
            msg.to_string()
        } else {
            format!("{node}: {msg}")
        },
        MESSAGE_CHARS,
    )
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}...", &text[..i]),
        None => text.to_string(),
    }
}

/// A saved workflow path: relative, `/`-separated, no `.`/`..` parts, a `.json` file.
fn saved_path_ok(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && path.ends_with(".json")
        && !path.contains(['\\', '\0'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

/// A template or custom-node module name: one plain path segment.
fn template_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
}

fn prompt_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}
