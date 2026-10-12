//! Ollama `Modelfile` import.
//!
//! A Modelfile becomes a [`DefinitionImport`] (a [`ModelDefinition`] plus an
//! [`InferencePreset`]) that the existing `Domain::import_definition` saves. Nothing is saved
//! here and Ollama's own directory is only ever read.
//!
//! Spec (docs.ollama.com/modelfile): instructions `FROM`, `PARAMETER`, `TEMPLATE`, `SYSTEM`,
//! `ADAPTER`, `LICENSE`, `MESSAGE`, `REQUIRES`, `CAPABILITY`; case-insensitive; `#` comments;
//! values may be wrapped in `"""` and span lines.
//!
//! Mapping:
//!
//! | Modelfile | Harness |
//! |---|---|
//! | `FROM <file.gguf>` / `FROM a-*.gguf` | definition base (first shard) |
//! | `FROM <name[:tag]>` | installed Ollama model: GGUF blob, projector, adapter layers and its stored params / system are inherited (read-only) |
//! | `PARAMETER temperature top_p top_k min_p typical_p repeat_penalty repeat_last_n presence_penalty frequency_penalty mirostat mirostat_tau mirostat_eta seed` | preset sampling (same names) |
//! | `PARAMETER num_predict` | preset `max_tokens` (`-1` = unlimited = not sent) |
//! | `PARAMETER stop` (repeatable) | preset `stop` |
//! | `PARAMETER num_ctx / num_gpu / num_thread / num_batch / main_gpu` | definition load `context_size / gpu_layers / threads / logical_batch / main_gpu` |
//! | `SYSTEM` | preset system prompt |
//! | `ADAPTER <gguf>` | definition adapter |
//! | `TEMPLATE` | not used (Go template); the GGUF's Jinja template applies |
//! | `MESSAGE <role> <text>` (repeatable; also an installed Ollama model's stored messages) | appended to the preset's system prompt as an example conversation (`User: ...` / `Assistant: ...`); the preset has no turn list, so they are examples inside the system message, not real history turns (warning says so) |
//! | `LICENSE`, `REQUIRES`, `CAPABILITY` | ignored |
//! | any other `PARAMETER` | warning |

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::launch::normalize_definition;
use crate::domain::portable::DefinitionImport;
use crate::domain::{
    sanitize_alias, unique_alias, AdapterRef, InferencePreset, ModelDefinition, ModelRef,
    ReasoningSettings, SamplingParams,
};
use crate::error_key::err;
use crate::importer::{blob_path, is_gguf, ollama_dir};
use crate::params::ParamValue;

// --- syntax ---

/// One instruction of a Modelfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 1-based line of the instruction word.
    pub line: usize,
    /// Lower-cased instruction (`from`, `parameter`, ...).
    pub key: String,
    /// The value: `"""` block or single line, quotes removed.
    pub value: String,
}

/// Splits a Modelfile into instructions. Only syntax errors (an unterminated `"""`) fail.
pub fn parse(text: &str) -> Result<Vec<Item>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i].trim();
        let line_no = i + 1;
        i += 1;
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        let (key, rest) = match raw.split_once(char::is_whitespace) {
            Some((k, r)) => (k, r.trim_start()),
            None => (raw, ""),
        };
        // `MESSAGE <role> """multi-line text"""`: the block starts after the role word.
        let (block_prefix, block_rest) = match rest.split_once(char::is_whitespace) {
            Some((role, tail))
                if key.eq_ignore_ascii_case("message")
                    && tail.trim_start().starts_with("\"\"\"") =>
            {
                (format!("{role} "), tail.trim_start())
            }
            _ => (String::new(), rest),
        };
        let value = if let Some(after) = block_rest.strip_prefix("\"\"\"") {
            let mut buf = String::new();
            let mut cur = after.to_string();
            loop {
                if let Some(end) = cur.find("\"\"\"") {
                    buf.push_str(&cur[..end]);
                    break;
                }
                buf.push_str(&cur);
                if i >= lines.len() {
                    return Err(err("modelfile.unclosed_block")
                        .num("line", line_no)
                        .with("instruction", key.to_uppercase())
                        .into());
                }
                buf.push('\n');
                cur = lines[i].to_string();
                i += 1;
            }
            format!("{block_prefix}{}", buf.trim())
        } else {
            unquote(rest.trim_end()).to_string()
        };
        items.push(Item {
            line: line_no,
            key: key.to_ascii_lowercase(),
            value,
        });
    }
    Ok(items)
}

/// One `MESSAGE` turn: the role (`system`, `user`, `assistant`) and its non-empty text.
fn example_turn(role: &str, text: &str) -> Option<(String, String)> {
    let role = role.trim().to_ascii_lowercase();
    if !matches!(role.as_str(), "system" | "user" | "assistant") {
        return None;
    }
    let text = unquote(text.trim()).trim();
    (!text.is_empty()).then(|| (role, text.to_string()))
}

/// The system prompt followed by the `MESSAGE` examples as one example conversation. The
/// preset has no list of turns to prepend, so the examples travel inside the system message
/// (every consumer of a preset's system prompt applies them).
pub fn with_examples(system: Option<&str>, examples: &[(String, String)]) -> String {
    let mut out = system.map(str::trim).unwrap_or_default().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(
        "Example conversation (from the Modelfile MESSAGE instructions); answer in the same style:",
    );
    for (role, text) in examples {
        let label = match role.as_str() {
            "assistant" => "Assistant",
            "system" => "System",
            _ => "User",
        };
        out.push_str(&format!("\n\n{label}: {text}"));
    }
    out
}

fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

// --- result ---

/// One value the import sets, for the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapped {
    /// What the Modelfile said (`PARAMETER num_ctx 8192`).
    pub source: String,
    /// Where it went (`load.context_size`, `sampling.temperature`, `system_prompt`...).
    pub target: String,
    pub value: String,
}

/// Something that could not be carried over. `code` is stable (the UI translates it);
/// `detail` is the instruction / parameter / value concerned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub code: String,
    pub line: Option<usize>,
    pub detail: String,
}

/// What a Modelfile import found (nothing is saved yet).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelfileImport {
    /// Save it with `Domain::import_definition`.
    pub import: DefinitionImport,
    pub mapped: Vec<Mapped>,
    pub warnings: Vec<Warning>,
    /// `FROM` as written.
    pub from: String,
}

/// Request of the Control API / GUI: a Modelfile's text and where its relative paths start.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelfileRequest {
    pub text: String,
    /// Directory of the Modelfile (relative `FROM` / `ADAPTER` paths).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
    /// Alias to use (default: derived from `FROM`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl ModelfileRequest {
    /// Reads the text; `taken_aliases` are the aliases already in use.
    pub fn preview(&self, taken_aliases: Vec<String>) -> Result<ModelfileImport, String> {
        import_text(
            &self.text,
            &ImportOptions {
                base_dir: self.base_dir.as_deref().map(PathBuf::from),
                alias: self.alias.clone(),
                taken_aliases,
                ..Default::default()
            },
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Relative paths resolve against this (the Modelfile's directory).
    pub base_dir: Option<PathBuf>,
    /// Ollama models directory (default: `OLLAMA_MODELS`, `~/.ollama/models`).
    pub ollama_dir: Option<PathBuf>,
    /// Wanted alias (default: derived from `FROM`).
    pub alias: Option<String>,
    /// Aliases already in use (for the suggestion).
    pub taken_aliases: Vec<String>,
}

// --- Ollama models directory (read-only) ---

/// An installed Ollama model, read from its manifest.
#[derive(Debug, Default)]
struct OllamaModel {
    model: PathBuf,
    mmproj: Option<PathBuf>,
    adapters: Vec<PathBuf>,
    params: Option<serde_json::Map<String, Value>>,
    system: Option<String>,
    has_template: bool,
    /// Stored `MESSAGE` examples (role, text).
    messages: Vec<(String, String)>,
}

/// `qwen3:30b-a3b` -> manifest path components under `manifests/`.
fn manifest_parts(name: &str) -> Result<Vec<String>, String> {
    let name = name.trim();
    let (model, tag) = match name.rsplit_once(':') {
        Some((m, t)) if !t.contains('/') && !t.is_empty() => (m, t),
        _ => (name, "latest"),
    };
    let segs: Vec<&str> = model.split('/').collect();
    let mut parts: Vec<String> = match segs.as_slice() {
        [m] => vec!["registry.ollama.ai".into(), "library".into(), m.to_string()],
        [host, m] if host.contains('.') => {
            vec![host.to_string(), "library".into(), m.to_string()]
        }
        [ns, m] => vec!["registry.ollama.ai".into(), ns.to_string(), m.to_string()],
        all => all.iter().map(|s| s.to_string()).collect(),
    };
    parts.push(tag.to_string());
    if parts
        .iter()
        .any(|p| p.is_empty() || p == "." || p == ".." || p.contains(['\\', ':']))
    {
        return Err(err("modelfile.invalid_ollama_name")
            .with("name", name)
            .into());
    }
    Ok(parts)
}

fn read_ollama_model(dir: &Path, name: &str) -> Result<OllamaModel, String> {
    let parts = manifest_parts(name)?;
    let manifest = parts
        .iter()
        .fold(dir.join("manifests"), |p, seg| p.join(seg));
    let text = std::fs::read_to_string(&manifest).map_err(|_| {
        String::from(
            err("modelfile.ollama_model_missing")
                .with("name", name)
                .with("dir", dir.display()),
        )
    })?;
    let json: Value = serde_json::from_str(&text).map_err(|e| {
        String::from(
            err("json.parse_failed")
                .with("path", manifest.display())
                .with("error", e),
        )
    })?;
    let layers = json["layers"].as_array().cloned().unwrap_or_default();
    let blobs = |media: &str| -> Vec<PathBuf> {
        layers
            .iter()
            .filter(|l| l["mediaType"].as_str() == Some(media))
            .filter_map(|l| blob_path(dir, l["digest"].as_str()?))
            .collect()
    };
    let text_of = |media: &str| {
        blobs(media)
            .first()
            .and_then(|p| std::fs::read_to_string(p).ok())
    };
    let model = blobs("application/vnd.ollama.image.model")
        .into_iter()
        .next()
        .ok_or_else(|| String::from(err("modelfile.no_model_layer").with("name", name)))?;
    if !model.is_file() {
        return Err(err("modelfile.weights_missing")
            .with("name", name)
            .with("path", model.display())
            .into());
    }
    if !is_gguf(&model) {
        return Err(err("modelfile.ollama_not_gguf").with("name", name).into());
    }
    Ok(OllamaModel {
        model,
        mmproj: blobs("application/vnd.ollama.image.projector")
            .into_iter()
            .find(|p| p.is_file() && is_gguf(p)),
        adapters: blobs("application/vnd.ollama.image.adapter")
            .into_iter()
            .filter(|p| p.is_file() && is_gguf(p))
            .collect(),
        params: text_of("application/vnd.ollama.image.params")
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned()),
        system: text_of("application/vnd.ollama.image.system").filter(|s| !s.trim().is_empty()),
        has_template: !blobs("application/vnd.ollama.image.template").is_empty(),
        messages: text_of("application/vnd.ollama.image.messages")
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| {
                v.as_array().map(|a| {
                    a.iter()
                        .filter_map(|m| example_turn(m["role"].as_str()?, m["content"].as_str()?))
                        .collect()
                })
            })
            .unwrap_or_default(),
    })
}

// --- FROM ---

fn expand_home(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        if let Some(h) = home {
            return PathBuf::from(h).join(rest);
        }
    }
    PathBuf::from(s)
}

fn resolve_rel(value: &str, base: Option<&Path>) -> PathBuf {
    let p = expand_home(value);
    match base {
        Some(b) if p.is_relative() => b.join(p),
        _ => p,
    }
}

fn looks_like_path(value: &str, base: Option<&Path>) -> bool {
    let lower = value.to_ascii_lowercase();
    value.starts_with('.')
        || value.starts_with('~')
        || value.contains('\\')
        || value.contains('*')
        || lower.ends_with(".gguf")
        || Path::new(value).is_absolute()
        || value.as_bytes().get(1) == Some(&b':') // C:/...
        || resolve_rel(value, base).exists()
}

fn wildcard(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => {
            name.len() >= head.len()
                && name.starts_with(head)
                && (0..=name.len() - head.len()).any(|i| {
                    name.is_char_boundary(head.len() + i) && wildcard(tail, &name[head.len() + i..])
                })
        }
    }
}

/// `model-*.gguf` -> the first match by name (shard 1 of a split model).
fn expand_glob(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let mut hits: Vec<PathBuf> = std::fs::read_dir(dir.unwrap_or(Path::new(".")))
        .ok()?
        .flatten()
        .filter(|e| wildcard(&name, &e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    hits.sort();
    hits.into_iter().next()
}

// --- values ---

struct Ctx {
    mapped: Vec<Mapped>,
    warnings: Vec<Warning>,
    sampling: SamplingParams,
    load: crate::params::ParamValues,
}

impl Ctx {
    fn warn(&mut self, code: &str, line: Option<usize>, detail: impl Into<String>) {
        self.warnings.push(Warning {
            code: code.into(),
            line,
            detail: detail.into(),
        });
    }

    fn map(&mut self, source: String, target: &str, value: impl ToString) {
        self.mapped.push(Mapped {
            source,
            target: target.into(),
            value: value.to_string(),
        });
    }

    /// One `PARAMETER name value` (own or inherited); `stop` is handled by the caller.
    fn parameter(&mut self, name: &str, raw: &str, line: Option<usize>) {
        let src = format!("PARAMETER {} {}", name, raw);
        let f = |raw: &str| raw.trim().parse::<f64>().ok().filter(|x| x.is_finite());
        let n = |raw: &str| {
            let raw = raw.trim();
            raw.parse::<i64>()
                .ok()
                .or_else(|| f(raw).filter(|x| x.fract() == 0.0).map(|x| x as i64))
        };
        macro_rules! float {
            ($field:ident) => {
                match f(raw) {
                    Some(v) => {
                        self.sampling.$field = Some(v);
                        self.map(src, concat!("sampling.", stringify!($field)), v);
                    }
                    None => self.warn("invalid_value", line, format!("{} {}", name, raw)),
                }
            };
        }
        macro_rules! int {
            ($field:ident) => {
                match n(raw) {
                    Some(v) => {
                        self.sampling.$field = Some(v);
                        self.map(src, concat!("sampling.", stringify!($field)), v);
                    }
                    None => self.warn("invalid_value", line, format!("{} {}", name, raw)),
                }
            };
        }
        match name {
            "temperature" => float!(temperature),
            "top_p" => float!(top_p),
            "min_p" => float!(min_p),
            "typical_p" => float!(typical_p),
            "repeat_penalty" => float!(repeat_penalty),
            "presence_penalty" => float!(presence_penalty),
            "frequency_penalty" => float!(frequency_penalty),
            "mirostat_tau" => float!(mirostat_tau),
            "mirostat_eta" => float!(mirostat_eta),
            "top_k" => int!(top_k),
            "repeat_last_n" => int!(repeat_last_n),
            "seed" => int!(seed),
            "mirostat" => match n(raw) {
                Some(v @ 0..=2) => {
                    self.sampling.mirostat = Some(v);
                    self.map(src, "sampling.mirostat", v);
                }
                _ => self.warn("invalid_value", line, format!("{} {}", name, raw)),
            },
            "num_predict" => match n(raw) {
                Some(v) if v > 0 => {
                    self.sampling.max_tokens = Some(v);
                    self.map(src, "sampling.max_tokens", v);
                }
                // -1 = unlimited: llama-server's default too.
                Some(-1) | Some(0) => {}
                Some(v) => self.warn("ignored_value", line, format!("num_predict {}", v)),
                None => self.warn("invalid_value", line, format!("{} {}", name, raw)),
            },
            "num_ctx" => self.load_int(src, "context_size", name, raw, line, |v| v > 0),
            "num_gpu" => self.load_int(src, "gpu_layers", name, raw, line, |v| v >= 0),
            "num_thread" => self.load_int(src, "threads", name, raw, line, |v| v > 0),
            "num_batch" => self.load_int(src, "logical_batch", name, raw, line, |v| v > 0),
            "main_gpu" => self.load_int(src, "main_gpu", name, raw, line, |v| v >= 0),
            "tfs_z" | "penalize_newline" | "num_keep" | "num_gqa" | "low_vram" | "f16_kv"
            | "logits_all" | "vocab_only" | "use_mmap" | "use_mlock" | "numa"
            | "draft_num_predict" | "num_thread_batch" => {
                self.warn("ignored_parameter", line, format!("{} {}", name, raw))
            }
            _ => self.warn("unknown_parameter", line, format!("{} {}", name, raw)),
        }
    }

    fn load_int(
        &mut self,
        src: String,
        id: &str,
        name: &str,
        raw: &str,
        line: Option<usize>,
        ok: impl Fn(i64) -> bool,
    ) {
        match raw.trim().parse::<i64>() {
            Ok(v) if ok(v) => {
                self.load.insert(id.into(), ParamValue::Int(v));
                self.map(src, &format!("load.{}", id), v);
            }
            // num_gpu -1 / num_thread 0 = "let the engine decide": nothing to set.
            Ok(-1) | Ok(0) if name == "num_gpu" || name == "num_thread" => {}
            _ => self.warn("invalid_value", line, format!("{} {}", name, raw)),
        }
    }
}

fn json_scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

// --- import ---

/// Reads a Modelfile and describes what importing it would create.
pub fn import_text(text: &str, opts: &ImportOptions) -> Result<ModelfileImport, String> {
    import_items(&parse(text)?, opts)
}

/// Same as [`import_text`] for already split instructions (the gateway's `POST /api/create`
/// builds them from its JSON fields, so no value ever needs Modelfile quoting).
pub fn import_items(items: &[Item], opts: &ImportOptions) -> Result<ModelfileImport, String> {
    let from = items
        .iter()
        .rev()
        .find(|i| i.key == "from")
        .ok_or_else(|| String::from(err("modelfile.no_from")))?;
    if from.value.trim().is_empty() {
        return Err(err("modelfile.from_empty").num("line", from.line).into());
    }
    let base_dir = opts.base_dir.as_deref();
    let mut ctx = Ctx {
        mapped: Vec::new(),
        warnings: Vec::new(),
        sampling: SamplingParams::default(),
        load: Default::default(),
    };
    let mut missing: Vec<String> = Vec::new();
    let mut adapters: Vec<AdapterRef> = Vec::new();
    let mut mmproj: Option<PathBuf> = None;
    let mut system: Option<String> = None;
    let mut inherited_stop: Vec<String> = Vec::new();
    // `MESSAGE` examples: the installed model's own first, then the Modelfile's.
    let mut examples: Vec<(String, String)> = Vec::new();
    let mut alias_seed: String;
    let model_path: PathBuf;
    let model_found: bool;
    let mut ollama: Option<OllamaModel> = None;

    let fv = from.value.trim();
    if looks_like_path(fv, base_dir) {
        let mut p = resolve_rel(fv, base_dir);
        if fv.contains('*') {
            p = expand_glob(&p).unwrap_or(p);
        }
        if p.is_dir() {
            return Err(err("modelfile.from_is_dir")
                .with("path", p.display())
                .into());
        }
        model_found = p.is_file();
        if model_found {
            if !is_gguf(&p) {
                return Err(err("modelfile.from_not_gguf")
                    .with("path", p.display())
                    .into());
            }
        } else {
            ctx.warn(
                "model_file_missing",
                Some(from.line),
                p.display().to_string(),
            );
        }
        alias_seed = crate::process::file_name_any_sep(&p)
            .map(|name| match name.rsplit_once('.') {
                Some((stem, _)) if !stem.is_empty() => stem.to_string(),
                _ => name,
            })
            .unwrap_or_else(|| "model".into());
        model_path = p;
    } else {
        let dir = opts
            .ollama_dir
            .clone()
            .or_else(ollama_dir)
            .ok_or_else(|| String::from(err("modelfile.ollama_dir_missing")))?;
        let m = read_ollama_model(&dir, fv)?;
        model_found = true;
        alias_seed = fv.to_string();
        model_path = m.model.clone();
        mmproj = m.mmproj.clone();
        adapters.extend(m.adapters.iter().map(|p| AdapterRef {
            path: p.clone(),
            scale: None,
        }));
        system = m.system.clone();
        if let Some(params) = &m.params {
            for (k, v) in params {
                if k == "stop" {
                    inherited_stop = match v {
                        Value::Array(a) => a.iter().filter_map(json_scalar).collect(),
                        other => json_scalar(other).into_iter().collect(),
                    };
                } else if let Some(s) = json_scalar(v) {
                    ctx.parameter(&k.to_ascii_lowercase(), &s, None);
                }
            }
        }
        examples.extend(m.messages.iter().cloned());
        ollama = Some(m);
    }
    if let Some(m) = &ollama {
        ctx.map(
            format!("FROM {}", fv),
            "base",
            m.model.display().to_string(),
        );
        if let Some(p) = &m.mmproj {
            ctx.map(format!("FROM {}", fv), "mmproj", p.display().to_string());
        }
        if m.system.is_some() {
            ctx.map(format!("FROM {}", fv), "system_prompt", "(model'den)");
        }
        let _ = m.has_template; // Ollama's own template layer: never used (see TEMPLATE)
    } else {
        ctx.map(
            format!("FROM {}", fv),
            "base",
            model_path.display().to_string(),
        );
    }

    let mut own_stop: Vec<String> = Vec::new();
    let mut has_template = false;
    for it in items {
        match it.key.as_str() {
            "from" => {}
            "parameter" => {
                let (name, val) = match it.value.split_once(char::is_whitespace) {
                    Some((n, v)) => (n.trim().to_ascii_lowercase(), unquote(v.trim()).to_string()),
                    None => {
                        ctx.warn(
                            "invalid_value",
                            Some(it.line),
                            format!("PARAMETER {}", it.value),
                        );
                        continue;
                    }
                };
                if name == "stop" {
                    ctx.map(
                        format!("PARAMETER stop {:?}", val),
                        "sampling.stop",
                        format!("{:?}", val),
                    );
                    own_stop.push(val);
                } else {
                    ctx.parameter(&name, &val, Some(it.line));
                }
            }
            "system" => {
                if it.value.trim().is_empty() {
                    continue;
                }
                system = Some(it.value.clone());
                ctx.mapped.retain(|m| m.target != "system_prompt");
                let shown: String = it
                    .value
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(80)
                    .collect();
                ctx.map("SYSTEM".into(), "system_prompt", shown);
            }
            "adapter" => {
                let p = resolve_rel(&it.value, base_dir);
                if p.is_dir() || (p.is_file() && !is_gguf(&p)) {
                    ctx.warn("adapter_unsupported", Some(it.line), it.value.clone());
                    continue;
                }
                if !p.is_file() {
                    missing.push(it.value.clone());
                }
                ctx.map(
                    format!("ADAPTER {}", it.value),
                    "adapters",
                    p.display().to_string(),
                );
                adapters.push(AdapterRef {
                    path: p,
                    scale: None,
                });
            }
            "template" => has_template = true,
            "message" => match it.value.split_once(char::is_whitespace) {
                Some((role, text)) => match example_turn(role, text) {
                    Some(turn) => examples.push(turn),
                    None => ctx.warn(
                        "invalid_value",
                        Some(it.line),
                        format!("MESSAGE {}", it.value.chars().take(60).collect::<String>()),
                    ),
                },
                None => ctx.warn(
                    "invalid_value",
                    Some(it.line),
                    format!("MESSAGE {}", it.value),
                ),
            },
            "license" => {}
            "requires" | "capability" => ctx.warn(
                "ignored_instruction",
                Some(it.line),
                format!("{} {}", it.key.to_uppercase(), it.value),
            ),
            other => ctx.warn("unknown_instruction", Some(it.line), other.to_uppercase()),
        }
    }
    if !examples.is_empty() {
        system = Some(with_examples(system.as_deref(), &examples));
        ctx.map(
            "MESSAGE".into(),
            "system_prompt.examples",
            examples.len().to_string(),
        );
        ctx.warn("messages_as_examples", None, examples.len().to_string());
    }
    if has_template {
        let gguf_has = model_found
            && crate::gguf::read_gguf_metadata(&model_path)
                .map(|m| m.has_chat_template)
                .unwrap_or(true);
        ctx.warn(
            if gguf_has {
                "template_ignored"
            } else {
                "template_ignored_no_jinja"
            },
            None,
            String::new(),
        );
    }
    let stop = if own_stop.is_empty() {
        inherited_stop
    } else {
        own_stop
    };
    if !stop.is_empty() && !ctx.mapped.iter().any(|m| m.target == "sampling.stop") {
        for s in &stop {
            ctx.map("(model)".into(), "sampling.stop", format!("{:?}", s));
        }
    }
    ctx.sampling.stop = stop;

    // Alias: requested, else derived from FROM.
    alias_seed = opts.alias.clone().unwrap_or(alias_seed);
    let alias = sanitize_alias(&alias_seed);
    let taken = &opts.taken_aliases;
    let alias_taken = taken.iter().any(|a| a.eq_ignore_ascii_case(&alias));
    let suggested_alias = unique_alias(&alias, taken.iter().map(|s| s.as_str()));

    let mut def = ModelDefinition::new(
        alias.clone(),
        ModelRef::Local {
            path: model_path.clone(),
        },
    );
    def.description = format!("Modelfile'dan içe aktarıldı (FROM {})", fv);
    def.mmproj = mmproj;
    def.adapters = adapters;
    def.load = ctx.load;
    normalize_definition(&mut def);

    let has_sampling = ctx.sampling != SamplingParams::default();
    let preset = (has_sampling || system.is_some()).then(|| InferencePreset {
        id: String::new(),
        name: alias.clone(),
        builtin: false,
        description: "Modelfile içe aktarma".into(),
        system_prompt: system,
        sampling: ctx.sampling,
        reasoning: ReasoningSettings::default(),
        response_format: None,
        tool_choice: None,
        created_at: 0,
        updated_at: 0,
    });
    if let Some(p) = &preset {
        p.validate()?;
    }
    Ok(ModelfileImport {
        import: DefinitionImport {
            definition: def,
            model_found,
            hf_ref: None,
            missing,
            alias_taken,
            suggested_alias,
            preset,
            chat_template_text: None,
        },
        mapped: ctx.mapped,
        warnings: ctx.warnings,
        from: fv.to_string(),
    })
}

/// Reads the Modelfile at `path` (relative paths inside it start at its directory).
pub fn import_file(path: &Path, opts: &ImportOptions) -> Result<ModelfileImport, String> {
    let bytes = std::fs::read(path).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let mut opts = opts.clone();
    if opts.base_dir.is_none() {
        opts.base_dir = path.parent().map(Path::to_path_buf);
    }
    import_text(&text, &opts)
}

#[cfg(test)]
#[path = "modelfile_tests.rs"]
mod tests;
