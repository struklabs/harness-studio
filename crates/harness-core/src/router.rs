//! llama-server router mode: one process serving many models, loaded on demand.
//!
//! * The model API of the router (`GET /models`, `POST /models/load`, `POST /models/unload`):
//!   which models it knows and whether each is loaded.
//! * The per-model preset file (`--models-preset`, INI): the settings a Harness model
//!   definition loads with (context, GPU layers, KV cache, template, metadata fixes like the
//!   Gemma 3 QAT `sliding_window` override), so a router loads every model the way a
//!   single-model server would.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capabilities::RuntimeCapabilities;
use crate::domain::launch::{resolve_launch, LaunchDefaults};
use crate::domain::{ModelDefinition, ModelKind};
use crate::error_key::err;
use crate::models::LocalModel;
use crate::process::{LaunchConfig, ProcessManager, ServerCaps, ServerStatus};

/// Load state of one model of a router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RouterModelState {
    Loaded,
    Loading,
    Unloaded,
    /// The last load ended in an error (the router reports an exit code).
    Failed,
}

/// One model a router serves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RouterModel {
    /// The name clients send as `model`.
    pub id: String,
    pub state: RouterModelState,
    /// Where the router found it: `models_dir`, `preset` or `cache`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The model file, when the router's arguments name one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    /// Exit code of the last failed load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
}

/// Parses the body of `GET /models` (`{"data": [{"id", "status": {"value", ...}}]}`).
pub fn parse_models(body: &Value) -> Vec<RouterModel> {
    let Some(items) = body.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_string();
            let status = item.get("status");
            let value = status
                .and_then(|s| s.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("unloaded");
            let exit_code = status
                .and_then(|s| s.get("exit_code"))
                .and_then(Value::as_i64);
            let failed = status
                .and_then(|s| s.get("failed"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || exit_code.is_some_and(|c| c != 0);
            let state = match value {
                "loaded" => RouterModelState::Loaded,
                "loading" => RouterModelState::Loading,
                _ if failed => RouterModelState::Failed,
                _ => RouterModelState::Unloaded,
            };
            let path = status
                .and_then(|s| s.get("args"))
                .and_then(Value::as_array)
                .and_then(|args| {
                    let args: Vec<&str> = args.iter().filter_map(Value::as_str).collect();
                    args.iter()
                        .position(|a| *a == "--model" || *a == "-m")
                        .and_then(|i| args.get(i + 1))
                        .map(|p| p.to_string())
                });
            Some(RouterModel {
                id,
                state,
                source: item
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                path,
                context_length: item.get("context_length").and_then(Value::as_u64),
                exit_code,
            })
        })
        .collect()
}

/// Where a running router answers.
#[derive(Debug, Clone)]
pub struct RouterEndpoint {
    pub base: String,
    pub api_key: Option<String>,
    /// The models directory the router serves; models in it that llama.cpp cannot run (whisper
    /// ggml, stable-diffusion, text-to-speech) are left out of [`list`].
    pub models_dir: Option<PathBuf>,
}

/// The endpoint of a running router server; an error for anything else.
pub fn endpoint(pm: &ProcessManager) -> Result<RouterEndpoint, String> {
    let active = match pm.active_server() {
        Some(a) if pm.get_status() == ServerStatus::Running => a,
        _ => return Err(err("router.not_running").into()),
    };
    if !active.router_mode {
        return Err(err("router.not_router").into());
    }
    let host = crate::process::connect_host(&active.connect_host);
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host
    };
    Ok(RouterEndpoint {
        base: format!("http://{host}:{}", active.port),
        api_key: active.api_key.clone(),
        models_dir: pm
            .launch_config()
            .map(|c| c.models_path.unwrap_or(c.model_path)),
    })
}

/// Whether the llama.cpp family can serve a model of this kind (chat, embedding, reranker).
pub fn llama_servable(kind: ModelKind) -> bool {
    matches!(
        kind,
        ModelKind::Llm | ModelKind::Embedding | ModelKind::Reranker
    )
}

/// Names a router gives a library file: its file name, the stem and, for a model in its own
/// folder, the folder name.
fn router_names(m: &LocalModel) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(f) = m.path.file_name().and_then(|n| n.to_str()) {
        names.push(f.to_string());
    }
    if let Some(s) = m.path.file_stem().and_then(|n| n.to_str()) {
        names.push(s.to_string());
    }
    if let Some(d) = m
        .path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
    {
        names.push(d.to_string());
    }
    names
}

/// Drops the models a router lists but llama.cpp cannot run (a whisper.cpp ggml `.bin`, a
/// stable-diffusion GGUF, a speech folder), judged by the kind the library scan recorded.
pub fn retain_servable(models: &mut Vec<RouterModel>, library: &[LocalModel]) {
    use std::collections::HashSet;
    let mut bad_paths = HashSet::new();
    let mut bad: HashSet<String> = HashSet::new();
    let mut good: HashSet<String> = HashSet::new();
    for m in library {
        if llama_servable(m.kind) {
            good.extend(router_names(m));
        } else if m.kind != ModelKind::Mmproj && m.kind != ModelKind::Adapter {
            bad_paths.insert(crate::operation_gate::normalized_path(&m.path));
            bad.extend(router_names(m));
        }
    }
    models.retain(|rm| {
        if let Some(p) = &rm.path {
            if bad_paths.contains(&crate::operation_gate::normalized_path(Path::new(p))) {
                return false;
            }
        }
        !(bad.contains(&rm.id) && !good.contains(&rm.id))
    });
}

fn client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .no_proxy()
        .build()
        .unwrap_or_default()
}

fn failed(detail: impl std::fmt::Display) -> String {
    err("router.request_failed").with("detail", detail).into()
}

async fn request(
    ep: &RouterEndpoint,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    timeout: Duration,
) -> Result<Value, String> {
    let mut req = client(timeout).request(method, format!("{}{path}", ep.base));
    if let Some(key) = &ep.api_key {
        req = req.bearer_auth(key);
    }
    if let Some(body) = body {
        req = req.json(&body);
    }
    let res = req.send().await.map_err(failed)?;
    let status = res.status();
    let text = res.text().await.map_err(failed)?;
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        let message = json
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or(text.trim());
        return Err(failed(format!("HTTP {}: {message}", status.as_u16())));
    }
    Ok(json)
}

/// The models of a router and their state.
pub async fn list(ep: &RouterEndpoint) -> Result<Vec<RouterModel>, String> {
    let body = request(
        ep,
        reqwest::Method::GET,
        "/models",
        None,
        Duration::from_secs(5),
    )
    .await?;
    let mut models = parse_models(&body);
    if let Some(dir) = ep.models_dir.clone() {
        let library = tokio::task::spawn_blocking(move || {
            crate::models::ModelManager.scan_directories(&[dir])
        })
        .await
        .unwrap_or_default();
        retain_servable(&mut models, &library);
    }
    Ok(models)
}

/// Asks the router to load a model; with `wait` returns once it is loaded (or has failed).
pub async fn load(
    ep: &RouterEndpoint,
    model: &str,
    wait: Option<Duration>,
) -> Result<RouterModel, String> {
    request(
        ep,
        reqwest::Method::POST,
        "/models/load",
        Some(serde_json::json!({"model": model})),
        Duration::from_secs(30),
    )
    .await?;
    let deadline = wait.map(|w| Instant::now() + w);
    loop {
        let models = list(ep).await?;
        let found = models.into_iter().find(|m| m.id == model);
        let Some(found) = found else {
            return Err(failed(format!("unknown model: {model}")));
        };
        match (found.state, deadline) {
            (RouterModelState::Failed, _) => {
                return Err(err("router.load_failed")
                    .with("model", model)
                    .with("code", found.exit_code.unwrap_or(-1))
                    .into())
            }
            (RouterModelState::Loaded, _) | (_, None) => return Ok(found),
            (_, Some(d)) if Instant::now() > d => {
                return Err(err("router.load_timeout").with("model", model).into())
            }
            _ => tokio::time::sleep(Duration::from_millis(400)).await,
        }
    }
}

/// Asks the router to unload a model.
pub async fn unload(ep: &RouterEndpoint, model: &str) -> Result<RouterModel, String> {
    request(
        ep,
        reqwest::Method::POST,
        "/models/unload",
        Some(serde_json::json!({"model": model})),
        Duration::from_secs(30),
    )
    .await?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let found = list(ep).await?.into_iter().find(|m| m.id == model);
        match found {
            None => return Err(failed(format!("unknown model: {model}"))),
            Some(m) if m.state != RouterModelState::Loaded || Instant::now() > deadline => {
                return Ok(m)
            }
            _ => tokio::time::sleep(Duration::from_millis(300)).await,
        }
    }
}

// --- Presets ---

/// Options that belong to the router process itself or name the model file; a preset section
/// never carries them (the router adds the model and its own connection settings).
fn is_server_level(key: &str) -> bool {
    matches!(
        key,
        "host"
            | "port"
            | "api-key"
            | "api-key-file"
            | "alias"
            | "a"
            | "model"
            | "m"
            | "models-dir"
            | "models-path"
            | "models-preset"
            | "models-max"
            | "models-autoload"
            | "no-models-autoload"
            | "hf-repo"
            | "hf-file"
            | "hf"
            | "hfr"
            | "hff"
            | "model-url"
            | "mu"
            | "cors-origins"
            | "mcp-servers-config"
            | "path"
            | "api-prefix"
            | "metrics"
            | "props"
            | "slots"
            | "no-slots"
            | "timeout"
            | "to"
            | "threads-http"
            | "reuse-port"
            | "tools"
    ) || key.starts_with("webui")
        || key.starts_with("no-webui")
        || key.starts_with("ssl-")
        || key.starts_with("log-")
        || matches!(key, "v" | "lv" | "verbose" | "verbosity")
}

/// Command-line arguments (`--ctx-size 4096 --jinja -fa on`) as preset entries: names without
/// their dashes, a flag without a value as `true`, server-level options dropped. A repeated
/// option keeps its last value.
pub fn args_to_entries(args: &[String]) -> Vec<(String, String)> {
    let is_value = |s: &str| !s.starts_with('-') || s.len() > 1 && s[1..].parse::<f64>().is_ok();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if !arg.starts_with('-') || arg.len() < 2 {
            continue;
        }
        let (key, inline) = match arg.trim_start_matches('-').split_once('=') {
            Some((k, v)) if arg.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (arg.trim_start_matches('-').to_string(), None),
        };
        let value = match inline {
            Some(v) => v,
            None => match args.get(i) {
                Some(next) if is_value(next) => {
                    i += 1;
                    next.clone()
                }
                _ => "true".to_string(),
            },
        };
        if is_server_level(&key) || value.contains('\n') || value.contains('\r') {
            continue;
        }
        out.retain(|(k, _)| *k != key);
        out.push((key, value));
    }
    out
}

/// One `[section]` of a preset: the name clients ask for, the file, and the load options.
#[derive(Debug, Clone, PartialEq)]
pub struct PresetModel {
    pub name: String,
    pub model: PathBuf,
    pub entries: Vec<(String, String)>,
}

/// The INI text of a preset file (`version = 1`, one section per model).
pub fn render_preset(models: &[PresetModel]) -> String {
    let mut out = String::from("version = 1\n");
    for m in models {
        out.push_str(&format!("\n[{}]\n", m.name));
        out.push_str(&format!("model = {}\n", m.model.display()));
        for (k, v) in &m.entries {
            out.push_str(&format!("{k} = {v}\n"));
        }
    }
    out
}

/// The preset section of a model definition: what a single-model server would launch it with
/// (definition settings, serving mode, automatic context, the known GGUF metadata fixes).
pub fn preset_for_definition(
    def: &ModelDefinition,
    defaults: &LaunchDefaults,
    runtime: Option<&RuntimeCapabilities>,
) -> Result<PresetModel, String> {
    let mut config = resolve_launch(defaults, Some(def), None, &Default::default())?;
    config.router_mode = false;
    let meta = crate::models::cached_metadata(&config.model_path);
    let prepared = crate::serve_mode::prepare(&mut config, meta.as_ref(), runtime);
    if config.context_size == 0 {
        config.context_size = crate::prepare::resolve_auto_context(
            prepared.mode == crate::serve_mode::ServeMode::Chat,
            config.custom_args.as_deref(),
            meta.as_ref().and_then(|m| m.context_length),
        );
    }
    let caps = runtime.map(ServerCaps::from_runtime).unwrap_or_default();
    let out = ProcessManager::build_args_with(&config, &caps, runtime)?;
    Ok(PresetModel {
        name: def.alias.clone(),
        model: config.model_path,
        entries: args_to_entries(&out.args),
    })
}

/// Whether llama-server can load the file: a GGUF that is not an image generator's.
fn runs_on_llama(path: &Path) -> bool {
    crate::engine::ModelFormat::of(path) == crate::engine::ModelFormat::Gguf
        && !crate::engine::is_image_gguf(path, crate::models::cached_metadata(path).as_ref())
}

fn same_or_inside(file: &Path, dir: &Path) -> bool {
    let file = crate::operation_gate::normalized_path(file);
    let dir = crate::operation_gate::normalized_path(dir);
    file.starts_with(dir)
}

/// The presets of every definition whose file lies in `models_dir`.
pub fn presets_for_dir(
    defs: &[ModelDefinition],
    models_dir: &Path,
    defaults: &LaunchDefaults,
    runtime: Option<&RuntimeCapabilities>,
) -> Vec<PresetModel> {
    defs.iter()
        .filter(|d| {
            d.base
                .local_path()
                .is_some_and(|p| p.is_file() && same_or_inside(p, models_dir) && runs_on_llama(p))
        })
        .filter_map(|d| preset_for_definition(d, defaults, runtime).ok())
        .collect()
}

/// Where the preset of server `id` is written.
pub fn preset_path(id: &str) -> PathBuf {
    crate::paths::config_dir()
        .join("router-presets")
        .join(format!("{id}.ini"))
}

/// Writes the preset for a router launch and returns its path; `None` when no definition lies
/// in the served directory (the router then uses its defaults for everything). Blocking.
pub fn write_preset_for_launch(
    server_id: &str,
    config: &LaunchConfig,
    runtime: Option<&RuntimeCapabilities>,
) -> Result<Option<PathBuf>, String> {
    let dir = config.models_path.as_ref().unwrap_or(&config.model_path);
    let defs = crate::domain::store::Domain::global().definitions()?;
    let models = presets_for_dir(&defs, dir, &LaunchDefaults::load(), runtime);
    if models.is_empty() {
        return Ok(None);
    }
    let path = preset_path(server_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, render_preset(&models)).map_err(|e| e.to_string())?;
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn library_file(path: &str, kind: ModelKind) -> LocalModel {
        let path = PathBuf::from(path);
        LocalModel {
            id: path.to_string_lossy().to_string(),
            name: path.file_stem().unwrap().to_string_lossy().to_string(),
            filename: path.file_name().unwrap().to_string_lossy().to_string(),
            path: path.clone(),
            size_bytes: 1,
            architecture: None,
            context_length: None,
            quantization: String::new(),
            is_split: false,
            total_shards: None,
            present_shards: None,
            is_ready: true,
            shard_paths: vec![path],
            root_dir: String::new(),
            gguf: None,
            source: None,
            mmproj_path: None,
            kind,
            origin: Default::default(),
            task: None,
        }
    }

    fn router_model(id: &str) -> RouterModel {
        RouterModel {
            id: id.to_string(),
            state: RouterModelState::Unloaded,
            source: Some("models_dir".into()),
            path: None,
            context_length: None,
            exit_code: None,
        }
    }

    #[test]
    fn only_llama_servable_models_stay_in_the_router_list() {
        let library = [
            library_file("C:/m/ggml-base.en.bin", ModelKind::Asr),
            library_file("C:/m/sd_turbo-f16-q8_0.gguf", ModelKind::Image),
            library_file("C:/m/qwen3-Q4_K_M.gguf", ModelKind::Llm),
            library_file("C:/mbge-m3-Q8_0.gguf", ModelKind::Embedding),
            library_file("C:/mbge-reranker-Q8_0.gguf", ModelKind::Reranker),
        ];
        let mut models: Vec<RouterModel> = [
            "ggml-base.en",
            "sd_turbo-f16-q8_0",
            "qwen3-Q4_K_M",
            "bge-m3-Q8_0",
            "bge-reranker-Q8_0",
            "not-in-library",
        ]
        .iter()
        .map(|id| router_model(id))
        .collect();
        // A model whose arguments name the file is matched by path as well.
        models[0].id = "renamed".into();
        models[0].path = Some("C:/m/ggml-base.en.bin".into());
        retain_servable(&mut models, &library);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "qwen3-Q4_K_M",
                "bge-m3-Q8_0",
                "bge-reranker-Q8_0",
                "not-in-library"
            ]
        );
    }

    #[test]
    fn a_folder_name_hides_a_model_only_when_nothing_servable_shares_it() {
        let library = [
            library_file("C:/m/sd/sd_turbo.gguf", ModelKind::Image),
            library_file("C:/m/llm/qwen.gguf", ModelKind::Llm),
            library_file("C:/m/llm/mmproj-qwen.gguf", ModelKind::Mmproj),
        ];
        let mut models = vec![router_model("sd"), router_model("llm")];
        retain_servable(&mut models, &library);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "llm");
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn models_response_is_parsed() {
        let body = json!({"data": [
            {"id": "qwen3", "source": "preset", "context_length": 40960,
             "status": {"value": "loaded", "args": ["llama-server", "--alias", "qwen3",
                "--model", "C:\\m\\q.gguf", "--n-gpu-layers", "0"]}},
            {"id": "qwen05", "source": "models_dir", "status": {"value": "unloaded"}},
            {"id": "warming", "status": {"value": "loading"}},
            {"id": "broken", "status": {"value": "unloaded", "failed": true, "exit_code": 1}},
            {"status": {"value": "loaded"}}
        ]});
        let models = parse_models(&body);
        assert_eq!(models.len(), 4);
        assert_eq!(models[0].state, RouterModelState::Loaded);
        assert_eq!(models[0].path.as_deref(), Some("C:\\m\\q.gguf"));
        assert_eq!(models[0].context_length, Some(40960));
        assert_eq!(models[0].source.as_deref(), Some("preset"));
        assert_eq!(models[1].state, RouterModelState::Unloaded);
        assert_eq!(models[1].path, None);
        assert_eq!(models[2].state, RouterModelState::Loading);
        assert_eq!(models[3].state, RouterModelState::Failed);
        assert_eq!(models[3].exit_code, Some(1));
        assert!(parse_models(&json!({})).is_empty());
    }

    #[test]
    fn arguments_become_preset_entries() {
        let args = strings(&[
            "-m",
            "C:\\m\\q.gguf",
            "--alias",
            "q",
            "--host",
            "127.0.0.1",
            "--port",
            "8080",
            "-ngl",
            "99",
            "--ctx-size=8192",
            "--jinja",
            "-fa",
            "on",
            "--rope-freq-scale",
            "-0.5",
            "--override-kv",
            "gemma3.attention.sliding_window=int:512",
            "--no-warmup",
            "--api-key",
            "secret",
            "--log-colors",
            "off",
            "--cache-type-k",
            "q8_0",
            "--cache-type-k",
            "q4_0",
        ]);
        let entries = args_to_entries(&args);
        let get = |k: &str| {
            entries
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("ngl"), Some("99"));
        assert_eq!(get("ctx-size"), Some("8192"));
        assert_eq!(get("jinja"), Some("true"));
        assert_eq!(get("fa"), Some("on"));
        assert_eq!(get("rope-freq-scale"), Some("-0.5"));
        assert_eq!(get("no-warmup"), Some("true"));
        assert_eq!(
            get("override-kv"),
            Some("gemma3.attention.sliding_window=int:512")
        );
        assert_eq!(get("cache-type-k"), Some("q4_0"), "the last value wins");
        for gone in ["m", "alias", "host", "port", "api-key", "log-colors"] {
            assert_eq!(get(gone), None, "{gone} is server-level");
        }
        assert_eq!(
            entries.iter().filter(|(k, _)| k == "cache-type-k").count(),
            1
        );
    }

    #[test]
    fn multi_line_values_are_dropped() {
        let args = strings(&["--chat-template", "a\nb", "--jinja"]);
        let entries = args_to_entries(&args);
        assert_eq!(entries, vec![("jinja".to_string(), "true".to_string())]);
    }

    #[test]
    fn preset_text_has_one_section_per_model() {
        let text = render_preset(&[
            PresetModel {
                name: "gemma-1b".into(),
                model: PathBuf::from("C:\\m\\gemma.gguf"),
                entries: vec![
                    ("ctx-size".into(), "4096".into()),
                    (
                        "override-kv".into(),
                        "gemma3.attention.sliding_window=int:512".into(),
                    ),
                ],
            },
            PresetModel {
                name: "qwen".into(),
                model: PathBuf::from("C:\\m\\qwen.gguf"),
                entries: vec![],
            },
        ]);
        assert_eq!(
            text,
            "version = 1\n\n[gemma-1b]\nmodel = C:\\m\\gemma.gguf\nctx-size = 4096\n\
             override-kv = gemma3.attention.sliding_window=int:512\n\n[qwen]\nmodel = C:\\m\\qwen.gguf\n"
        );
    }

    #[test]
    fn a_definition_preset_carries_its_settings() {
        use crate::domain::ModelRef;
        use crate::params::ParamValue;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("m.gguf");
        std::fs::write(&file, b"GGUF").unwrap();
        let mut def = ModelDefinition::new("my-model", ModelRef::Local { path: file.clone() });
        def.id = "md-1".into();
        def.load
            .insert("context_size".into(), ParamValue::Int(2048));
        def.load.insert("gpu_layers".into(), ParamValue::Int(7));
        let defaults = LaunchDefaults {
            gpu_layers: 0,
            context_size: 0,
            host: "127.0.0.1".into(),
            port: 8080,
            models_dir: dir.path().to_path_buf(),
        };
        let preset = preset_for_definition(&def, &defaults, None).unwrap();
        assert_eq!(preset.name, "my-model");
        assert_eq!(preset.model, file);
        let get = |k: &str| {
            preset
                .entries
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("c").or(get("ctx-size")), Some("2048"));
        assert_eq!(get("ngl").or(get("n-gpu-layers")), Some("7"));
        assert!(get("port").is_none() && get("host").is_none() && get("m").is_none());

        // Only definitions inside the served directory are listed.
        let other = tempfile::tempdir().unwrap();
        assert_eq!(
            presets_for_dir(&[def.clone()], dir.path(), &defaults, None).len(),
            1
        );
        assert!(presets_for_dir(&[def], other.path(), &defaults, None).is_empty());
    }
}
