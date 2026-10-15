//! Request and response shapes of the two decision APIs, read into one form ([`DecisionRequest`],
//! [`Outcome`]) and written back.
//!
//! * `/v1/systemone` (Ollama, LM Studio, llama.cpp): `state`, `images`, `questions` as an object
//!   keyed by name with `choice` / `noul` / `score` questions and their `criteria`; answers as
//!   an object keyed by the same names.
//! * `/v1/decisions` (OpenAI, LM Studio): `input`, `questions` as an array with `predicate` /
//!   `choice` (`choices`) / `score` (`levels`) questions; answers as an array in question order.
//!
//! Only documented fields are written. Probabilities are normalized over the candidates;
//! `confidence` is `1 - H(p) / ln(N)` (Ollama's definition, also llama.cpp's).

use serde_json::{json, Map, Value};

/// Questions per request (Ollama's limit; llama.cpp and OpenAI document none).
pub const MAX_QUESTIONS: usize = 64;
/// Options of a choice question.
pub const MAX_CHOICES: usize = 255;
/// Levels of a score question.
pub const MAX_LEVELS: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// `noul` / `predicate`: the probability of true; optional descriptions of both answers.
    Noul {
        yes: Option<String>,
        no: Option<String>,
    },
    Choice(Vec<Choice>),
    /// Lowest level first.
    Score(Vec<Level>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// The option as the model sees it (an OpenAI boolean value as `true` / `false`).
    pub key: String,
    /// The value the client sent (a string, or a boolean for `/v1/decisions`).
    pub value: Value,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub label: String,
    pub description: Option<String>,
}

impl Level {
    /// The level as one criterion text (`label: description`).
    pub fn text(&self) -> String {
        match self.description.as_deref().map(str::trim) {
            Some(d) if !d.is_empty() && !self.label.trim().is_empty() => {
                format!("{}: {}", self.label, d)
            }
            Some(d) if !d.is_empty() => d.to_string(),
            _ => self.label.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// `None` only for an unnamed `/v1/decisions` question.
    pub name: Option<String>,
    /// A string, an object or an array (given to the model as JSON text).
    pub instructions: Value,
    pub kind: Kind,
}

impl Question {
    /// Candidates the probabilities are spread over.
    pub fn candidates(&self) -> usize {
        match &self.kind {
            Kind::Noul { .. } => 2,
            Kind::Choice(c) => c.len(),
            Kind::Score(l) => l.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionRequest {
    pub model: String,
    /// What the questions are about: a string, an object or an array.
    pub state: Value,
    /// Images as data URLs, in request order.
    pub images: Vec<String>,
    pub questions: Vec<Question>,
    /// Ollama `keep_alive` (`/v1/systemone` only), as sent.
    pub keep_alive: Option<Value>,
}

/// Probabilities of one question, normalized over its candidates.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// Probability of true.
    Noul(f64),
    /// One per option, in option order.
    Choice(Vec<f64>),
    /// One per level, lowest first.
    Score(Vec<f64>),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Outcome {
    pub answers: Vec<Answer>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// A request the API refuses (400), with the field it is about.
#[derive(Debug, Clone, PartialEq)]
pub struct Invalid {
    pub message: String,
    pub param: Option<String>,
}

fn invalid(param: impl Into<String>, message: impl Into<String>) -> Invalid {
    Invalid {
        message: message.into(),
        param: Some(param.into()),
    }
}

/// Ollama's `\S` rule for names and strings.
fn has_text(s: &str) -> bool {
    !s.trim().is_empty()
}

fn object(body: &[u8]) -> Result<Map<String, Value>, Invalid> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(o)) => Ok(o),
        Ok(_) => Err(Invalid {
            message: "the request body must be a JSON object".into(),
            param: None,
        }),
        Err(e) => Err(Invalid {
            message: format!("invalid JSON: {e}"),
            param: None,
        }),
    }
}

fn model(req: &Map<String, Value>) -> Result<String, Invalid> {
    match req.get("model") {
        Some(Value::String(m)) if has_text(m) => Ok(m.trim().to_string()),
        Some(Value::String(_)) | None | Some(Value::Null) => {
            Err(invalid("model", "model is required"))
        }
        Some(_) => Err(invalid("model", "model must be a string")),
    }
}

fn no_stream(req: &Map<String, Value>) -> Result<(), Invalid> {
    if req.get("stream").and_then(Value::as_bool) == Some(true) {
        return Err(invalid(
            "stream",
            "streaming is not supported for decisions",
        ));
    }
    Ok(())
}

/// A `state`-style value: a string with text, an object or an array.
fn state_value(v: Option<&Value>, param: &str) -> Result<Value, Invalid> {
    match v {
        Some(Value::String(s)) if has_text(s) => Ok(Value::String(s.clone())),
        Some(v @ (Value::Object(_) | Value::Array(_))) => Ok(v.clone()),
        None | Some(Value::Null) => Err(invalid(param, format!("{param} is required"))),
        Some(_) => Err(invalid(
            param,
            format!("{param} must be a non-empty string, an object or an array"),
        )),
    }
}

/// An image as a data URL: Ollama sends bare base64, LM Studio and llama.cpp data URLs.
fn image_url(raw: &str, param: &str) -> Result<String, Invalid> {
    let raw = raw.trim();
    if raw.starts_with("data:") {
        if !raw.starts_with("data:image/") || !raw.contains(";base64,") {
            return Err(invalid(
                param,
                "images must be base64 image data (data:image/...;base64,...)",
            ));
        }
        return Ok(raw.to_string());
    }
    if raw.is_empty() || raw.contains("://") {
        return Err(invalid(
            param,
            "image URLs are not supported: send the image as base64 data",
        ));
    }
    Ok(format!(
        "data:{};base64,{}",
        crate::ollama::convert::image_mime(raw),
        raw
    ))
}

// --- /v1/systemone ---

/// Reads an Ollama / LM Studio / llama.cpp `/v1/systemone` request.
pub fn parse_systemone(body: &[u8]) -> Result<DecisionRequest, Invalid> {
    let req = object(body)?;
    no_stream(&req)?;
    let model = model(&req)?;
    let state = state_value(req.get("state"), "state")?;
    let mut images = Vec::new();
    // `files` is llama.cpp's name, `images` its alias and Ollama's / LM Studio's.
    for key in ["images", "files"] {
        match req.get(key) {
            None | Some(Value::Null) => {}
            Some(Value::Array(list)) => {
                for (i, v) in list.iter().enumerate() {
                    let param = format!("{key}[{i}]");
                    let s = v
                        .as_str()
                        .ok_or_else(|| invalid(&param, "images must be strings"))?;
                    images.push(image_url(s, &param)?);
                }
            }
            Some(_) => return Err(invalid(key, format!("{key} must be an array"))),
        }
    }
    let list = match req.get("questions") {
        Some(Value::Object(q)) => q,
        None | Some(Value::Null) => return Err(invalid("questions", "questions is required")),
        Some(_) => {
            return Err(invalid(
                "questions",
                "questions must be an object that maps each question name to a question",
            ))
        }
    };
    if list.is_empty() || list.len() > MAX_QUESTIONS {
        return Err(invalid(
            "questions",
            format!("questions must have 1 to {MAX_QUESTIONS} entries"),
        ));
    }
    let mut questions = Vec::with_capacity(list.len());
    for (name, q) in list {
        let param = format!("questions.{name}");
        if !has_text(name) {
            return Err(invalid("questions", "question names must not be blank"));
        }
        let q = q
            .as_object()
            .ok_or_else(|| invalid(&param, format!("{param} must be an object")))?;
        let instructions = state_value(q.get("instructions"), &format!("{param}.instructions"))?;
        let criteria = q.get("criteria").filter(|c| !c.is_null());
        let cparam = format!("{param}.criteria");
        let kind = match q.get("type").and_then(Value::as_str) {
            Some("choice") => {
                let Some(Value::Object(c)) = criteria else {
                    return Err(invalid(
                        &cparam,
                        format!(
                            "{cparam} must be an object that maps each option to its description"
                        ),
                    ));
                };
                if c.len() < 2 || c.len() > MAX_CHOICES {
                    return Err(invalid(
                        &cparam,
                        format!("{cparam} must have 2 to {MAX_CHOICES} options"),
                    ));
                }
                let mut options = Vec::with_capacity(c.len());
                for (key, d) in c {
                    if !has_text(key) {
                        return Err(invalid(&cparam, "option names must not be blank"));
                    }
                    let description = match d {
                        Value::Null => None,
                        Value::String(s) => Some(s.clone()),
                        _ => {
                            return Err(invalid(
                                &cparam,
                                format!("{cparam}.{key} must be a string or null"),
                            ))
                        }
                    };
                    options.push(Choice {
                        key: key.clone(),
                        value: Value::String(key.clone()),
                        description,
                    });
                }
                Kind::Choice(options)
            }
            Some("noul") => {
                let (mut yes, mut no) = (None, None);
                match criteria {
                    None => {}
                    Some(Value::Object(c)) => {
                        for (key, d) in c {
                            let d = d.as_str().map(str::to_string).ok_or_else(|| {
                                invalid(&cparam, format!("{cparam}.{key} must be a string"))
                            })?;
                            match key.as_str() {
                                "true" => yes = Some(d),
                                "false" => no = Some(d),
                                _ => {
                                    return Err(invalid(
                                        &cparam,
                                        format!(
                                            "{cparam} may only describe \"true\" and \"false\""
                                        ),
                                    ))
                                }
                            }
                        }
                    }
                    Some(_) => return Err(invalid(&cparam, format!("{cparam} must be an object"))),
                }
                Kind::Noul { yes, no }
            }
            Some("score") => {
                let Some(Value::Array(c)) = criteria else {
                    return Err(invalid(
                        &cparam,
                        format!("{cparam} must be an array of level descriptions"),
                    ));
                };
                if c.len() < 2 || c.len() > MAX_LEVELS {
                    return Err(invalid(
                        &cparam,
                        format!("{cparam} must have 2 to {MAX_LEVELS} levels"),
                    ));
                }
                let levels = c
                    .iter()
                    .map(|l| {
                        l.as_str()
                            .map(|s| Level {
                                label: s.to_string(),
                                description: None,
                            })
                            .ok_or_else(|| {
                                invalid(&cparam, format!("{cparam} must contain strings"))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Kind::Score(levels)
            }
            _ => {
                return Err(invalid(
                    format!("{param}.type"),
                    format!("{param}: \"type\" must be one of: choice, score, noul"),
                ))
            }
        };
        questions.push(Question {
            name: Some(name.clone()),
            instructions,
            kind,
        });
    }
    Ok(DecisionRequest {
        model,
        state,
        images,
        questions,
        keep_alive: req.get("keep_alive").cloned(),
    })
}

/// The `/v1/systemone` response.
pub fn render_systemone(model: &str, req: &DecisionRequest, out: &Outcome) -> Value {
    let mut answers = Map::new();
    for (q, a) in req.questions.iter().zip(&out.answers) {
        let name = q.name.clone().unwrap_or_default();
        let value = match (&q.kind, a) {
            (Kind::Noul { .. }, Answer::Noul(p)) => json!({"type": "noul", "noul": p}),
            (Kind::Choice(options), Answer::Choice(p)) => {
                let probabilities: Map<String, Value> = options
                    .iter()
                    .zip(p)
                    .map(|(o, p)| (o.key.clone(), json!(p)))
                    .collect();
                json!({
                    "type": "choice",
                    "choice": options[argmax(p)].key,
                    "probabilities": probabilities,
                    "confidence": confidence(p),
                })
            }
            (Kind::Score(levels), Answer::Score(p)) => {
                let legend: Map<String, Value> = levels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| (i.to_string(), json!(l.text())))
                    .collect();
                let probabilities: Map<String, Value> = p
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (i.to_string(), json!(p)))
                    .collect();
                json!({
                    "type": "score",
                    "score": expected(p),
                    "legend": legend,
                    "probabilities": probabilities,
                    "confidence": confidence(p),
                })
            }
            _ => continue,
        };
        answers.insert(name, value);
    }
    json!({
        "model": model,
        "answers": answers,
        "usage": {"input_tokens": out.input_tokens, "output_tokens": out.output_tokens},
    })
}

// --- /v1/decisions ---

fn opt_text(v: Option<&Value>, param: &str) -> Result<Option<String>, Invalid> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(invalid(param, format!("{param} must be a string"))),
    }
}

/// `input` as a state and images: a string, or user messages whose content is a string or
/// `input_text` / `input_image` parts (images as data URLs only, like OpenAI's endpoint).
fn decisions_input(v: Option<&Value>) -> Result<(Value, Vec<String>), Invalid> {
    let list = match v {
        Some(Value::String(s)) if has_text(s) => return Ok((Value::String(s.clone()), vec![])),
        Some(Value::Array(list)) if !list.is_empty() => list,
        None | Some(Value::Null) => return Err(invalid("input", "input is required")),
        _ => {
            return Err(invalid(
                "input",
                "input must be a non-empty string or an array of user messages",
            ))
        }
    };
    let mut texts = Vec::new();
    let mut images = Vec::new();
    for (i, m) in list.iter().enumerate() {
        let param = format!("input[{i}]");
        let m = m
            .as_object()
            .ok_or_else(|| invalid(&param, format!("{param} must be a message object")))?;
        if m.get("role").and_then(Value::as_str).unwrap_or("user") != "user" {
            return Err(invalid(
                format!("{param}.role"),
                "only user messages are supported",
            ));
        }
        match m.get("content") {
            Some(Value::String(s)) => texts.push(s.clone()),
            Some(Value::Array(parts)) => {
                for (j, p) in parts.iter().enumerate() {
                    let pparam = format!("{param}.content[{j}]");
                    match p.get("type").and_then(Value::as_str) {
                        Some("input_text") => texts.push(
                            p.get("text")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    invalid(format!("{pparam}.text"), "text is required")
                                })?
                                .to_string(),
                        ),
                        Some("input_image") => {
                            let url = p.get("image_url").and_then(Value::as_str);
                            match url {
                                Some(u) if u.trim_start().starts_with("data:") => {
                                    images.push(image_url(u, &format!("{pparam}.image_url"))?)
                                }
                                _ => {
                                    return Err(invalid(
                                        format!("{pparam}.image_url"),
                                        "images must be inline base64 data URLs; image URLs and file_id are not supported",
                                    ))
                                }
                            }
                        }
                        _ => {
                            return Err(invalid(
                                format!("{pparam}.type"),
                                "content parts must be input_text or input_image",
                            ))
                        }
                    }
                }
            }
            _ => {
                return Err(invalid(
                    format!("{param}.content"),
                    "content must be a string or an array of parts",
                ))
            }
        }
    }
    let text = texts.join("\n\n");
    if !has_text(&text) && images.is_empty() {
        return Err(invalid("input", "input must contain text or an image"));
    }
    Ok((Value::String(text), images))
}

/// Reads an OpenAI / LM Studio `/v1/decisions` request.
pub fn parse_decisions(body: &[u8]) -> Result<DecisionRequest, Invalid> {
    let req = object(body)?;
    no_stream(&req)?;
    let model = model(&req)?;
    let (state, images) = decisions_input(req.get("input"))?;
    let list = match req.get("questions") {
        Some(Value::Array(q)) => q,
        None | Some(Value::Null) => return Err(invalid("questions", "questions is required")),
        Some(_) => return Err(invalid("questions", "questions must be an array")),
    };
    if list.is_empty() || list.len() > MAX_QUESTIONS {
        return Err(invalid(
            "questions",
            format!("questions must have 1 to {MAX_QUESTIONS} entries"),
        ));
    }
    let mut questions = Vec::with_capacity(list.len());
    let mut names = std::collections::HashSet::new();
    for (i, q) in list.iter().enumerate() {
        let param = format!("questions[{i}]");
        let q = q
            .as_object()
            .ok_or_else(|| invalid(&param, format!("{param} must be an object")))?;
        let name = opt_text(q.get("name"), &format!("{param}.name"))?;
        if let Some(n) = &name {
            if !names.insert(n.clone()) {
                return Err(invalid(
                    format!("{param}.name"),
                    format!("question names must be unique: \"{n}\" is used twice"),
                ));
            }
        }
        let iparam = format!("{param}.instructions");
        let instructions = match q.get("instructions") {
            Some(Value::String(s)) if has_text(s) => Value::String(s.clone()),
            _ => {
                return Err(invalid(
                    &iparam,
                    format!("{iparam} must be a non-empty string"),
                ))
            }
        };
        let kind = match q.get("type").and_then(Value::as_str) {
            Some("predicate") => Kind::Noul {
                yes: None,
                no: None,
            },
            Some("choice") => {
                let cparam = format!("{param}.choices");
                let Some(Value::Array(c)) = q.get("choices") else {
                    return Err(invalid(&cparam, format!("{cparam} must be an array")));
                };
                if c.is_empty() || c.len() > MAX_CHOICES {
                    return Err(invalid(
                        &cparam,
                        format!("{cparam} must have 1 to {MAX_CHOICES} entries"),
                    ));
                }
                let mut options = Vec::with_capacity(c.len());
                let mut keys = std::collections::HashSet::new();
                for (j, o) in c.iter().enumerate() {
                    let oparam = format!("{cparam}[{j}]");
                    let value = o.get("value").cloned().unwrap_or(Value::Null);
                    let key = match &value {
                        Value::String(s) if has_text(s) => s.clone(),
                        Value::Bool(b) => b.to_string(),
                        _ => {
                            return Err(invalid(
                                format!("{oparam}.value"),
                                "a choice value must be a non-empty string or a boolean",
                            ))
                        }
                    };
                    if !keys.insert(key.clone()) {
                        return Err(invalid(
                            format!("{oparam}.value"),
                            format!("choice values must be unique: \"{key}\" is used twice"),
                        ));
                    }
                    let description =
                        opt_text(o.get("description"), &format!("{oparam}.description"))?;
                    options.push(Choice {
                        key,
                        value,
                        description,
                    });
                }
                Kind::Choice(options)
            }
            Some("score") => {
                let lparam = format!("{param}.levels");
                let Some(Value::Array(l)) = q.get("levels") else {
                    return Err(invalid(&lparam, format!("{lparam} must be an array")));
                };
                if l.len() < 2 || l.len() > MAX_LEVELS {
                    return Err(invalid(
                        &lparam,
                        format!("{lparam} must have 2 to {MAX_LEVELS} levels"),
                    ));
                }
                let mut levels = Vec::with_capacity(l.len());
                for (j, o) in l.iter().enumerate() {
                    let oparam = format!("{lparam}[{j}]");
                    let label = match o.get("label") {
                        Some(Value::String(s)) if has_text(s) => s.clone(),
                        _ => {
                            return Err(invalid(
                                format!("{oparam}.label"),
                                "a level label must be a non-empty string",
                            ))
                        }
                    };
                    let description =
                        opt_text(o.get("description"), &format!("{oparam}.description"))?;
                    levels.push(Level { label, description });
                }
                Kind::Score(levels)
            }
            _ => {
                return Err(invalid(
                    format!("{param}.type"),
                    "type must be one of: predicate, choice, score",
                ))
            }
        };
        questions.push(Question {
            name,
            instructions,
            kind,
        });
    }
    Ok(DecisionRequest {
        model,
        state,
        images,
        questions,
        keep_alive: None,
    })
}

/// The `/v1/decisions` response: answers in question order.
pub fn render_decisions(model: &str, req: &DecisionRequest, out: &Outcome) -> Value {
    let answers: Vec<Value> = req
        .questions
        .iter()
        .zip(&out.answers)
        .filter_map(|(q, a)| {
            Some(match (&q.kind, a) {
                (Kind::Noul { .. }, Answer::Noul(p)) => {
                    json!({"type": "predicate", "name": q.name, "probability": p})
                }
                (Kind::Choice(options), Answer::Choice(p)) => json!({
                    "type": "choice",
                    "name": q.name,
                    "choice": options[argmax(p)].value,
                    "confidence": confidence(p),
                    "probabilities": options.iter().zip(p)
                        .map(|(o, p)| json!({"value": o.value, "probability": p}))
                        .collect::<Vec<_>>(),
                }),
                (Kind::Score(levels), Answer::Score(p)) => json!({
                    "type": "score",
                    "name": q.name,
                    "score": expected(p),
                    "confidence": confidence(p),
                    "probabilities": levels.iter().zip(p).enumerate()
                        .map(|(i, (l, p))| json!({"value": i, "label": l.label, "probability": p}))
                        .collect::<Vec<_>>(),
                }),
                _ => return None,
            })
        })
        .collect();
    json!({
        "model": model,
        "answers": answers,
        "usage": {
            "input_tokens": out.input_tokens,
            "output_tokens": out.output_tokens,
            "total_tokens": out.input_tokens + out.output_tokens,
        },
    })
}

// --- numbers ---

/// Index of the highest probability; a tie goes to the first option.
pub fn argmax(p: &[f64]) -> usize {
    let mut best = 0;
    for (i, v) in p.iter().enumerate() {
        if *v > p[best] {
            best = i;
        }
    }
    best
}

/// `1 - H(p) / ln(N)`: 0 when every candidate is as likely, near 1 when one dominates.
pub fn confidence(p: &[f64]) -> f64 {
    if p.len() < 2 {
        return 1.0;
    }
    let h: f64 = p.iter().filter(|v| **v > 0.0).map(|v| -v * v.ln()).sum();
    (1.0 - h / (p.len() as f64).ln()).clamp(0.0, 1.0)
}

/// Probability-weighted level index.
pub fn expected(p: &[f64]) -> f64 {
    p.iter().enumerate().map(|(i, v)| i as f64 * v).sum()
}

/// Scales to a sum of 1; `None` when nothing is positive.
pub fn normalize(p: Vec<f64>) -> Option<Vec<f64>> {
    let p: Vec<f64> = p
        .into_iter()
        .map(|v| if v.is_finite() && v > 0.0 { v } else { 0.0 })
        .collect();
    let sum: f64 = p.iter().sum();
    (sum > 0.0).then(|| p.into_iter().map(|v| v / sum).collect())
}
