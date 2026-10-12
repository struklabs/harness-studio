//! `POST /v1/chat/completions`: request parsing, chat templates, stop strings and the response
//! shapes. Pure: no model, no runtime (the generation loop is in [`crate::chat_engine`]).
//!
//! * The prompt comes from the model's own Jinja chat template: `chat_template.jinja`, or the
//!   `chat_template` of `tokenizer_config.json` (a string, or a list of named templates), rendered
//!   with `minijinja` the way `transformers` does (`messages`, `add_generation_prompt`,
//!   `bos_token`, `eos_token`, `raise_exception`, `strftime_now`, plus the request's
//!   `chat_template_kwargs`, e.g. `enable_thinking`).
//! * Tool calling is not implemented: `tools` are ignored (the model just chats) and a `tool`
//!   role message is passed to the template as it is.

use crate::api::ApiError;
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, Error, ErrorKind, Value as JValue};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;

/// One `messages` entry of a request.
#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: Value,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

/// `stop`: a string or a list of strings.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Stop {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize, Default)]
pub struct StreamOptions {
    #[serde(default)]
    pub include_usage: bool,
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub stream_options: Option<StreamOptions>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub max_completion_tokens: Option<i64>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub top_k: Option<i64>,
    #[serde(default)]
    pub repeat_penalty: Option<f64>,
    #[serde(default)]
    pub repetition_penalty: Option<f64>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub stop: Option<Stop>,
    #[serde(default)]
    pub n: Option<u32>,
    #[serde(default)]
    pub chat_template_kwargs: Option<Map<String, Value>>,
}

/// A chat request after validation.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatJob {
    /// `{role, content}` objects (content flattened to text), plus any extra fields the
    /// template may read (`name`, `tool_calls`, `tool_call_id`).
    pub messages: Vec<Value>,
    pub stream: bool,
    pub include_usage: bool,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<i64>,
    pub repetition_penalty: Option<f64>,
    pub seed: Option<u64>,
    pub stop: Vec<String>,
    pub template_kwargs: Map<String, Value>,
}

/// The text of a message `content`: a string, or an array of parts of which the `text` ones
/// are joined. Images and audio are refused (these models are text only).
fn content_text(content: &Value) -> Result<String, ApiError> {
    match content {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s.clone()),
        Value::Array(parts) => {
            let mut text = Vec::new();
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") | None => {
                        if let Some(t) = part.get("text").and_then(Value::as_str) {
                            text.push(t.to_string());
                        } else if let Some(t) = part.as_str() {
                            text.push(t.to_string());
                        }
                    }
                    Some(other) => {
                        return Err(ApiError::invalid(format!(
                            "content part of type \"{other}\" is not supported: this model reads text only"
                        )))
                    }
                }
            }
            Ok(text.join("\n"))
        }
        other => Err(ApiError::invalid(format!(
            "message content must be a string or an array, not {other}"
        ))),
    }
}

impl ChatRequest {
    pub fn into_job(self) -> Result<ChatJob, ApiError> {
        if self.messages.is_empty() {
            return Err(ApiError::invalid("messages must not be empty"));
        }
        if self.n.is_some_and(|n| n != 1) {
            return Err(ApiError::invalid("n must be 1: one completion per request"));
        }
        let mut messages = Vec::new();
        for m in self.messages {
            let mut obj = m.rest;
            obj.insert("role".into(), json!(m.role));
            obj.insert("content".into(), json!(content_text(&m.content)?));
            messages.push(Value::Object(obj));
        }
        let max = self.max_completion_tokens.or(self.max_tokens);
        let max_tokens = match max {
            // llama-server's convention: -1 means "until the model stops".
            None | Some(-1) => None,
            Some(n) if n >= 1 => Some(n as usize),
            Some(n) => {
                return Err(ApiError::invalid(format!(
                    "max_tokens must be at least 1, not {n}"
                )))
            }
        };
        if self.temperature.is_some_and(|t| !(0.0..=5.0).contains(&t)) {
            return Err(ApiError::invalid("temperature must be between 0 and 5"));
        }
        if self.top_p.is_some_and(|p| !(0.0..=1.0).contains(&p)) {
            return Err(ApiError::invalid("top_p must be between 0 and 1"));
        }
        let stop = match self.stop {
            None => Vec::new(),
            Some(Stop::One(s)) => vec![s],
            Some(Stop::Many(v)) => v,
        }
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
        if stop.len() > 16 {
            return Err(ApiError::invalid("at most 16 stop sequences"));
        }
        Ok(ChatJob {
            messages,
            stream: self.stream,
            include_usage: self.stream_options.is_some_and(|o| o.include_usage),
            max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            repetition_penalty: self.repeat_penalty.or(self.repetition_penalty),
            seed: self.seed,
            stop,
            template_kwargs: self.chat_template_kwargs.unwrap_or_default(),
        })
    }
}

/// The model's chat template and the special tokens it names.
#[derive(Debug, Clone)]
pub struct ChatTemplate {
    source: String,
    bos: String,
    eos: String,
    /// Where the template came from, for the startup log.
    pub origin: String,
}

/// A special token of `tokenizer_config.json`: a string, or `{"content": "..."}`.
fn token_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) => o
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

/// The `chat_template` value of a tokenizer config: a string, or the list of
/// `{name, template}` entries (the one named `default`, else the first).
fn template_from_config(v: &Value) -> Option<String> {
    match v.get("chat_template")? {
        Value::String(s) => Some(s.clone()),
        Value::Array(list) => {
            let pick = list
                .iter()
                .find(|e| e.get("name").and_then(Value::as_str) == Some("default"))
                .or_else(|| list.first())?;
            pick.get("template")
                .and_then(Value::as_str)
                .map(String::from)
        }
        _ => None,
    }
}

impl ChatTemplate {
    pub fn from_source(source: String, bos: String, eos: String, origin: &str) -> Self {
        Self {
            source,
            bos,
            eos,
            origin: origin.to_string(),
        }
    }

    /// The template of a model folder; `None` when it ships none.
    pub fn load(dir: &Path) -> Option<Self> {
        let config: Value = std::fs::read_to_string(dir.join("tokenizer_config.json"))
            .ok()
            .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
            .unwrap_or(Value::Null);
        let bos = token_text(config.get("bos_token"));
        let eos = token_text(config.get("eos_token"));
        if let Ok(file) = std::fs::read_to_string(dir.join("chat_template.jinja")) {
            return Some(Self::from_source(file, bos, eos, "chat_template.jinja"));
        }
        let source = template_from_config(&config)?;
        Some(Self::from_source(source, bos, eos, "tokenizer_config.json"))
    }

    /// The prompt for `messages`, ending where the assistant's answer starts.
    pub fn render(
        &self,
        messages: &[Value],
        kwargs: &Map<String, Value>,
    ) -> Result<String, ApiError> {
        let mut env = Environment::new();
        // The environment `transformers` renders chat templates in.
        env.set_syntax(
            SyntaxConfig::builder()
                .trim_blocks(true)
                .lstrip_blocks(true)
                .build()
                .map_err(|e| ApiError::server(e.to_string()))?,
        );
        env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
        env.add_function("raise_exception", |msg: String| -> Result<JValue, Error> {
            Err(Error::new(ErrorKind::InvalidOperation, msg))
        });
        env.add_function("strftime_now", |fmt: String| -> String {
            strftime(&fmt, std::time::SystemTime::now())
        });
        env.add_template("chat", &self.source)
            .map_err(|e| ApiError::server(format!("the chat template does not compile: {e}")))?;
        let template = env
            .get_template("chat")
            .map_err(|e| ApiError::server(e.to_string()))?;
        let mut ctx: Map<String, Value> = kwargs.clone();
        ctx.insert("messages".into(), Value::Array(messages.to_vec()));
        ctx.insert("add_generation_prompt".into(), json!(true));
        ctx.entry("bos_token").or_insert_with(|| json!(self.bos));
        ctx.entry("eos_token").or_insert_with(|| json!(self.eos));
        template.render(to_jinja(&Value::Object(ctx))).map_err(|e| {
            // The template's own `raise_exception` is a client error (a wrong role order).
            let reason = e
                .detail()
                .map(str::to_string)
                .unwrap_or_else(|| e.to_string());
            ApiError::invalid(format!("chat template: {reason}"))
        })
    }
}

/// A JSON value as a template value (minijinja 3 has no serde bridge of its own).
fn to_jinja(v: &Value) -> JValue {
    match v {
        Value::Null => JValue::from(()),
        Value::Bool(b) => JValue::from(*b),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => JValue::from(i),
            (None, Some(u)) => JValue::from(u),
            _ => JValue::from(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => JValue::from(s.as_str()),
        Value::Array(a) => JValue::from(a.iter().map(to_jinja).collect::<Vec<_>>()),
        Value::Object(o) => JValue::from_pairs(o.iter().map(|(k, v)| (k.clone(), to_jinja(v)))),
    }
}

/// `strftime_now` for templates that print the date (Llama 3.1): the codes they use.
fn strftime(fmt: &str, at: std::time::SystemTime) -> String {
    let secs = at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const WEEKDAYS: [&str; 7] = [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ];
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&year.to_string()),
            Some('m') => out.push_str(&format!("{month:02}")),
            Some('d') => out.push_str(&format!("{day:02}")),
            Some('e') => out.push_str(&format!("{day:2}")),
            Some('B') => out.push_str(MONTHS[month as usize - 1]),
            Some('b') => out.push_str(&MONTHS[month as usize - 1][..3]),
            Some('A') => out.push_str(WEEKDAYS[days.rem_euclid(7) as usize]),
            Some('a') => out.push_str(&WEEKDAYS[days.rem_euclid(7) as usize][..3]),
            Some('H') => out.push_str(&format!("{:02}", rem / 3600)),
            Some('M') => out.push_str(&format!("{:02}", rem % 3600 / 60)),
            Some('S') => out.push_str(&format!("{:02}", rem % 60)),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// Cuts generated text at the first stop string while streaming: text that might still become a
/// stop string is held back until it is clear.
#[derive(Debug, Default)]
pub struct StopFilter {
    stops: Vec<String>,
    held: String,
    pub stopped: bool,
}

impl StopFilter {
    pub fn new(stops: Vec<String>) -> Self {
        Self {
            stops,
            held: String::new(),
            stopped: false,
        }
    }

    /// Feeds new text; returns what is safe to send now.
    pub fn push(&mut self, text: &str) -> String {
        if self.stopped {
            return String::new();
        }
        self.held.push_str(text);
        if let Some(cut) = self
            .stops
            .iter()
            .filter_map(|s| self.held.find(s.as_str()))
            .min()
        {
            self.stopped = true;
            let out = self.held[..cut].to_string();
            self.held.clear();
            return out;
        }
        // Keep the longest tail that is a proper prefix of a stop string.
        let longest = self.stops.iter().map(String::len).max().unwrap_or(0);
        let keep = (1..longest.min(self.held.len() + 1))
            .rev()
            .find(|&n| {
                self.held.is_char_boundary(self.held.len() - n)
                    && self.stops.iter().any(|s| {
                        s.len() > n && s.is_char_boundary(n) && self.held.ends_with(&s[..n])
                    })
            })
            .unwrap_or(0);
        let at = self.held.len() - keep;
        let out = self.held[..at].to_string();
        self.held.drain(..at);
        out
    }

    /// The generation ended: whatever was held back belongs to the answer.
    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.held)
    }
}

/// Why a completion ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    Stop,
    Length,
}

impl Finish {
    pub fn as_str(self) -> &'static str {
        match self {
            Finish::Stop => "stop",
            Finish::Length => "length",
        }
    }
}

/// Token counts and speed of one request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    /// Seconds spent on the prompt (first token) and on the rest.
    pub prompt_secs: f64,
    pub generation_secs: f64,
}

impl Usage {
    pub fn tokens_per_second(&self) -> f64 {
        if self.generation_secs > 0.0 {
            self.completion_tokens.saturating_sub(1) as f64 / self.generation_secs
        } else {
            0.0
        }
    }

    pub fn json(&self) -> Value {
        json!({
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "total_tokens": self.prompt_tokens + self.completion_tokens,
        })
    }

    /// llama-server's `timings` object, so the gateway and the UI read the same fields.
    pub fn timings(&self) -> Value {
        let per = |tokens: usize, secs: f64| {
            if tokens > 0 && secs > 0.0 {
                secs * 1000.0 / tokens as f64
            } else {
                0.0
            }
        };
        let rate = |tokens: usize, secs: f64| {
            if secs > 0.0 {
                tokens as f64 / secs
            } else {
                0.0
            }
        };
        json!({
            "prompt_n": self.prompt_tokens,
            "prompt_ms": self.prompt_secs * 1000.0,
            "prompt_per_token_ms": per(self.prompt_tokens, self.prompt_secs),
            "prompt_per_second": rate(self.prompt_tokens, self.prompt_secs),
            "predicted_n": self.completion_tokens,
            "predicted_ms": self.generation_secs * 1000.0,
            "predicted_per_second": self.tokens_per_second(),
        })
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The non-streaming response.
pub fn completion_response(
    id: &str,
    model: &str,
    text: &str,
    finish: Finish,
    usage: &Usage,
) -> Value {
    json!({
        "id": id,
        "object": "chat.completion",
        "created": now(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": finish.as_str(),
        }],
        "usage": usage.json(),
        "timings": usage.timings(),
    })
}

/// One streaming chunk: the role announcement, a text delta, or the closing chunk.
pub fn chunk(id: &str, model: &str, delta: Value, finish: Option<Finish>) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": now(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish.map(Finish::as_str),
        }],
    })
}

/// The trailing usage chunk of `stream_options.include_usage`.
pub fn usage_chunk(id: &str, model: &str, usage: &Usage) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": now(),
        "model": model,
        "choices": [],
        "usage": usage.json(),
        "timings": usage.timings(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(v: Value) -> Result<ChatJob, ApiError> {
        serde_json::from_value::<ChatRequest>(v).unwrap().into_job()
    }

    #[test]
    fn request_defaults_and_overrides() {
        let job = request(json!({
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 64, "temperature": 0.2, "top_p": 0.9, "top_k": 40,
            "stop": "END", "stream": true, "stream_options": {"include_usage": true},
            "seed": 7, "chat_template_kwargs": {"enable_thinking": false}
        }))
        .unwrap();
        assert_eq!(job.max_tokens, Some(64));
        assert_eq!(job.stop, vec!["END".to_string()]);
        assert!(job.stream && job.include_usage);
        assert_eq!(
            (job.temperature, job.top_p, job.top_k, job.seed),
            (Some(0.2), Some(0.9), Some(40), Some(7))
        );
        assert_eq!(job.template_kwargs["enable_thinking"], json!(false));
        let plain =
            request(json!({"messages": [{"role": "user", "content": "x"}], "max_tokens": -1}))
                .unwrap();
        assert_eq!(plain.max_tokens, None);
        assert!(plain.stop.is_empty() && !plain.stream);
    }

    #[test]
    fn content_parts_are_joined_and_images_refused() {
        let job = request(json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "a"}, {"type": "text", "text": "b"}]}]}))
        .unwrap();
        assert_eq!(job.messages[0]["content"], json!("a\nb"));
        let err = request(json!({"messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"url": "x"}}]}]}))
        .unwrap_err();
        assert!(err.message.contains("image_url"));
    }

    #[test]
    fn invalid_requests_are_400s() {
        for body in [
            json!({"messages": []}),
            json!({"messages": [{"role": "user", "content": "x"}], "n": 2}),
            json!({"messages": [{"role": "user", "content": "x"}], "max_tokens": 0}),
            json!({"messages": [{"role": "user", "content": "x"}], "temperature": 9}),
            json!({"messages": [{"role": "user", "content": "x"}], "top_p": 1.5}),
        ] {
            assert_eq!(request(body.clone()).unwrap_err().status, 400, "{body}");
        }
    }

    const CHATML: &str = "{% for m in messages %}<|im_start|>{{ m['role'] }}\n{{ m['content'] }}<|im_end|>\n{% endfor %}{% if add_generation_prompt %}<|im_start|>assistant\n{% endif %}";

    fn template(src: &str) -> ChatTemplate {
        ChatTemplate::from_source(src.into(), "<s>".into(), "</s>".into(), "test")
    }

    #[test]
    fn chatml_template_renders() {
        let t = template(CHATML);
        let msgs = vec![
            json!({"role": "system", "content": "be brief"}),
            json!({"role": "user", "content": "hi"}),
        ];
        let out = t.render(&msgs, &Map::new()).unwrap();
        assert_eq!(
            out,
            "<|im_start|>system\nbe brief<|im_end|>\n<|im_start|>user\nhi<|im_end|>\n<|im_start|>assistant\n"
        );
    }

    #[test]
    fn special_tokens_kwargs_and_python_methods_work() {
        let t = template(
            "{{ bos_token }}{% for m in messages %}{{ m['content'].strip() | upper }}{% endfor %}{% if enable_thinking is defined and enable_thinking is false %}<nothink>{% endif %}{{ strftime_now('%Y') | length }}",
        );
        let msgs = vec![json!({"role": "user", "content": "  hi "})];
        let mut kw = Map::new();
        assert_eq!(t.render(&msgs, &kw).unwrap(), "<s>HI4");
        kw.insert("enable_thinking".into(), json!(false));
        assert_eq!(t.render(&msgs, &kw).unwrap(), "<s>HI<nothink>4");
    }

    #[test]
    fn raise_exception_is_a_client_error() {
        let t = template("{{ raise_exception('roles must alternate') }}");
        let e = t.render(&[], &Map::new()).unwrap_err();
        assert_eq!(e.status, 400);
        assert!(e.message.contains("roles must alternate"), "{}", e.message);
        let broken = template("{% for %}");
        assert_eq!(broken.render(&[], &Map::new()).unwrap_err().status, 500);
    }

    #[test]
    fn templates_are_found_in_the_folder() {
        let d = tempfile::tempdir().unwrap();
        assert!(ChatTemplate::load(d.path()).is_none());
        std::fs::write(
            d.path().join("tokenizer_config.json"),
            r#"{"bos_token": {"content": "<s>"}, "eos_token": "</s>",
                "chat_template": [{"name": "tool_use", "template": "T"}, {"name": "default", "template": "D{{ bos_token }}"}]}"#,
        )
        .unwrap();
        let t = ChatTemplate::load(d.path()).unwrap();
        assert_eq!(t.origin, "tokenizer_config.json");
        assert_eq!(t.render(&[], &Map::new()).unwrap(), "D<s>");
        // A jinja file next to it wins.
        std::fs::write(d.path().join("chat_template.jinja"), "FILE").unwrap();
        let t = ChatTemplate::load(d.path()).unwrap();
        assert_eq!(
            (
                t.origin.as_str(),
                t.render(&[], &Map::new()).unwrap().as_str()
            ),
            ("chat_template.jinja", "FILE")
        );
    }

    #[test]
    fn strftime_formats_a_known_instant() {
        // 2024-07-23 14:05:09 UTC, a Tuesday.
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_721_743_509);
        assert_eq!(strftime("%d %b %Y", at), "23 Jul 2024");
        assert_eq!(
            strftime("%A %B %e, %H:%M:%S %%", at),
            "Tuesday July 23, 14:05:09 %"
        );
    }

    #[test]
    fn stop_filter_cuts_and_holds_back_partial_matches() {
        let mut f = StopFilter::new(vec!["<end>".into()]);
        assert_eq!(f.push("Hello <e"), "Hello ");
        assert_eq!(f.push("nd> ignored"), "");
        assert!(f.stopped);
        assert_eq!(f.push("more"), "");

        // A held prefix that does not complete is released.
        let mut f = StopFilter::new(vec!["<end>".into()]);
        assert_eq!(f.push("a <e"), "a ");
        assert_eq!(f.push("x"), "<ex");
        assert!(!f.stopped);
        assert_eq!(f.finish(), "");

        // Held text is flushed when the generation ends.
        let mut f = StopFilter::new(vec!["STOP".into()]);
        assert_eq!(f.push("ST"), "");
        assert_eq!(f.finish(), "ST");

        // The earliest of several stop strings wins; multi-byte text is not split.
        let mut f = StopFilter::new(vec!["b".into(), "ü!".into()]);
        assert_eq!(f.push("aü"), "a");
        assert_eq!(f.push("!zz"), "");
        assert!(f.stopped);
    }

    #[test]
    fn responses_have_the_openai_shape() {
        let usage = Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            prompt_secs: 0.5,
            generation_secs: 0.4,
        };
        let r = completion_response("chatcmpl-1", "m", "hi", Finish::Length, &usage);
        assert_eq!(r["choices"][0]["finish_reason"], "length");
        assert_eq!(r["usage"]["total_tokens"], 15);
        assert!((usage.tokens_per_second() - 10.0).abs() < 1e-9);
        assert_eq!(r["timings"]["predicted_n"], 5);
        let c = chunk("id", "m", json!({"content": "x"}), None);
        assert!(c["choices"][0]["finish_reason"].is_null());
        assert_eq!(usage_chunk("id", "m", &usage)["usage"]["prompt_tokens"], 10);
    }
}
