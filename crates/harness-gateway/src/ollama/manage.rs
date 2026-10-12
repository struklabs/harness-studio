//! Model management of the Ollama-compatible API: `POST /api/create`, `POST /api/copy`,
//! `DELETE /api/delete`, `POST /api/push`.
//!
//! Harness has no model registry, so these work on **model definitions** (alias + GGUF + load
//! settings + default inference preset), not on Ollama layers:
//!
//! * `create` builds a definition from `from` (an existing Harness model: its definition is
//!   copied, then changed) or from `files` (a GGUF uploaded with `POST /api/blobs/:digest`, see
//!   [`super::blobs`]). `adapters` (GGUF LoRA blobs), `system`, `parameters`, `messages`,
//!   `template` and `license` go through the same mapping as the Library's Modelfile import
//!   ([`harness_core::modelfile::import_items`]), so values and warnings match; the legacy
//!   `modelfile` text is parsed with the same parser. `quantize` is refused.
//! * `copy` duplicates a definition under a new alias.
//! * `delete` removes the definition. Model files are never touched except GGUF blobs that
//!   `create` stored under `ollama-blobs/` and that no other definition uses.
//! * `push` always answers with an explicit error (there is no registry).
//!
//! A definition made by `create` is marked in its description ([`MARK`]); creating the same
//! name again replaces only such definitions, anything else is protected (409).

use axum::body::Body;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use harness_core::domain::store::Domain;
use harness_core::domain::{
    validate_alias, AdapterRef, InferencePreset, ModelDefinition, SamplingParams,
};
use harness_core::gateway::scheduler::EXTERNAL_PREFIX;
use harness_core::modelfile::{self, ImportOptions, Item};
use serde_json::{json, Map, Value};

use super::blobs;
use super::convert::{self, NameRef};
use super::{auth, bad_request, error_response, model_field, parse_body, resolve, NDJSON};
use crate::errors::GatewayError;
use crate::GwState;

/// Start of the description (and of the preset description) of everything `create` makes.
pub const MARK: &str = "Ollama API create";

// --- request ---

/// A parsed `POST /api/create` body.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateSpec {
    /// Requested name as sent.
    pub model: String,
    pub from: Option<String>,
    /// `files`: file name -> `sha256` hex of an uploaded blob.
    pub files: Vec<(String, String)>,
    /// `adapters`: file name -> `sha256` hex of an uploaded GGUF LoRA blob.
    pub adapters: Vec<(String, String)>,
    /// Modelfile-style instructions for everything but `FROM` / `ADAPTER`.
    pub items: Vec<Item>,
    pub stream: bool,
}

fn item(key: &str, value: impl Into<String>) -> Item {
    Item {
        line: 0,
        key: key.into(),
        value: value.into(),
    }
}

fn digest_map(v: Option<&Value>, field: &str) -> Result<Vec<(String, String)>, GatewayError> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let obj = v
        .as_object()
        .ok_or_else(|| bad_request(format!("{field} must be an object of file name -> digest")))?;
    obj.iter()
        .map(|(name, d)| {
            let d = d.as_str().ok_or_else(|| {
                bad_request(format!("{field}: the digest of {name:?} must be a string"))
            })?;
            blobs::parse_digest(d)
                .map(|hex| (name.clone(), hex))
                .map_err(bad_request)
        })
        .collect()
}

fn text_field(req: &Map<String, Value>, key: &str) -> Result<Option<String>, GatewayError> {
    match req.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(bad_request(format!("{key} must be a string"))),
    }
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Reads and validates the body of `POST /api/create`.
pub fn parse_create(req: &Map<String, Value>) -> Result<CreateSpec, GatewayError> {
    let model = model_field(req).ok_or_else(|| bad_request("model is required"))?;
    // `quantize` re-encodes weights with Ollama's quantizer; Harness has none.
    if req
        .get("quantize")
        .or_else(|| req.get("quantization"))
        .and_then(|q| q.as_str())
        .is_some_and(|q| !q.trim().is_empty())
    {
        return Err(GatewayError::new(
            501,
            "not_supported",
            "quantize is not supported by Harness's Ollama-compatible API: create the model from an already quantized GGUF (or quantize with llama.cpp's llama-quantize).",
        ));
    }
    let stream = req.get("stream").and_then(|s| s.as_bool()).unwrap_or(true);
    let mut from = text_field(req, "from")?
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty());
    let files = digest_map(req.get("files"), "files")?;
    let adapters = digest_map(req.get("adapters"), "adapters")?;
    let mut items: Vec<Item> = Vec::new();

    // Legacy form: a Modelfile text. Its FROM names a Harness model; ADAPTER paths (files on
    // the server) are refused, upload a blob and use `adapters` instead.
    if let Some(text) = text_field(req, "modelfile")?.filter(|t| !t.trim().is_empty()) {
        for it in modelfile::parse(&text).map_err(|e| bad_request(keyed_text(&e)))? {
            match it.key.as_str() {
                "from" => {
                    if from.is_some() {
                        return Err(bad_request("send either from or a modelfile with FROM"));
                    }
                    from = Some(it.value.trim().to_string()).filter(|f| !f.is_empty());
                }
                "adapter" => {
                    return Err(bad_request(
                        "ADAPTER in a modelfile is not accepted: upload the GGUF with POST /api/blobs/:digest and send it in adapters",
                    ))
                }
                _ => items.push(it),
            }
        }
    }
    if let Some(t) = text_field(req, "template")?.filter(|t| !t.trim().is_empty()) {
        items.push(item("template", t));
    }
    if let Some(s) = text_field(req, "system")?.filter(|s| !s.trim().is_empty()) {
        items.push(item("system", s));
    }
    if let Some(l) = text_field(req, "license")?.filter(|l| !l.trim().is_empty()) {
        items.push(item("license", l));
    }
    match req.get("parameters") {
        None | Some(Value::Null) => {}
        Some(Value::Object(p)) => {
            for (k, v) in p {
                let values: Vec<&Value> = match v {
                    Value::Array(a) => a.iter().collect(),
                    other => vec![other],
                };
                for v in values {
                    let s = scalar(v).ok_or_else(|| {
                        bad_request(format!(
                            "parameters.{k} must be a string, number or array of them"
                        ))
                    })?;
                    items.push(item("parameter", format!("{k} {s}")));
                }
            }
        }
        Some(_) => return Err(bad_request("parameters must be an object")),
    }
    match req.get("messages") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) => {
            for m in a {
                let (Some(role), Some(content)) = (
                    m.get("role").and_then(|r| r.as_str()),
                    m.get("content").and_then(|c| c.as_str()),
                ) else {
                    return Err(bad_request(
                        "each message needs a role and a content string",
                    ));
                };
                items.push(item("message", format!("{role} {content}")));
            }
        }
        Some(_) => return Err(bad_request("messages must be an array")),
    }
    if from.is_some() && !files.is_empty() {
        return Err(bad_request(
            "send either from (an existing model) or files (an uploaded GGUF), not both",
        ));
    }
    if from.is_none() && files.is_empty() {
        return Err(bad_request(
            "from or files is required: name an existing Harness model, or upload a GGUF with POST /api/blobs/:digest and send it in files",
        ));
    }
    Ok(CreateSpec {
        model,
        from,
        files,
        adapters,
        items,
        stream,
    })
}

/// The alias a requested name stands for: `name:latest` is the bare alias `name`, a
/// `hf.co/...` name cannot be created.
pub fn alias_of(name: &str) -> Result<String, GatewayError> {
    let alias = match convert::parse_name(name) {
        NameRef::Hf { .. } => {
            return Err(bad_request(format!(
                "{name:?} is a Hugging Face reference; pick a plain model name"
            )))
        }
        NameRef::Alias(c) => c.last().cloned().unwrap_or_default(),
    };
    validate_alias(&alias).map_err(|_| {
        bad_request(format!(
            "invalid model name {:?}: use letters, digits and . _ : - (at most {} characters)",
            name,
            harness_core::domain::ALIAS_MAX_LEN
        ))
    })?;
    Ok(alias)
}

// --- errors ---

/// A core error string (an i18n key with parameters, or plain text) as readable text.
pub fn keyed_text(e: &str) -> String {
    match harness_core::error_key::parse(e) {
        Some((key, params)) if !params.is_empty() => {
            let detail: Vec<String> = params
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}={}",
                        k,
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string())
                    )
                })
                .collect();
            format!("{} ({})", key, detail.join(", "))
        }
        _ => e.to_string(),
    }
}

fn core_error(e: String) -> GatewayError {
    let status = harness_core::error_key::parse(&e)
        .map(|(key, _)| match key {
            "definitions.used_by_profile"
            | "definitions.alias_exists"
            | "definitions.alias_already_exists" => 409,
            k => harness_core::error_key::http_status(k),
        })
        .unwrap_or(500);
    GatewayError::new(status, "model_management_failed", keyed_text(&e))
}

// --- create ---

/// What `create` did: the NDJSON status lines (the last is `success`).
#[derive(Debug)]
pub struct Created {
    pub alias: String,
    pub lines: Vec<Value>,
}

/// Human text of a Modelfile mapping warning (the status line shows it).
pub fn warning_text(w: &modelfile::Warning) -> String {
    let d = &w.detail;
    match w.code.as_str() {
        "template_ignored" => "TEMPLATE ignored: the GGUF's own chat template is used".into(),
        "template_ignored_no_jinja" => {
            "TEMPLATE ignored (Go templates are not supported) and the GGUF has no chat template of its own".into()
        }
        "messages_as_examples" => format!(
            "{d} MESSAGE turn(s) were added to the system prompt as an example conversation"
        ),
        "ignored_parameter" => format!("parameter ignored: {d}"),
        "unknown_parameter" => format!("unknown parameter ignored: {d}"),
        "invalid_value" => format!("invalid value ignored: {d}"),
        "ignored_value" => format!("value ignored: {d}"),
        code => format!("{code}: {d}"),
    }
}

fn status(s: impl Into<String>) -> Value {
    json!({"status": s.into()})
}

/// `sampling` of `base` with every field `over` sets replaced (generic over the field list:
/// unset fields are not serialized).
fn overlay_sampling(base: &SamplingParams, over: &SamplingParams) -> SamplingParams {
    let (Ok(Value::Object(mut b)), Ok(Value::Object(o))) =
        (serde_json::to_value(base), serde_json::to_value(over))
    else {
        return over.clone();
    };
    b.extend(o);
    serde_json::from_value(Value::Object(b)).unwrap_or_else(|_| over.clone())
}

fn blob_file(
    dir: &std::path::Path,
    hex: &str,
    name: &str,
) -> Result<std::path::PathBuf, GatewayError> {
    let p = blobs::blob_path_in(dir, hex);
    if !p.is_file() {
        return Err(bad_request(format!(
            "file {name:?} (sha256:{hex}) was not uploaded: send it with POST /api/blobs/sha256:{hex} first"
        )));
    }
    Ok(p)
}

/// Blocking part of `create` (reads GGUF headers, writes the definition store).
pub fn create_blocking(
    spec: &CreateSpec,
    alias: &str,
    parent: Option<ModelDefinition>,
    blob_dir: &std::path::Path,
) -> Result<Created, GatewayError> {
    let domain = Domain::global();
    let defs = domain.definitions().map_err(core_error)?;
    let existing = defs.iter().find(|d| d.alias.eq_ignore_ascii_case(alias));
    if let Some(e) = existing {
        if !e.description.starts_with(MARK) {
            return Err(GatewayError::new(
                409,
                "model_exists",
                format!(
                    "model \"{}\" already exists and was not created through the Ollama API: choose another name, or delete it in Harness Studio first",
                    e.alias
                ),
            ));
        }
    }
    let mut lines: Vec<Value> = Vec::new();

    // Weights: the parent's file, or the uploaded GGUF (a "clip" GGUF is the projector).
    let mut mmproj = None;
    let base_path = match &parent {
        Some(p) => {
            let path = p.base.local_path().ok_or_else(|| {
                bad_request(format!(
                    "model \"{}\" has no downloaded GGUF yet (download it in Harness Studio first)",
                    p.alias
                ))
            })?;
            if !path.is_file() {
                return Err(bad_request(format!(
                    "the GGUF of model \"{}\" is missing: {}",
                    p.alias,
                    path.display()
                )));
            }
            path.to_path_buf()
        }
        None => {
            lines.push(status("parsing GGUF"));
            let mut weights = Vec::new();
            for (name, hex) in &spec.files {
                let path = blob_file(blob_dir, hex, name)?;
                if !blobs::is_gguf(&path) {
                    return Err(bad_request(format!(
                        "file {name:?} is not a GGUF: only GGUF models can be created (convert safetensors with llama.cpp's convert_hf_to_gguf.py)"
                    )));
                }
                let clip = harness_core::gguf::read_gguf_metadata(&path)
                    .map(|m| m.architecture.as_deref() == Some("clip"))
                    .unwrap_or(false);
                lines.push(status(format!("using existing layer sha256:{hex}")));
                if clip {
                    mmproj = Some(path);
                } else {
                    weights.push((name.clone(), path));
                }
            }
            match weights.as_slice() {
                [(_, p)] => p.clone(),
                [] => {
                    return Err(bad_request(
                        "files has no model GGUF (only a multimodal projector)",
                    ))
                }
                _ => {
                    return Err(bad_request(
                        "files has several model GGUFs: Harness creates one model from one GGUF",
                    ))
                }
            }
        }
    };

    // The shared Modelfile mapping.
    let mut items = vec![item("from", base_path.display().to_string())];
    let mut adapter_refs: Vec<AdapterRef> = Vec::new();
    for (name, hex) in &spec.adapters {
        let path = blob_file(blob_dir, hex, name)?;
        if !blobs::is_gguf(&path) {
            return Err(bad_request(format!(
                "adapter {name:?} is not a GGUF LoRA adapter (convert safetensors adapters with llama.cpp's convert_lora_to_gguf.py)"
            )));
        }
        lines.push(status(format!("using existing layer sha256:{hex}")));
        adapter_refs.push(AdapterRef { path, scale: None });
    }
    items.extend(spec.items.iter().cloned());
    let mapped = modelfile::import_items(
        &items,
        &ImportOptions {
            alias: Some(alias.to_string()),
            ..Default::default()
        },
    )
    .map_err(|e| bad_request(keyed_text(&e)))?;
    for w in &mapped.warnings {
        lines.push(status(format!("warning: {}", warning_text(w))));
    }

    // The definition.
    let from_label = spec
        .from
        .clone()
        .unwrap_or_else(|| "uploaded GGUF".to_string());
    let mut def = match &parent {
        Some(p) => p.clone(),
        None => mapped.import.definition.clone(),
    };
    if parent.is_some() {
        for (k, v) in &mapped.import.definition.load {
            def.load.insert(k.clone(), v.clone());
        }
    }
    def.id = existing.map(|e| e.id.clone()).unwrap_or_default();
    def.alias = alias.to_string();
    def.description = format!("{MARK} (from {from_label})");
    def.adapters.extend(adapter_refs);
    if mmproj.is_some() {
        def.mmproj = mmproj;
    }
    def.auto_created = false;
    def.created_at = 0;
    def.updated_at = 0;

    // The preset: the parent's default with the request's values on top, as a new user preset.
    let old_preset = existing.and_then(|e| e.default_preset_id.clone());
    let mut new_preset_id: Option<String> = None;
    if let Some(over) = mapped.import.preset.clone() {
        let base = def
            .default_preset_id
            .as_deref()
            .and_then(|id| domain.inference_preset(id).ok());
        let preset = match base {
            Some(b) => InferencePreset {
                id: String::new(),
                name: alias.to_string(),
                builtin: false,
                description: MARK.to_string(),
                system_prompt: over.system_prompt.or(b.system_prompt),
                sampling: overlay_sampling(&b.sampling, &over.sampling),
                reasoning: b.reasoning,
                response_format: b.response_format,
                tool_choice: b.tool_choice,
                created_at: 0,
                updated_at: 0,
            },
            None => InferencePreset {
                description: MARK.to_string(),
                ..over
            },
        };
        let saved = domain.save_inference_preset(preset).map_err(core_error)?;
        def.default_preset_id = Some(saved.id.clone());
        new_preset_id = Some(saved.id);
    }
    let saved = match domain.save_definition(def) {
        Ok(d) => d,
        Err(e) => {
            if let Some(id) = new_preset_id {
                let _ = domain.delete_inference_preset(&id);
            }
            return Err(core_error(e));
        }
    };
    // Replaced definition: its old preset goes when nothing uses it any more.
    if let Some(old) = old_preset.filter(|o| Some(o) != saved.default_preset_id.as_ref()) {
        drop_unused_preset(&domain, &old);
    }
    lines.push(status("writing manifest"));
    lines.push(status("success"));
    Ok(Created {
        alias: saved.alias,
        lines,
    })
}

/// Deletes the user preset `id` when `create` made it and no definition uses it.
fn drop_unused_preset(domain: &Domain, id: &str) {
    let Ok(defs) = domain.definitions() else {
        return;
    };
    if defs
        .iter()
        .any(|d| d.default_preset_id.as_deref() == Some(id))
    {
        return;
    }
    if domain
        .inference_preset(id)
        .is_ok_and(|p| !p.builtin && p.description == MARK)
    {
        let _ = domain.delete_inference_preset(id);
    }
}

fn ndjson(lines: &[Value]) -> Response {
    let mut out = Vec::new();
    for l in lines {
        out.extend(convert::line(l));
    }
    ([(header::CONTENT_TYPE, NDJSON)], Body::from(out)).into_response()
}

/// `POST /api/create`.
pub async fn create(state: GwState, headers: HeaderMap, body: Bytes) -> Response {
    match run_create(&state, &headers, body).await {
        Ok((stream, created)) => {
            eprintln!(
                "[Harness Gateway] /api/create: model tanımı {} oluşturuldu",
                created.alias
            );
            if stream {
                ndjson(&created.lines)
            } else {
                axum::Json(json!({"status": "success"})).into_response()
            }
        }
        Err(e) => error_response(e),
    }
}

async fn run_create(
    state: &GwState,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<(bool, Created), GatewayError> {
    let caller = auth(state, headers)?;
    let req = parse_body(&body)?;
    let spec = parse_create(&req)?;
    let alias = alias_of(&spec.model)?;
    caller.check_model(&alias)?;
    let parent = match &spec.from {
        Some(from) => {
            let def = resolve(state, from).await.map_err(|e| {
                if e.status == 404 {
                    GatewayError::new(
                        404,
                        "model_not_found",
                        format!(
                            "from model \"{from}\" not found: Harness has no model registry, so from must be an existing local model (see /api/tags)"
                        ),
                    )
                } else {
                    e
                }
            })?;
            caller.check_model(&def.alias)?;
            if def.id.starts_with(EXTERNAL_PREFIX) {
                return Err(bad_request(format!(
                    "\"{from}\" is a running server without a model definition: create a definition for it first"
                )));
            }
            Some(def)
        }
        None => None,
    };
    let stream = spec.stream;
    let dir = blobs::blob_dir();
    let created = tokio::task::spawn_blocking(move || create_blocking(&spec, &alias, parent, &dir))
        .await
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))??;
    Ok((stream, created))
}

// --- copy ---

/// `POST /api/copy` (`source`, `destination`): a copy of the definition under a new alias.
pub async fn copy(state: GwState, headers: HeaderMap, body: Bytes) -> Response {
    match run_copy(&state, &headers, body).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => error_response(e),
    }
}

async fn run_copy(state: &GwState, headers: &HeaderMap, body: Bytes) -> Result<(), GatewayError> {
    let caller = auth(state, headers)?;
    let req = parse_body(&body)?;
    let text = |k: &str| {
        req.get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let source = text("source").ok_or_else(|| bad_request("source is required"))?;
    let destination = text("destination").ok_or_else(|| bad_request("destination is required"))?;
    let alias = alias_of(&destination)?;
    let def = resolve(state, &source).await?;
    caller.check_model(&def.alias)?;
    caller.check_model(&alias)?;
    if def.id.starts_with(EXTERNAL_PREFIX) {
        return Err(bad_request(format!(
            "\"{source}\" is a running server without a model definition: nothing to copy"
        )));
    }
    tokio::task::spawn_blocking(move || {
        Domain::global()
            .duplicate_definition(&def.id, Some(alias))
            .map(|_| ())
    })
    .await
    .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?
    .map_err(|e| {
        let mut err = core_error(e);
        if err.status == 409 {
            err.message = format!(
                "{} (copy never overwrites: delete the destination first)",
                err.message
            );
        }
        err
    })
}

// --- delete ---

/// `DELETE /api/delete` (`model`).
pub async fn delete(state: GwState, headers: HeaderMap, body: Bytes) -> Response {
    match run_delete(&state, &headers, body).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => error_response(e),
    }
}

async fn run_delete(state: &GwState, headers: &HeaderMap, body: Bytes) -> Result<(), GatewayError> {
    let caller = auth(state, headers)?;
    let req = parse_body(&body)?;
    let name = model_field(&req).ok_or_else(|| bad_request("model is required"))?;
    let def = resolve(state, &name).await?;
    caller.check_model(&def.alias)?;
    if def.id.starts_with(EXTERNAL_PREFIX) {
        return Err(bad_request(format!(
            "\"{name}\" is a running server without a model definition: stop it in Harness Studio"
        )));
    }
    // A model the gateway holds in memory is released first (a failure here is not fatal).
    let _ = state.scheduler.unload(&def.id).await;
    let removed = tokio::task::spawn_blocking(move || delete_blocking(&def))
        .await
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?
        .map_err(core_error)?;
    for f in removed {
        eprintln!(
            "[Harness Gateway] /api/delete: {} silindi (Ollama API ile yüklenen blob)",
            f.display()
        );
    }
    Ok(())
}

/// Deletes the definition; returns the blob files removed with it. The definition's GGUF is
/// only removed when it is an uploaded blob nothing else uses; the default preset only when
/// `create` made it and nothing else uses it.
pub fn delete_blocking(def: &ModelDefinition) -> Result<Vec<std::path::PathBuf>, String> {
    let domain = Domain::global();
    domain.delete_definition(&def.id)?;
    let remaining = domain.definitions()?;
    if let Some(p) = def.default_preset_id.as_deref() {
        drop_unused_preset(&domain, p);
    }
    let mut files: Vec<&std::path::Path> = def.base.local_path().into_iter().collect();
    files.extend(def.mmproj.as_deref());
    files.extend(def.adapters.iter().map(|a| a.path.as_path()));
    let in_use = |p: &std::path::Path| {
        remaining.iter().any(|d| {
            d.base.local_path() == Some(p)
                || d.mmproj.as_deref() == Some(p)
                || d.adapters.iter().any(|a| a.path == p)
        })
    };
    let mut removed = Vec::new();
    for p in files {
        if blobs::is_blob_file(p) && !in_use(p) && std::fs::remove_file(p).is_ok() {
            removed.push(p.to_path_buf());
        }
    }
    Ok(removed)
}

// --- push ---

/// `POST /api/push` (and `/api/me`, which `ollama push` / `signin` call first): there is no
/// registry to push to and no account.
pub async fn push() -> Response {
    error_response(GatewayError::new(
        501,
        "not_supported",
        "push is not supported: Harness has no model registry and no accounts. Share a model as a file (Library > Export) or publish its GGUF on Hugging Face.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn parses_the_current_api_fields() {
        let spec = parse_create(&obj(json!({
            "model": "mymodel",
            "from": "qwen3-4b",
            "system": "You are terse.",
            "template": "{{ .Prompt }}",
            "license": "MIT",
            "parameters": {"temperature": 0.3, "num_ctx": 8192, "stop": ["<a>", "<b>"]},
            "messages": [{"role": "user", "content": "hi"}, {"role": "assistant", "content": "yo"}],
            "stream": false
        })))
        .unwrap();
        assert_eq!(spec.model, "mymodel");
        assert_eq!(spec.from.as_deref(), Some("qwen3-4b"));
        assert!(!spec.stream);
        let keys: Vec<(&str, &str)> = spec
            .items
            .iter()
            .map(|i| (i.key.as_str(), i.value.as_str()))
            .collect();
        assert!(keys.contains(&("system", "You are terse.")));
        assert!(keys.contains(&("template", "{{ .Prompt }}")));
        assert!(keys.contains(&("parameter", "temperature 0.3")));
        assert!(keys.contains(&("parameter", "num_ctx 8192")));
        assert!(keys.contains(&("parameter", "stop <a>")));
        assert!(keys.contains(&("parameter", "stop <b>")));
        assert!(keys.contains(&("message", "assistant yo")));
    }

    #[test]
    fn files_are_digests_and_stream_defaults_on() {
        let spec = parse_create(&obj(json!({
            "name": "m",
            "files": {"model.gguf": format!("sha256:{HEX}")},
            "adapters": {"lora.gguf": format!("sha256:{HEX}")}
        })))
        .unwrap();
        assert!(spec.stream);
        assert_eq!(
            spec.files,
            vec![("model.gguf".to_string(), HEX.to_string())]
        );
        assert_eq!(spec.adapters.len(), 1);
    }

    #[test]
    fn refuses_what_cannot_work() {
        let err = |v: Value| parse_create(&obj(v)).unwrap_err();
        let e = err(json!({"model": "m", "from": "x", "quantize": "q4_K_M"}));
        assert_eq!(e.status, 501);
        assert!(e.message.contains("quantize"));
        assert_eq!(err(json!({"from": "x"})).status, 400); // no model
        assert_eq!(err(json!({"model": "m"})).status, 400); // no source
        assert_eq!(
            err(json!({"model": "m", "from": "x", "files": {"a.gguf": format!("sha256:{HEX}")}}))
                .status,
            400
        );
        assert_eq!(
            err(json!({"model": "m", "files": {"a.gguf": "sha256:xyz"}})).status,
            400
        );
        assert_eq!(
            err(json!({"model": "m", "from": "x", "parameters": []})).status,
            400
        );
        assert_eq!(
            err(json!({"model": "m", "from": "x", "parameters": {"stop": [{"a": 1}]}})).status,
            400
        );
        assert_eq!(
            err(json!({"model": "m", "from": "x", "messages": [{"role": "user"}]})).status,
            400
        );
    }

    #[test]
    fn empty_quantize_is_fine() {
        assert!(parse_create(&obj(json!({"model": "m", "from": "x", "quantize": ""}))).is_ok());
    }

    #[test]
    fn legacy_modelfile_text() {
        let spec = parse_create(&obj(json!({
            "model": "m",
            "modelfile": "FROM qwen3-4b\nPARAMETER temperature 0.2\nSYSTEM \"\"\"Be brief.\nReally.\"\"\"\nMESSAGE user hi\n"
        })))
        .unwrap();
        assert_eq!(spec.from.as_deref(), Some("qwen3-4b"));
        let keys: Vec<&str> = spec.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["parameter", "system", "message"]);
        assert_eq!(spec.items[1].value, "Be brief.\nReally.");
        // Server-side file paths are not accepted over the network.
        let e = parse_create(&obj(json!({
            "model": "m", "modelfile": "FROM x\nADAPTER ./lora.gguf"
        })))
        .unwrap_err();
        assert!(e.message.contains("ADAPTER"));
        // FROM given twice.
        assert!(parse_create(&obj(json!({
            "model": "m", "from": "a", "modelfile": "FROM b"
        })))
        .is_err());
    }

    #[test]
    fn names_become_aliases() {
        assert_eq!(alias_of("mymodel").unwrap(), "mymodel");
        assert_eq!(alias_of("mymodel:latest").unwrap(), "mymodel");
        assert_eq!(alias_of("mymodel:7b").unwrap(), "mymodel:7b");
        assert!(alias_of("hf.co/a/b:Q4_K_M").is_err());
        assert!(alias_of("bad name!").is_err());
        assert!(alias_of("../x").is_err());
    }

    #[test]
    fn items_use_the_modelfile_mapping() {
        // The same mapping the Library import uses, driven by items (no file needed: a missing
        // model file only warns).
        let spec = parse_create(&obj(json!({
            "model": "m", "from": "x",
            "system": "Be brief.",
            "parameters": {"temperature": 0.25, "num_ctx": 2048, "stop": "END", "bogus": 1},
            "messages": [{"role": "user", "content": "ping"}, {"role": "assistant", "content": "pong"}],
            "template": "{{ .Prompt }}"
        })))
        .unwrap();
        let mut items = vec![item("from", "/nonexistent/model.gguf")];
        items.extend(spec.items);
        let m = modelfile::import_items(&items, &ImportOptions::default()).unwrap();
        let preset = m.import.preset.unwrap();
        assert_eq!(preset.sampling.temperature, Some(0.25));
        assert_eq!(preset.sampling.stop, vec!["END".to_string()]);
        let sys = preset.system_prompt.unwrap();
        assert!(sys.starts_with("Be brief."));
        assert!(sys.contains("User: ping") && sys.contains("Assistant: pong"));
        assert_eq!(
            m.import.definition.load.get("context_size"),
            Some(&harness_core::params::ParamValue::Int(2048))
        );
        let codes: Vec<&str> = m.warnings.iter().map(|w| w.code.as_str()).collect();
        for want in [
            "unknown_parameter",
            "messages_as_examples",
            "model_file_missing",
        ] {
            assert!(codes.contains(&want), "{want} in {codes:?}");
        }
        assert!(codes.iter().any(|c| c.starts_with("template_ignored")));
        let texts: Vec<String> = m.warnings.iter().map(warning_text).collect();
        assert!(texts
            .iter()
            .any(|t| t.starts_with("unknown parameter ignored")));
    }

    #[test]
    fn sampling_overlay_keeps_unset_fields() {
        let base = SamplingParams {
            temperature: Some(0.7),
            top_k: Some(40),
            stop: vec!["a".into()],
            ..Default::default()
        };
        let over = SamplingParams {
            temperature: Some(0.1),
            ..Default::default()
        };
        let merged = overlay_sampling(&base, &over);
        assert_eq!(merged.temperature, Some(0.1));
        assert_eq!(merged.top_k, Some(40));
        assert_eq!(merged.stop, vec!["a".to_string()]);
        let over_stop = SamplingParams {
            stop: vec!["z".into()],
            ..Default::default()
        };
        assert_eq!(
            overlay_sampling(&base, &over_stop).stop,
            vec!["z".to_string()]
        );
    }

    #[test]
    fn keyed_errors_read_as_text() {
        let e: String = harness_core::error_key::err("definitions.alias_exists")
            .with("alias", "x")
            .into();
        assert_eq!(keyed_text(&e), "definitions.alias_exists (alias=x)");
        assert_eq!(keyed_text("plain text"), "plain text");
        let g = core_error(e);
        assert_eq!(g.status, 409);
        assert_eq!(core_error("definitions.not_found".into()).status, 404);
    }
}
