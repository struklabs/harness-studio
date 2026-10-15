//! Pure translation between the Ollama API and llama-server's OpenAI-compatible API (no I/O).
//!
//! Field names and shapes follow the official Ollama API reference
//! (`github.com/ollama/ollama/blob/main/docs/api.md`, `docs.ollama.com/api`) and its Go types
//! (`api/types.go`); see `docs/en/ARCHITECTURE.md` §13.4 for the table of decisions.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::gateway::scheduler::KeepAlive;
use serde_json::{json, Map, Value};

// --- names ---

/// How an Ollama `model` name is looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRef {
    /// Definition aliases to try, in order.
    Alias(Vec<String>),
    /// `hf.co/<owner>/<repo>[:<quant>]` (also `huggingface.co/...`): a definition whose GGUF
    /// lives in that repo's folder (and has that quantization), or one to pull.
    Hf { repo: String, quant: Option<String> },
}

/// The Ollama name of a definition alias: aliases that carry a tag (`qwen3:4b`) are used as
/// they are, the others get Ollama's default tag (`qwen3` → `qwen3:latest`).
pub fn ollama_name(alias: &str) -> String {
    if alias.contains(':') {
        alias.to_string()
    } else {
        format!("{}:latest", alias)
    }
}

/// Parses a requested name. Rule (documented in ARCHITECTURE §13.4):
/// 1. the name as sent is an alias (case-insensitive; aliases may contain `:`, so `qwen3:4b`
///    matches a definition named `qwen3:4b`);
/// 2. `name:latest` also matches the alias `name` (Ollama's default tag);
/// 3. `hf.co/<owner>/<repo>[:<quant>]` matches by file location (see [`NameRef::Hf`]).
pub fn parse_name(raw: &str) -> NameRef {
    let name = raw.trim();
    let bare = name
        .strip_prefix("https://")
        .or_else(|| name.strip_prefix("http://"))
        .unwrap_or(name);
    for prefix in ["hf.co/", "huggingface.co/"] {
        if bare.len() > prefix.len() && bare[..prefix.len()].eq_ignore_ascii_case(prefix) {
            let rest = &bare[prefix.len()..];
            let (repo, quant) = match rest.rsplit_once(':') {
                Some((r, q)) => (r, Some(q)),
                None => (rest, None),
            };
            let quant = quant
                .map(str::trim)
                .filter(|q| !q.is_empty() && !q.eq_ignore_ascii_case("latest"))
                .map(str::to_string);
            return NameRef::Hf {
                repo: repo.trim_matches('/').to_string(),
                quant,
            };
        }
    }
    let mut out = vec![name.to_string()];
    if let Some(stripped) = name.strip_suffix(":latest") {
        if !stripped.is_empty() {
            out.push(stripped.to_string());
        }
    }
    NameRef::Alias(out)
}

// --- keep_alive ---

/// Parses Ollama's `keep_alive`: a number of seconds or a Go duration string (`"5m"`,
/// `"1h30m"`, `"0"`, `"-1"`); negative = forever. `None` when absent or `null`.
pub fn parse_keep_alive(v: Option<&Value>) -> Result<Option<KeepAlive>, String> {
    let secs: f64 = match v {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => parse_go_duration(s.trim())
            .ok_or_else(|| format!("invalid keep_alive duration: {:?}", s))?,
        Some(other) => return Err(format!("invalid keep_alive: {}", other)),
    };
    Ok(Some(if secs < 0.0 {
        KeepAlive::Forever
    } else {
        KeepAlive::Secs(secs.ceil() as u64)
    }))
}

/// Go `time.ParseDuration` subset (`ns`, `us`/`µs`, `ms`, `s`, `m`, `h`; sign; fractions),
/// plus a bare number = seconds (lenient, like the numeric JSON form). Returns seconds.
fn parse_go_duration(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    if let Ok(n) = s.parse::<f64>() {
        return Some(n);
    }
    let (neg, mut rest) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if rest.is_empty() {
        return None;
    }
    let mut total = 0.0;
    while !rest.is_empty() {
        let num_end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if num_end == 0 {
            return None;
        }
        let n: f64 = rest[..num_end].parse().ok()?;
        rest = &rest[num_end..];
        let unit_end = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let factor = match &rest[..unit_end] {
            "ns" => 1e-9,
            "us" | "µs" | "μs" => 1e-6,
            "ms" => 1e-3,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            _ => return None,
        };
        total += n * factor;
        rest = &rest[unit_end..];
    }
    Some(if neg { -total } else { total })
}

// --- time ---

/// RFC 3339 UTC timestamp with nanoseconds (`created_at`, `modified_at`, `expires_at`).
pub fn rfc3339(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    rfc3339_parts(d.as_secs() as i64, d.subsec_nanos())
}

pub fn rfc3339_parts(secs: i64, nanos: u32) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        nanos
    )
}

pub fn now_rfc3339() -> String {
    rfc3339(SystemTime::now())
}

/// Ollama's parameter size label (`"4.0B"`, `"137M"`).
pub fn parameter_size(count: u64) -> String {
    let n = count as f64;
    if n >= 1e9 {
        format!("{:.1}B", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.0}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.0}K", n / 1e3)
    } else {
        count.to_string()
    }
}

// --- requests ---

/// What an Ollama request asks the gateway to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Empty `prompt` / `messages`: load the model (`done_reason: "load"`).
    Load,
    /// Empty request with `keep_alive: 0`: unload it (`done_reason: "unload"`).
    Unload,
    /// Forward to llama-server.
    Forward(Upstream),
}

/// A request for llama-server.
#[derive(Debug, Clone, PartialEq)]
pub struct Upstream {
    /// `/v1/chat/completions`, `/v1/completions` or `/v1/embeddings`.
    pub path: &'static str,
    pub body: Value,
    pub stream: bool,
    /// Parts of the request that could not be applied (`X-Harness-Compat`, log, record).
    pub notes: Vec<String>,
    pub has_images: bool,
    /// `/api/embed` `dimensions`: vectors are cut to this length and normalized again.
    pub dimensions: Option<usize>,
}

/// Which Ollama response shape a translation produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `/api/chat`: `message: {role, content, thinking, tool_calls}`.
    Chat,
    /// `/api/generate` over chat completions: `response`, `thinking`.
    Generate,
    /// `/api/generate` with `raw: true` over `/v1/completions`.
    RawGenerate,
}

/// A request error (400, Ollama `{"error": ...}`).
pub type ReqError = String;

/// Sampling options llama-server applies per request: Ollama option → llama-server field.
const SAMPLING: &[(&str, &str)] = &[
    ("temperature", "temperature"),
    ("top_p", "top_p"),
    ("top_k", "top_k"),
    ("min_p", "min_p"),
    ("typical_p", "typical_p"),
    ("seed", "seed"),
    ("stop", "stop"),
    ("presence_penalty", "presence_penalty"),
    ("frequency_penalty", "frequency_penalty"),
    ("repeat_penalty", "repeat_penalty"),
    ("repeat_last_n", "repeat_last_n"),
    ("num_keep", "n_keep"),
    ("mirostat", "mirostat"),
    ("mirostat_tau", "mirostat_tau"),
    ("mirostat_eta", "mirostat_eta"),
];

/// Applies Ollama `options` to an OpenAI body. Load-time options (`num_ctx`, `num_gpu`...)
/// cannot change a running llama-server: they come from the model definition, and are
/// reported as ignored.
fn apply_options(opts: Option<&Value>, out: &mut Map<String, Value>, notes: &mut Vec<String>) {
    let Some(Value::Object(opts)) = opts else {
        return;
    };
    let mut ignored = Vec::new();
    for (k, v) in opts {
        if v.is_null() {
            continue;
        }
        if let Some((_, to)) = SAMPLING.iter().find(|(from, _)| from == k) {
            out.insert((*to).to_string(), v.clone());
        } else if k == "num_predict" {
            // -1 = until the context is full, -2 = fill the context: llama-server's default.
            if v.as_i64().is_some_and(|n| n >= 0) {
                out.insert("max_tokens".into(), v.clone());
            }
        } else {
            ignored.push(k.clone());
        }
    }
    if !ignored.is_empty() {
        ignored.sort();
        notes.push(format!("ignored_options={}", ignored.join(",")));
    }
}

/// `format`: `"json"` → JSON mode, a JSON schema object → structured output.
fn apply_format(format: Option<&Value>, out: &mut Map<String, Value>) -> Result<(), ReqError> {
    match format {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(s)) if s.is_empty() => Ok(()),
        Some(Value::String(s)) if s == "json" => {
            out.insert("response_format".into(), json!({"type": "json_object"}));
            Ok(())
        }
        Some(Value::Object(schema)) => {
            out.insert(
                "response_format".into(),
                json!({"type": "json_schema", "json_schema": {
                    "name": "response", "schema": Value::Object(schema.clone()), "strict": true
                }}),
            );
            Ok(())
        }
        Some(other) => Err(format!(
            "invalid format: {} (use \"json\" or a JSON schema object)",
            other
        )),
    }
}

/// Validate `think` here; apply the shared model-aware table after the scheduler lease.
fn apply_think(think: Option<&Value>, out: &mut Map<String, Value>) -> Result<(), ReqError> {
    match think {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Bool(_)) => {}
        Some(Value::String(level))
            if harness_core::reasoning::ThinkingLevel::from_effort(&level.to_ascii_lowercase())
                .is_some() => {}
        Some(other) => return Err(format!("invalid think value: {}", other)),
    }
    let value = think.expect("validated think");
    out.insert(
        "think".into(),
        match value {
            Value::String(s) => json!(s.to_ascii_lowercase()),
            v => v.clone(),
        },
    );
    Ok(())
}

/// MIME type of a base64 image from its first bytes (llama-server decodes the data itself).
pub(crate) fn image_mime(b64: &str) -> &'static str {
    let head = b64.get(..16).unwrap_or(b64);
    if head.starts_with("iVBOR") {
        "image/png"
    } else if head.starts_with("/9j/") {
        "image/jpeg"
    } else if head.starts_with("R0lG") {
        "image/gif"
    } else if head.starts_with("UklG") {
        "image/webp"
    } else if head.starts_with("Qk") {
        "image/bmp"
    } else {
        "image/png"
    }
}

fn image_parts(images: Option<&Value>) -> Result<Vec<Value>, ReqError> {
    let Some(images) = images else {
        return Ok(Vec::new());
    };
    let Value::Array(list) = images else {
        if images.is_null() {
            return Ok(Vec::new());
        }
        return Err("images must be a list of base64 strings".into());
    };
    list.iter()
        .map(|i| {
            let b64 = i
                .as_str()
                .ok_or("images must be a list of base64 strings")?
                .trim();
            // A data URL is accepted as sent.
            let url = if b64.starts_with("data:") {
                b64.to_string()
            } else {
                format!("data:{};base64,{}", image_mime(b64), b64)
            };
            Ok(json!({"type": "image_url", "image_url": {"url": url}}))
        })
        .collect()
}

fn user_content(text: &str, images: Vec<Value>) -> Value {
    if images.is_empty() {
        return Value::String(text.to_string());
    }
    let mut parts = Vec::with_capacity(images.len() + 1);
    if !text.is_empty() {
        parts.push(json!({"type": "text", "text": text}));
    }
    parts.extend(images);
    Value::Array(parts)
}

/// Ollama messages → OpenAI messages. Tool calls get ids (`call_<n>`) when the client sent
/// none; a `tool` message without `tool_call_id` is paired with the earliest unanswered call
/// of the same `tool_name` (else the earliest unanswered call).
fn messages(list: &[Value]) -> Result<(Vec<Value>, bool), ReqError> {
    let mut out = Vec::with_capacity(list.len());
    let mut pending: Vec<(String, String)> = Vec::new(); // (id, name) of unanswered calls
    let mut next_id = 0usize;
    let mut has_images = false;
    for (i, m) in list.iter().enumerate() {
        let Some(obj) = m.as_object() else {
            return Err(format!("messages[{}] is not an object", i));
        };
        let role = obj
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("user")
            .to_ascii_lowercase();
        let content = obj.get("content").and_then(|c| c.as_str()).unwrap_or("");
        let images = image_parts(obj.get("images"))?;
        has_images |= !images.is_empty();
        let mut msg = Map::new();
        msg.insert("role".into(), Value::String(role.clone()));
        match role.as_str() {
            "assistant" => {
                msg.insert("content".into(), Value::String(content.to_string()));
                if let Some(t) = obj
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .filter(|t| !t.is_empty())
                {
                    msg.insert("reasoning_content".into(), Value::String(t.to_string()));
                }
                if let Some(Value::Array(calls)) = obj.get("tool_calls") {
                    let mut oai = Vec::with_capacity(calls.len());
                    for c in calls {
                        let f = c.get("function").unwrap_or(&Value::Null);
                        let name = f.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let args = match f.get("arguments") {
                            Some(Value::String(s)) => s.clone(),
                            Some(v) if !v.is_null() => v.to_string(),
                            _ => "{}".to_string(),
                        };
                        let id = c
                            .get("id")
                            .and_then(|i| i.as_str())
                            .filter(|i| !i.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| {
                                next_id += 1;
                                format!("call_{}", next_id - 1)
                            });
                        pending.push((id.clone(), name.to_string()));
                        oai.push(json!({"id": id, "type": "function",
                            "function": {"name": name, "arguments": args}}));
                    }
                    if !oai.is_empty() {
                        msg.insert("tool_calls".into(), Value::Array(oai));
                    }
                }
            }
            "tool" => {
                msg.insert("content".into(), Value::String(content.to_string()));
                let explicit = obj
                    .get("tool_call_id")
                    .and_then(|i| i.as_str())
                    .filter(|i| !i.is_empty());
                let name = obj.get("tool_name").and_then(|n| n.as_str());
                let id = if let Some(id) = explicit {
                    pending.retain(|(p, _)| p != id);
                    Some(id.to_string())
                } else {
                    let pos = name
                        .and_then(|n| pending.iter().position(|(_, pn)| pn == n))
                        .or(if pending.is_empty() { None } else { Some(0) });
                    pos.map(|p| pending.remove(p).0)
                };
                if let Some(id) = id {
                    msg.insert("tool_call_id".into(), Value::String(id));
                }
            }
            _ => {
                msg.insert("content".into(), user_content(content, images));
            }
        }
        out.push(Value::Object(msg));
    }
    Ok((out, has_images))
}

fn stream_flag(req: &Map<String, Value>) -> bool {
    // Ollama streams unless `"stream": false`.
    req.get("stream").and_then(|s| s.as_bool()).unwrap_or(true)
}

fn keep_alive_zero(req: &Map<String, Value>) -> bool {
    matches!(
        parse_keep_alive(req.get("keep_alive")),
        Ok(Some(KeepAlive::Secs(0)))
    )
}

fn base(model: &str, stream: bool) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("model".into(), Value::String(model.to_string()));
    out.insert("stream".into(), Value::Bool(stream));
    if stream {
        out.insert("stream_options".into(), json!({"include_usage": true}));
    }
    out
}

/// `/api/chat` → `/v1/chat/completions` (or load / unload).
pub fn chat_request(req: &Map<String, Value>, upstream_model: &str) -> Result<Action, ReqError> {
    let list = match req.get("messages") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err("messages must be a list".into()),
    };
    if list.is_empty() {
        return Ok(if keep_alive_zero(req) {
            Action::Unload
        } else {
            Action::Load
        });
    }
    let stream = stream_flag(req);
    let mut out = base(upstream_model, stream);
    let (msgs, has_images) = messages(&list)?;
    out.insert("messages".into(), Value::Array(msgs));
    if let Some(Value::Array(tools)) = req.get("tools") {
        if !tools.is_empty() {
            out.insert("tools".into(), Value::Array(tools.clone()));
        }
    }
    let mut notes = Vec::new();
    apply_format(req.get("format"), &mut out)?;
    apply_think(req.get("think"), &mut out)?;
    apply_options(req.get("options"), &mut out, &mut notes);
    unsupported_fields(req, &["logprobs", "top_logprobs"], &mut notes);
    Ok(Action::Forward(Upstream {
        path: "/v1/chat/completions",
        body: Value::Object(out),
        stream,
        notes,
        has_images,
        dimensions: None,
    }))
}

/// Request fields that are accepted but have no effect here (reported, never silent).
fn unsupported_fields(req: &Map<String, Value>, keys: &[&str], notes: &mut Vec<String>) {
    let set: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|k| {
            req.get(*k)
                .is_some_and(|v| !v.is_null() && v != &Value::Bool(false) && v != &json!(0))
        })
        .collect();
    if !set.is_empty() {
        notes.push(format!("ignored={}", set.join(",")));
    }
}

/// `/api/generate` → `/v1/chat/completions` (`system` + `prompt` + `images`), or
/// `/v1/completions` with `raw: true` (the prompt is sent as is), or load / unload.
pub fn generate_request(
    req: &Map<String, Value>,
    upstream_model: &str,
) -> Result<(Action, Shape), ReqError> {
    let prompt = match req.get("prompt") {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err("prompt must be a string".into()),
    };
    let images = image_parts(req.get("images"))?;
    if prompt.is_empty() && images.is_empty() {
        let action = if keep_alive_zero(req) {
            Action::Unload
        } else {
            Action::Load
        };
        return Ok((action, Shape::Generate));
    }
    if req
        .get("suffix")
        .and_then(|s| s.as_str())
        .is_some_and(|s| !s.is_empty())
    {
        return Err("suffix (fill-in-the-middle) is not supported by Harness's Ollama-compatible API; use llama-server's /infill on the model's direct port".into());
    }
    if req
        .get("context")
        .and_then(|c| c.as_array())
        .is_some_and(|c| !c.is_empty())
    {
        return Err("context (deprecated token memory of /api/generate) is not supported; send the conversation with /api/chat".into());
    }
    let stream = stream_flag(req);
    let mut out = base(upstream_model, stream);
    let mut notes = Vec::new();
    let raw = req.get("raw").and_then(|r| r.as_bool()).unwrap_or(false);
    let has_images = !images.is_empty();
    let shape = if raw {
        if has_images {
            return Err("images cannot be combined with raw: true".into());
        }
        out.insert("prompt".into(), Value::String(prompt.to_string()));
        Shape::RawGenerate
    } else {
        let mut msgs = Vec::new();
        if let Some(system) = req
            .get("system")
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
        {
            msgs.push(json!({"role": "system", "content": system}));
        }
        msgs.push(json!({"role": "user", "content": user_content(prompt, images)}));
        out.insert("messages".into(), Value::Array(msgs));
        apply_think(req.get("think"), &mut out)?;
        Shape::Generate
    };
    apply_format(req.get("format"), &mut out)?;
    apply_options(req.get("options"), &mut out, &mut notes);
    if req
        .get("template")
        .and_then(|t| t.as_str())
        .is_some_and(|t| !t.is_empty())
    {
        // Go templates cannot drive llama-server; the GGUF's Jinja template is used.
        notes.push("ignored=template".into());
    }
    unsupported_fields(req, &["logprobs", "top_logprobs"], &mut notes);
    Ok((
        Action::Forward(Upstream {
            path: if raw {
                "/v1/completions"
            } else {
                "/v1/chat/completions"
            },
            body: Value::Object(out),
            stream,
            notes,
            has_images,
            dimensions: None,
        }),
        shape,
    ))
}

/// `/api/embed` (`input`: string or list) and the legacy `/api/embeddings` (`prompt`) →
/// `/v1/embeddings`. The legacy endpoint returns llama-server's raw (not normalized) vector
/// (`embd_normalize: -1`), like Ollama's; `/api/embed` returns L2-normalized vectors.
pub fn embed_request(
    req: &Map<String, Value>,
    upstream_model: &str,
    legacy: bool,
) -> Result<Upstream, ReqError> {
    let input = if legacy {
        match req.get("prompt") {
            Some(Value::String(s)) => Value::String(s.clone()),
            None | Some(Value::Null) => Value::String(String::new()),
            Some(_) => return Err("prompt must be a string".into()),
        }
    } else {
        match req.get("input") {
            Some(Value::String(s)) => Value::Array(vec![Value::String(s.clone())]),
            Some(Value::Array(a)) if a.iter().all(|x| x.is_string()) => Value::Array(a.clone()),
            None | Some(Value::Null) => Value::Array(Vec::new()),
            Some(_) => return Err("input must be a string or a list of strings".into()),
        }
    };
    let mut out = Map::new();
    out.insert("model".into(), Value::String(upstream_model.to_string()));
    out.insert("input".into(), input);
    out.insert("encoding_format".into(), json!("float"));
    if legacy {
        out.insert("embd_normalize".into(), json!(-1));
    }
    let mut notes = Vec::new();
    apply_options(req.get("options"), &mut Map::new(), &mut notes);
    Ok(Upstream {
        path: "/v1/embeddings",
        body: Value::Object(out),
        stream: false,
        notes,
        has_images: false,
        dimensions: req
            .get("dimensions")
            .and_then(|d| d.as_u64())
            .filter(|d| *d > 0)
            .map(|d| d as usize),
    })
}

/// Whether an embed request has nothing to embed (then it only loads / unloads).
pub fn embed_is_empty(up: &Upstream) -> bool {
    match up.body.get("input") {
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::String(s)) => s.is_empty(),
        _ => true,
    }
}

// --- responses ---

/// Totals the gateway measured for the final object.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    /// Request arrival → end, ns.
    pub total_ns: u64,
    /// JIT load of this request, ns (0 = already loaded).
    pub load_ns: u64,
}

fn done_reason(finish: Option<&str>) -> &str {
    match finish {
        // Ollama reports finished tool calls as "stop".
        Some("tool_calls") | Some("function_call") | None => "stop",
        Some(other) => other,
    }
}

fn ms_to_ns(v: Option<&Value>) -> u64 {
    v.and_then(|x| x.as_f64())
        .map(|ms| (ms * 1_000_000.0).round().max(0.0) as u64)
        .unwrap_or(0)
}

fn u64_of(v: Option<&Value>) -> Option<u64> {
    v.and_then(|x| x.as_u64())
}

/// Counts and durations of the final object from llama-server's `usage` and `timings`.
fn metrics(out: &mut Map<String, Value>, usage: Option<&Value>, timings: Option<&Value>, s: Stats) {
    let t = |k: &str| timings.and_then(|t| t.get(k));
    let u = |k: &str| usage.and_then(|u| u.get(k));
    let cached = u64_of(t("cache_n")).or_else(|| {
        usage
            .and_then(|u| u.get("prompt_tokens_details"))
            .and_then(|d| u64_of(d.get("cached_tokens")))
    });
    let prompt = u64_of(u("prompt_tokens"))
        .or_else(|| u64_of(t("prompt_n")).map(|n| n + cached.unwrap_or(0)));
    let eval = u64_of(u("completion_tokens")).or_else(|| u64_of(t("predicted_n")));
    out.insert("total_duration".into(), json!(s.total_ns));
    out.insert("load_duration".into(), json!(s.load_ns));
    out.insert("prompt_eval_count".into(), json!(prompt.unwrap_or(0)));
    if let Some(c) = cached.filter(|c| *c > 0) {
        out.insert("prompt_eval_cached_count".into(), json!(c));
    }
    out.insert(
        "prompt_eval_duration".into(),
        json!(ms_to_ns(t("prompt_ms"))),
    );
    out.insert("eval_count".into(), json!(eval.unwrap_or(0)));
    out.insert("eval_duration".into(), json!(ms_to_ns(t("predicted_ms"))));
}

/// Ollama tool calls from OpenAI ones (`arguments` JSON text → object).
fn tool_calls_of(calls: &[(Option<String>, String, String)]) -> Value {
    Value::Array(
        calls
            .iter()
            .enumerate()
            .map(|(i, (id, name, args))| {
                let arguments: Value = serde_json::from_str(args)
                    .ok()
                    .filter(|v: &Value| v.is_object())
                    .unwrap_or_else(|| {
                        if args.trim().is_empty() {
                            json!({})
                        } else {
                            // Not a JSON object: keep the text rather than dropping it.
                            json!({ "__raw": args })
                        }
                    });
                let mut call = Map::new();
                if let Some(id) = id {
                    call.insert("id".into(), json!(id));
                }
                call.insert(
                    "function".into(),
                    json!({"index": i, "name": name, "arguments": arguments}),
                );
                Value::Object(call)
            })
            .collect(),
    )
}

/// One Ollama object: message (chat) or response (generate) part.
fn object(
    shape: Shape,
    model: &str,
    content: &str,
    thinking: Option<&str>,
    tool_calls: Option<Value>,
    done: bool,
) -> Map<String, Value> {
    let mut o = Map::new();
    o.insert("model".into(), json!(model));
    o.insert("created_at".into(), json!(now_rfc3339()));
    match shape {
        Shape::Chat => {
            let mut m = Map::new();
            m.insert("role".into(), json!("assistant"));
            m.insert("content".into(), json!(content));
            if let Some(t) = thinking.filter(|t| !t.is_empty()) {
                m.insert("thinking".into(), json!(t));
            }
            if let Some(tc) = tool_calls {
                m.insert("tool_calls".into(), tc);
            }
            o.insert("message".into(), Value::Object(m));
        }
        Shape::Generate | Shape::RawGenerate => {
            o.insert("response".into(), json!(content));
            if let Some(t) = thinking.filter(|t| !t.is_empty()) {
                o.insert("thinking".into(), json!(t));
            }
        }
    }
    o.insert("done".into(), json!(done));
    o
}

/// The single object of a load / unload request.
pub fn lifecycle_object(shape: Shape, model: &str, reason: &str) -> Value {
    let mut o = object(shape, model, "", None, None, true);
    o.insert("done_reason".into(), json!(reason));
    Value::Object(o)
}

/// Non-streaming llama-server response → the Ollama object.
pub fn from_response(shape: Shape, model: &str, v: &Value, stats: Stats) -> Value {
    let choice = v
        .get("choices")
        .and_then(|c| c.get(0))
        .cloned()
        .unwrap_or(Value::Null);
    let finish = choice.get("finish_reason").and_then(|f| f.as_str());
    let (content, thinking, calls) = if shape == Shape::RawGenerate {
        (
            choice
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
            None,
            Vec::new(),
        )
    } else {
        let m = choice.get("message").unwrap_or(&Value::Null);
        let calls: Vec<(Option<String>, String, String)> = m
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .map(|a| {
                a.iter()
                    .map(|c| {
                        let f = c.get("function").unwrap_or(&Value::Null);
                        (
                            c.get("id").and_then(|i| i.as_str()).map(str::to_string),
                            f.get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string(),
                            f.get("arguments")
                                .and_then(|a| a.as_str())
                                .unwrap_or("")
                                .to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        (
            m.get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            m.get("reasoning_content")
                .and_then(|c| c.as_str())
                .map(str::to_string),
            calls,
        )
    };
    let tool_calls = (!calls.is_empty() && shape == Shape::Chat).then(|| tool_calls_of(&calls));
    let mut o = object(
        shape,
        model,
        &content,
        thinking.as_deref(),
        tool_calls,
        true,
    );
    o.insert("done_reason".into(), json!(done_reason(finish)));
    metrics(&mut o, v.get("usage"), v.get("timings"), stats);
    Value::Object(o)
}

/// Error text of a llama-server error body (OpenAI `{"error": {"message"}}` or plain text).
pub fn upstream_error_message(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    match serde_json::from_slice::<Value>(body) {
        Ok(v) => v
            .get("error")
            .and_then(|e| {
                e.get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
                    .or_else(|| e.as_str().map(str::to_string))
            })
            .or_else(|| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.trim().to_string()),
        Err(_) => text.trim().to_string(),
    }
}

/// `/v1/embeddings` response → `/api/embed` (or legacy `/api/embeddings`) object.
/// `dimensions` truncates each vector and normalizes it again (Matryoshka models).
pub fn embed_response(
    model: &str,
    v: &Value,
    legacy: bool,
    dimensions: Option<usize>,
    stats: Stats,
) -> Value {
    let mut rows: Vec<(u64, Vec<f64>)> = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(i, d)| {
                    let idx = d.get("index").and_then(|x| x.as_u64()).unwrap_or(i as u64);
                    let vec = d
                        .get("embedding")
                        .and_then(|e| e.as_array())
                        .map(|e| e.iter().filter_map(|x| x.as_f64()).collect())
                        .unwrap_or_default();
                    (idx, vec)
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by_key(|(i, _)| *i);
    let mut vecs: Vec<Vec<f64>> = rows.into_iter().map(|(_, v)| v).collect();
    if let Some(n) = dimensions.filter(|n| *n > 0) {
        for vec in &mut vecs {
            if vec.len() > n {
                vec.truncate(n);
                let norm = vec.iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm > 0.0 {
                    vec.iter_mut().for_each(|x| *x /= norm);
                }
            }
        }
    }
    if legacy {
        return json!({"embedding": vecs.into_iter().next().unwrap_or_default()});
    }
    json!({
        "model": model,
        "embeddings": vecs,
        "total_duration": stats.total_ns,
        "load_duration": stats.load_ns,
        "prompt_eval_count": v.get("usage").and_then(|u| u.get("prompt_tokens")).and_then(|p| p.as_u64()).unwrap_or(0),
    })
}

/// One NDJSON line.
pub fn line(v: &Value) -> Vec<u8> {
    let mut out = serde_json::to_vec(v).unwrap_or_default();
    out.push(b'\n');
    out
}

/// Streaming translation: llama-server SSE (chat or completions chunks) → Ollama NDJSON.
///
/// Content and thinking deltas become one object each; tool-call fragments are collected
/// and sent as one object with complete `tool_calls` when the choice finishes (Ollama sends
/// whole calls); the final `done: true` object carries `done_reason`, counts and durations
/// (`usage` and `timings` of llama-server's last chunks).
#[derive(Debug)]
pub struct StreamTranslator {
    shape: Shape,
    model: String,
    buf: Vec<u8>,
    tools: BTreeMap<u64, (Option<String>, String, String)>,
    finish: Option<String>,
    usage: Option<Value>,
    timings: Option<Value>,
    errored: bool,
    done: bool,
}

impl StreamTranslator {
    pub fn new(shape: Shape, model: &str) -> Self {
        Self {
            shape,
            model: model.to_string(),
            buf: Vec::new(),
            tools: BTreeMap::new(),
            finish: None,
            usage: None,
            timings: None,
            errored: false,
            done: false,
        }
    }

    /// The upstream sent an error object (the final line is then not sent).
    pub fn errored(&self) -> bool {
        self.errored
    }

    /// NDJSON lines for this chunk (complete SSE events only).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some((end, sep)) = event_end(&self.buf) {
            let event: Vec<u8> = self.buf.drain(..end + sep).collect();
            out.extend(self.event(&event[..end]));
        }
        out
    }

    /// The rest of the stream and the final `done: true` object.
    pub fn finish(&mut self, stats: Stats) -> Vec<u8> {
        let rest = std::mem::take(&mut self.buf);
        let mut out = self.event(&rest);
        if self.errored || self.done {
            return out;
        }
        self.done = true;
        out.extend(self.flush_tools());
        let mut o = object(self.shape, &self.model, "", None, None, true);
        o.insert(
            "done_reason".into(),
            json!(done_reason(self.finish.as_deref())),
        );
        metrics(&mut o, self.usage.as_ref(), self.timings.as_ref(), stats);
        out.extend(line(&Value::Object(o)));
        out
    }

    fn flush_tools(&mut self) -> Vec<u8> {
        if self.tools.is_empty() || self.shape != Shape::Chat {
            return Vec::new();
        }
        let calls: Vec<_> = std::mem::take(&mut self.tools).into_values().collect();
        let o = object(
            self.shape,
            &self.model,
            "",
            None,
            Some(tool_calls_of(&calls)),
            false,
        );
        line(&Value::Object(o))
    }

    fn event(&mut self, event: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if self.errored {
            return out;
        }
        let text = String::from_utf8_lossy(event);
        let mut is_error_event = false;
        for l in text.lines() {
            if let Some(ev) = l.strip_prefix("event:") {
                is_error_event = ev.trim() == "error";
                continue;
            }
            let Some(data) = l.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if is_error_event || v.get("error").is_some() {
                self.errored = true;
                let msg = upstream_error_message(data.as_bytes());
                out.extend(line(&json!({ "error": msg })));
                return out;
            }
            out.extend(self.chunk(&v));
        }
        out
    }

    fn chunk(&mut self, v: &Value) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
            self.usage = Some(u.clone());
        }
        if let Some(t) = v.get("timings").filter(|t| !t.is_null()) {
            self.timings = Some(t.clone());
        }
        let Some(choice) = v.get("choices").and_then(|c| c.get(0)) else {
            return out;
        };
        let (content, thinking) = if self.shape == Shape::RawGenerate {
            (choice.get("text").and_then(|t| t.as_str()), None)
        } else {
            let d = choice.get("delta").unwrap_or(&Value::Null);
            if let Some(Value::Array(calls)) = d.get("tool_calls") {
                for c in calls {
                    let idx = c.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    let e = self.tools.entry(idx).or_default();
                    if let Some(id) = c.get("id").and_then(|i| i.as_str()) {
                        e.0 = Some(id.to_string());
                    }
                    if let Some(f) = c.get("function") {
                        if let Some(n) = f.get("name").and_then(|n| n.as_str()) {
                            e.1.push_str(n);
                        }
                        if let Some(a) = f.get("arguments").and_then(|a| a.as_str()) {
                            e.2.push_str(a);
                        }
                    }
                }
            }
            (
                d.get("content").and_then(|c| c.as_str()),
                d.get("reasoning_content").and_then(|c| c.as_str()),
            )
        };
        let content = content.unwrap_or("");
        let thinking = thinking.unwrap_or("");
        if !content.is_empty() || !thinking.is_empty() {
            let o = object(
                self.shape,
                &self.model,
                content,
                Some(thinking),
                None,
                false,
            );
            out.extend(line(&Value::Object(o)));
        }
        if let Some(f) = choice.get("finish_reason").and_then(|f| f.as_str()) {
            self.finish = Some(f.to_string());
            out.extend(self.flush_tools());
        }
        out
    }
}

/// End of the first complete SSE event and the separator length.
fn event_end(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|p| (p, 2));
    let crlf = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| (p, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn names() {
        assert_eq!(ollama_name("qwen3"), "qwen3:latest");
        assert_eq!(ollama_name("qwen3:4b"), "qwen3:4b");
        assert_eq!(
            parse_name("qwen3:latest"),
            NameRef::Alias(vec!["qwen3:latest".into(), "qwen3".into()])
        );
        assert_eq!(
            parse_name("qwen3:4b"),
            NameRef::Alias(vec!["qwen3:4b".into()])
        );
        assert_eq!(
            parse_name("hf.co/unsloth/Qwen3-4B-GGUF:Q4_K_M"),
            NameRef::Hf {
                repo: "unsloth/Qwen3-4B-GGUF".into(),
                quant: Some("Q4_K_M".into())
            }
        );
        assert_eq!(
            parse_name("https://huggingface.co/bartowski/x-GGUF:latest"),
            NameRef::Hf {
                repo: "bartowski/x-GGUF".into(),
                quant: None
            }
        );
    }

    #[test]
    fn keep_alive() {
        let p = |v: Value| parse_keep_alive(Some(&v)).unwrap();
        assert_eq!(parse_keep_alive(None).unwrap(), None);
        assert_eq!(p(json!(0)), Some(KeepAlive::Secs(0)));
        assert_eq!(p(json!("0")), Some(KeepAlive::Secs(0)));
        assert_eq!(p(json!(-1)), Some(KeepAlive::Forever));
        assert_eq!(p(json!("-1m")), Some(KeepAlive::Forever));
        assert_eq!(p(json!("5m")), Some(KeepAlive::Secs(300)));
        assert_eq!(p(json!("1h30m")), Some(KeepAlive::Secs(5400)));
        assert_eq!(p(json!("1.5s")), Some(KeepAlive::Secs(2)));
        assert_eq!(p(json!("300ms")), Some(KeepAlive::Secs(1)));
        assert_eq!(p(json!(120)), Some(KeepAlive::Secs(120)));
        assert!(parse_keep_alive(Some(&json!("5 minutes"))).is_err());
        assert!(parse_keep_alive(Some(&json!(true))).is_err());
    }

    #[test]
    fn time_format() {
        assert_eq!(rfc3339_parts(0, 0), "1970-01-01T00:00:00.000000000Z");
        assert_eq!(
            rfc3339_parts(1_733_446_089, 983_619_000),
            "2024-12-06T00:48:09.983619000Z"
        );
        assert_eq!(
            rfc3339_parts(951_782_400, 0),
            "2000-02-29T00:00:00.000000000Z"
        );
        assert_eq!(parameter_size(4_022_468_096), "4.0B");
        assert_eq!(parameter_size(136_731_648), "137M");
    }

    #[test]
    fn chat_request_maps_messages_tools_images_options() {
        let req = obj(json!({
            "model": "qwen3",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "user", "content": "what is this?", "images": ["iVBORw0KGgoAAAANSUhEUg"]},
                {"role": "assistant", "content": "", "thinking": "hmm",
                 "tool_calls": [{"function": {"name": "get_weather", "arguments": {"city": "Paris"}}},
                                {"function": {"name": "get_time", "arguments": {"tz": "CET"}}}]},
                {"role": "tool", "content": "10:00", "tool_name": "get_time"},
                {"role": "tool", "content": "11 C", "tool_name": "get_weather"}
            ],
            "tools": [{"type": "function", "function": {"name": "get_weather", "parameters": {"type": "object"}}}],
            "format": {"type": "object", "properties": {"a": {"type": "string"}}},
            "options": {"temperature": 0.2, "num_predict": 64, "num_ctx": 8192, "top_k": 20, "stop": ["\n"], "num_gpu": 99},
            "think": "high",
            "keep_alive": "10m"
        }));
        let Action::Forward(up) = chat_request(&req, "qwen3").unwrap() else {
            panic!()
        };
        assert!(up.stream, "Ollama streams by default");
        assert!(up.has_images);
        let b = &up.body;
        assert_eq!(b["stream_options"]["include_usage"], true);
        assert_eq!(b["messages"][1]["content"][0]["text"], "what is this?");
        assert_eq!(
            b["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg"
        );
        let calls = &b["messages"][2]["tool_calls"];
        assert_eq!(calls[0]["id"], "call_0");
        assert_eq!(calls[0]["function"]["arguments"], "{\"city\":\"Paris\"}");
        assert_eq!(b["messages"][2]["reasoning_content"], "hmm");
        assert_eq!(
            b["messages"][3]["tool_call_id"], "call_1",
            "paired by tool_name"
        );
        assert_eq!(b["messages"][4]["tool_call_id"], "call_0");
        assert_eq!(b["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(b["response_format"]["type"], "json_schema");
        assert_eq!(b["temperature"], 0.2);
        assert_eq!(b["max_tokens"], 64);
        assert_eq!(b["top_k"], 20);
        assert_eq!(b["think"], "high"); // translated once the served model is known
        assert!(b.get("num_ctx").is_none());
        assert_eq!(up.notes, ["ignored_options=num_ctx,num_gpu"]);
    }

    #[test]
    fn load_unload_and_generate() {
        let r = obj(json!({"model": "m", "messages": []}));
        assert_eq!(chat_request(&r, "m").unwrap(), Action::Load);
        let r = obj(json!({"model": "m", "messages": [], "keep_alive": 0}));
        assert_eq!(chat_request(&r, "m").unwrap(), Action::Unload);
        let r = obj(json!({"model": "m", "keep_alive": "0s"}));
        assert_eq!(generate_request(&r, "m").unwrap().0, Action::Unload);

        let r = obj(
            json!({"model": "m", "prompt": "hi", "system": "sys", "stream": false,
            "format": "json", "template": "{{ .Prompt }}"}),
        );
        let (Action::Forward(up), shape) = generate_request(&r, "m").unwrap() else {
            panic!()
        };
        assert_eq!(shape, Shape::Generate);
        assert!(!up.stream);
        assert_eq!(up.path, "/v1/chat/completions");
        assert_eq!(up.body["messages"][0]["role"], "system");
        assert_eq!(up.body["messages"][1]["content"], "hi");
        assert_eq!(up.body["response_format"]["type"], "json_object");
        assert_eq!(up.notes, ["ignored=template"]);

        let r = obj(json!({"model": "m", "prompt": "<|im_start|>", "raw": true}));
        let (Action::Forward(up), shape) = generate_request(&r, "m").unwrap() else {
            panic!()
        };
        assert_eq!((shape, up.path), (Shape::RawGenerate, "/v1/completions"));
        assert_eq!(up.body["prompt"], "<|im_start|>");

        let r = obj(json!({"model": "m", "prompt": "a", "suffix": "b"}));
        assert!(generate_request(&r, "m").unwrap_err().contains("suffix"));
    }

    #[test]
    fn non_stream_response() {
        let v = json!({"choices": [{"finish_reason": "tool_calls", "message": {"role": "assistant",
            "content": "", "reasoning_content": "think", "tool_calls": [{"id": "c1", "type": "function",
            "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}}]}}],
            "usage": {"prompt_tokens": 169, "completion_tokens": 15, "prompt_tokens_details": {"cached_tokens": 0}},
            "timings": {"cache_n": 0, "prompt_n": 169, "prompt_ms": 24.5, "predicted_n": 15, "predicted_ms": 115.9}});
        let o = from_response(
            Shape::Chat,
            "qwen3:latest",
            &v,
            Stats {
                total_ns: 182_000_000,
                load_ns: 41_000_000,
            },
        );
        assert_eq!(o["done"], true);
        assert_eq!(o["done_reason"], "stop");
        assert_eq!(o["message"]["thinking"], "think");
        let call = &o["message"]["tool_calls"][0];
        assert_eq!(call["function"]["name"], "get_weather");
        assert_eq!(call["function"]["arguments"]["city"], "Tokyo");
        assert_eq!(call["function"]["index"], 0);
        assert_eq!(o["prompt_eval_count"], 169);
        assert_eq!(o["prompt_eval_duration"], 24_500_000);
        assert_eq!(o["eval_count"], 15);
        assert_eq!(o["eval_duration"], 115_900_000);
        assert_eq!(o["load_duration"], 41_000_000);
        assert!(o.get("prompt_eval_cached_count").is_none());
        assert!(o["created_at"].as_str().unwrap().ends_with('Z'));

        let raw = json!({"choices": [{"text": "abc", "finish_reason": "length"}]});
        let o = from_response(Shape::RawGenerate, "m", &raw, Stats::default());
        assert_eq!(
            (o["response"].as_str(), o["done_reason"].as_str()),
            (Some("abc"), Some("length"))
        );
    }

    fn sse(chunks: &[Value]) -> Vec<u8> {
        let mut out = Vec::new();
        for c in chunks {
            out.extend(format!("data: {}\n\n", c).into_bytes());
        }
        out.extend(b"data: [DONE]\n\n");
        out
    }

    fn lines(b: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(b)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn stream_text_and_tools() {
        // Shapes of real llama-server b11236 chunks (content, tool-call fragments, usage).
        let bytes = sse(&[
            json!({"choices": [{"finish_reason": null, "index": 0, "delta": {"role": "assistant", "content": null}}]}),
            json!({"choices": [{"finish_reason": null, "index": 0, "delta": {"reasoning_content": "hm"}}]}),
            json!({"choices": [{"finish_reason": null, "index": 0, "delta": {"content": "Hel"}}]}),
            json!({"choices": [{"finish_reason": null, "index": 0, "delta": {"tool_calls": [{"index": 0, "id": "abc", "type": "function", "function": {"name": "get_weather", "arguments": "{"}}]}}]}),
            json!({"choices": [{"finish_reason": null, "index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "\"city\": \"Paris\"}"}}]}}]}),
            json!({"choices": [{"finish_reason": "tool_calls", "index": 0, "delta": {}}]}),
            json!({"choices": [], "usage": {"completion_tokens": 20, "prompt_tokens": 144},
                   "timings": {"cache_n": 4, "prompt_n": 140, "prompt_ms": 70.1, "predicted_n": 20, "predicted_ms": 131.84}}),
        ]);
        let mut t = StreamTranslator::new(Shape::Chat, "qwen3:latest");
        let mut out = Vec::new();
        // Arbitrary chunk boundaries.
        for part in bytes.chunks(37) {
            out.extend(t.push(part));
        }
        out.extend(t.finish(Stats {
            total_ns: 5,
            load_ns: 0,
        }));
        let l = lines(&out);
        assert_eq!(l.len(), 4, "{l:?}");
        assert_eq!(l[0]["message"]["thinking"], "hm");
        assert_eq!(l[0]["message"]["content"], "");
        assert_eq!(l[1]["message"]["content"], "Hel");
        assert_eq!(l[1]["done"], false);
        assert_eq!(
            l[2]["message"]["tool_calls"][0]["function"]["arguments"]["city"],
            "Paris"
        );
        assert_eq!(l[2]["message"]["tool_calls"][0]["id"], "abc");
        assert_eq!(l[3]["done"], true);
        assert_eq!(l[3]["done_reason"], "stop");
        assert_eq!(l[3]["prompt_eval_count"], 144);
        assert_eq!(l[3]["prompt_eval_cached_count"], 4);
        assert_eq!(l[3]["eval_duration"], 131_840_000);
        assert_eq!(l[3]["total_duration"], 5);
    }

    #[test]
    fn stream_error_and_generate_shape() {
        let mut t = StreamTranslator::new(Shape::Generate, "m");
        let mut out = t.push(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        );
        out.extend(t.push(b"error: x\ndata: {\"error\":{\"code\":500,\"message\":\"boom\"}}\n\n"));
        out.extend(t.finish(Stats::default()));
        let l = lines(&out);
        assert_eq!(l[0]["response"], "a");
        assert_eq!(l[1]["error"], "boom");
        assert_eq!(l.len(), 2, "no done object after an error");
        assert!(t.errored());
    }

    #[test]
    fn embeddings() {
        let req = obj(json!({"model": "e", "input": "hello", "dimensions": 2}));
        let up = embed_request(&req, "e", false).unwrap();
        assert_eq!(up.body["input"], json!(["hello"]));
        assert!(!embed_is_empty(&up));
        let legacy = embed_request(&obj(json!({"model": "e", "prompt": "x"})), "e", true).unwrap();
        assert_eq!(legacy.body["embd_normalize"], -1);
        assert!(embed_is_empty(
            &embed_request(&obj(json!({"model": "e"})), "e", false).unwrap()
        ));
        let v = json!({"data": [{"index": 1, "embedding": [0.0, 3.0, 4.0]}, {"index": 0, "embedding": [3.0, 4.0, 9.0]}],
            "usage": {"prompt_tokens": 4}});
        let o = embed_response("e", &v, false, Some(2), Stats::default());
        assert_eq!(o["embeddings"][0], json!([0.6, 0.8]));
        assert_eq!(o["embeddings"][1], json!([0.0, 1.0]));
        assert_eq!(o["prompt_eval_count"], 4);
        let o = embed_response("e", &v, true, None, Stats::default());
        assert_eq!(o["embedding"], json!([3.0, 4.0, 9.0]));
    }

    /// Measured against a real Ollama 0.34.4 and llama-server (Qwen2.5-0.5B, temperature 0.8):
    /// both treat `seed` 0 as a real seed (identical output twice) and an absent seed or -1 as
    /// random, so the value must reach llama-server untouched; 0 must not be dropped as "unset".
    #[test]
    fn seed_is_forwarded_verbatim_including_zero_and_minus_one() {
        let seed_of = |options: Value| {
            let req = obj(
                json!({"model": "m", "messages": [{"role": "user", "content": "hi"}], "options": options}),
            );
            let Action::Forward(up) = chat_request(&req, "m").unwrap() else {
                panic!()
            };
            up.body.get("seed").cloned()
        };
        assert_eq!(seed_of(json!({"seed": 0})), Some(json!(0)));
        assert_eq!(seed_of(json!({"seed": 42})), Some(json!(42)));
        assert_eq!(seed_of(json!({"seed": -1})), Some(json!(-1)));
        assert_eq!(seed_of(json!({"seed": 42.0})), Some(json!(42.0)));
        // No seed (or a null one) sends nothing: llama-server picks a random seed, like Ollama.
        assert_eq!(seed_of(json!({"temperature": 0.8})), None);
        assert_eq!(seed_of(json!({"seed": null})), None);

        let req = obj(json!({"model": "m", "prompt": "hi", "options": {"seed": 0}}));
        let (Action::Forward(up), _) = generate_request(&req, "m").unwrap() else {
            panic!()
        };
        assert_eq!(up.body["seed"], json!(0));
        let req = obj(json!({"model": "m", "prompt": "hi", "raw": true, "options": {"seed": 7}}));
        let (Action::Forward(up), _) = generate_request(&req, "m").unwrap() else {
            panic!()
        };
        assert_eq!(up.body["seed"], json!(7));
    }
}
