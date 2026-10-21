//! Task routes over the pipeline models (`/v1/hf`): `/v1/classifications`, `/v1/entities`,
//! `/v1/zero-shot`, `/v1/detections`, the OpenAI-shaped `/v1/moderations` and
//! `/v1/images/upscale` (super-resolution; the answer is an image in the shape of the images routes).
//!
//! Each route is a typed view of the same upstream call as `/v1/hf/{model}` (`harness-onnx` or
//! the Python engine, `POST /hf`): the request becomes a Hugging Face Inference API body, runs
//! through the ordinary proxy path (token policy `pipelines`, model scope, scheduling, request
//! records under the route the client called) and the answer is reshaped into one envelope
//! that does not depend on which engine served the model. Errors of the upstream pass through.

use axum::body::to_bytes;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use serde_json::{json, Map, Value};

use crate::errors::GatewayError;
use crate::{Api, GwState};

/// Texts of one classification, entity or moderation request (the engines' own limit).
pub const MAX_TEXTS: usize = 256;

/// Largest upstream answer reshaped here (label lists, and the one image of an upscale).
const MAX_ANSWER: usize = 64 << 20;

/// Default score from which a moderation category counts as flagged.
pub const DEFAULT_THRESHOLD: f64 = 0.5;

/// The task routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// `/v1/classifications`: text classification (sentiment, topic, intent...).
    Classify,
    /// `/v1/entities`: token classification (named entities, PII, part of speech).
    Entities,
    /// `/v1/zero-shot`: zero-shot classification against labels sent with the request.
    ZeroShot,
    /// `/v1/detections`: object detection in one image.
    Detect,
    /// `/v1/moderations`: OpenAI-shaped moderation by a text-classification model.
    Moderate,
    /// `/v1/images/upscale`: an image enlarged by a super-resolution model (Swin2SR).
    Upscale,
}

impl Task {
    pub fn endpoint(self) -> &'static str {
        match self {
            Task::Classify => "/v1/classifications",
            Task::Entities => "/v1/entities",
            Task::ZeroShot => "/v1/zero-shot",
            Task::Detect => "/v1/detections",
            Task::Moderate => "/v1/moderations",
            Task::Upscale => "/v1/images/upscale",
        }
    }
}

impl Task {
    /// The Hugging Face task of the models the route needs.
    pub fn model_task(self) -> &'static str {
        match self {
            Task::Classify | Task::Moderate => "text-classification",
            Task::Entities => "token-classification",
            Task::ZeroShot => "zero-shot-classification",
            Task::Detect => "object-detection",
            Task::Upscale => "image-super-resolution",
        }
    }
}

/// Largest image (as sent, before base64) an upscale request takes.
pub const MAX_UPSCALE_IMAGE_BYTES: usize = 20 * 1024 * 1024;

fn invalid(message: impl Into<String>, param: &str) -> GatewayError {
    GatewayError::new(400, "invalid_request", message).param(param)
}

/// One route's request.
pub async fn handle(
    state: GwState,
    task: Task,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    let (hf_body, request) = match request(task, &body) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    if let Some(e) = python_task_mismatch(&state, task, &headers, &request.model).await {
        return e.into_response();
    }
    let upstream =
        crate::proxy::handle_routed(state, Api::Hf, task.endpoint(), headers, hf_body).await;
    if upstream.status() != 200 {
        return upstream;
    }
    let (mut parts, body) = upstream.into_parts();
    let bytes = match to_bytes(body, MAX_ANSWER).await {
        Ok(b) => b,
        Err(e) => {
            return GatewayError::new(502, "upstream_error", e.to_string()).into_response();
        }
    };
    let model = parts
        .headers
        .get("x-harness-model")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| request.model.clone());
    let out = match serde_json::from_slice::<Value>(&bytes)
        .map_err(|e| e.to_string())
        .and_then(|v| shape(task, &request, &model, &v))
    {
        Ok(v) => v,
        Err(e) => {
            return GatewayError::new(
                400,
                "model_task_mismatch",
                format!(
                    "`{model}` did not answer like a model for the {} task ({e}); {} needs one.",
                    task.model_task(),
                    task.endpoint()
                ),
            )
            .param("model")
            .into_response();
        }
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Response::from_parts(
        parts,
        axum::body::Body::from(serde_json::to_vec(&out).unwrap_or_default()),
    )
}

/// The Python engine's tasks that answer `task`.
fn python_tasks(task: Task) -> &'static [&'static str] {
    match task {
        Task::Classify | Task::Moderate => &["text-classification"],
        Task::Entities => &["token-classification"],
        Task::ZeroShot => &["zero-shot-classification"],
        Task::Detect => &["object-detection"],
        Task::Upscale => &["image-super-resolution"],
    }
}

/// A Python engine model of another task than the route's (its worker would answer in the
/// shape of its own task, or fail inside the pipeline): refused up front. Only for callers
/// that may use the model; every other case goes on to the proxy, which answers it.
async fn python_task_mismatch(
    state: &GwState,
    task: Task,
    headers: &axum::http::HeaderMap,
    model: &str,
) -> Option<GatewayError> {
    let caller = crate::auth::check(
        state,
        headers,
        false,
        harness_core::token_policy::GatewayEndpoint::Pipelines,
    )
    .ok()?;
    let def = state.scheduler.resolve(Some(model)).await.ok()?;
    caller.check_model(&def.alias).ok()?;
    let actual = crate::pyhf::task_of(&def)?;
    (!python_tasks(task).contains(&actual.as_str())).then(|| {
        GatewayError::new(
            400,
            "model_task_mismatch",
            format!(
                "`{}` is a {actual} model; {} needs a model for the {} task.",
                def.alias,
                task.endpoint(),
                task.model_task()
            ),
        )
        .param("model")
    })
}

/// What the answer needs from the request.
#[derive(Debug, Clone)]
pub struct Request {
    pub model: String,
    /// Texts sent (one per input).
    pub inputs: usize,
    pub threshold: f64,
    /// `response_format: "url"` (a data URL instead of `b64_json`), upscale only.
    pub as_url: bool,
}

/// The `/hf` body of a route's request, and what the answer needs of it.
pub fn request(task: Task, body: &[u8]) -> Result<(Bytes, Request), GatewayError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| {
        GatewayError::new(
            400,
            "invalid_json",
            format!("The request body is not valid JSON: {e}"),
        )
    })?;
    let obj = v
        .as_object()
        .ok_or_else(|| invalid("the request body must be a JSON object", "body"))?;
    let model = obj
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| {
            invalid(
                format!(
                    "`model` is required: {} needs a {} model from your library",
                    task.endpoint(),
                    task.model_task()
                ),
                "model",
            )
        })?
        .to_string();
    let mut params = Map::new();
    let (inputs, count) = match task {
        Task::Detect => {
            let image = obj
                .get("image")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    invalid("`image` is required: a data URL or base64 image", "image")
                })?;
            if image.starts_with("http://") || image.starts_with("https://") {
                return Err(invalid(
                    "`image` must be a data URL or base64 text: the gateway does not fetch URLs",
                    "image",
                ));
            }
            if let Some(t) = obj.get("threshold").filter(|t| !t.is_null()) {
                let t = t
                    .as_f64()
                    .filter(|t| (0.0..=1.0).contains(t))
                    .ok_or_else(|| invalid("`threshold` must be between 0 and 1", "threshold"))?;
                params.insert("threshold".into(), json!(t));
            }
            (Value::String(image.to_string()), 1)
        }
        Task::Upscale => {
            let image = obj
                .get("image")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    invalid("`image` is required: a data URL or base64 image", "image")
                })?;
            if image.starts_with("http://") || image.starts_with("https://") {
                return Err(invalid(
                    "`image` must be a data URL or base64 text: the gateway does not fetch URLs",
                    "image",
                ));
            }
            if image.len() > MAX_UPSCALE_IMAGE_BYTES.div_ceil(3) * 4 + 64 {
                return Err(GatewayError::new(
                    413,
                    "invalid_request",
                    "`image` is larger than 20 MiB; make it smaller first",
                )
                .param("image"));
            }
            if let Some(scale) = obj.get("scale").filter(|t| !t.is_null()) {
                let scale = scale
                    .as_u64()
                    .filter(|s| (1..=8).contains(s))
                    .ok_or_else(|| {
                        invalid("`scale` must be a whole number from 1 to 8", "scale")
                    })?;
                params.insert("scale".into(), json!(scale));
            }
            (Value::String(image.to_string()), 1)
        }
        Task::ZeroShot => {
            let text = obj
                .get("input")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid("`input` must be one non-empty string", "input"))?;
            let labels = match obj.get("candidate_labels") {
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>(),
                Some(Value::String(s)) => s
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                _ => Vec::new(),
            };
            if labels.is_empty() {
                return Err(invalid(
                    "`candidate_labels` needs at least one label (an array or a comma-separated string)",
                    "candidate_labels",
                ));
            }
            params.insert("candidate_labels".into(), json!(labels));
            for key in ["multi_label", "hypothesis_template"] {
                if let Some(x) = obj.get(key).filter(|x| !x.is_null()) {
                    params.insert(key.into(), x.clone());
                }
            }
            (Value::String(text.to_string()), 1)
        }
        Task::Classify | Task::Entities | Task::Moderate => {
            let texts: Vec<Value> = match obj.get("input") {
                Some(Value::String(s)) if !s.trim().is_empty() => vec![json!(s)],
                Some(Value::Array(a))
                    if !a.is_empty()
                        && a.iter()
                            .all(|x| x.as_str().is_some_and(|s| !s.trim().is_empty())) =>
                {
                    a.clone()
                }
                _ => {
                    return Err(invalid(
                        "`input` must be a non-empty string or an array of non-empty strings",
                        "input",
                    ))
                }
            };
            if texts.len() > MAX_TEXTS {
                return Err(invalid(
                    format!(
                        "`input` has {} texts; at most {MAX_TEXTS} per request",
                        texts.len()
                    ),
                    "input",
                ));
            }
            match task {
                Task::Entities => {
                    if let Some(s) = obj.get("aggregation_strategy").filter(|x| !x.is_null()) {
                        params.insert("aggregation_strategy".into(), s.clone());
                    }
                }
                Task::Classify => {
                    if let Some(k) = obj.get("top_k").filter(|x| !x.is_null()) {
                        let k = k.as_u64().filter(|k| *k > 0).ok_or_else(|| {
                            invalid("`top_k` must be a positive integer", "top_k")
                        })?;
                        params.insert("top_k".into(), json!(k));
                    }
                }
                _ => {}
            }
            let n = texts.len();
            (Value::Array(texts), n)
        }
    };
    let threshold = match (task, obj.get("threshold")) {
        (Task::Moderate, Some(t)) if !t.is_null() => t
            .as_f64()
            .filter(|t| (0.0..=1.0).contains(t))
            .ok_or_else(|| invalid("`threshold` must be between 0 and 1", "threshold"))?,
        _ => DEFAULT_THRESHOLD,
    };
    let as_url = match obj.get("response_format").and_then(Value::as_str) {
        None | Some("b64_json") => false,
        Some("url") if task == Task::Upscale => true,
        Some(other) => {
            return Err(invalid(
                format!("`response_format` {other} is not supported; use b64_json or url"),
                "response_format",
            ))
        }
    };
    let mut hf = Map::new();
    hf.insert("model".into(), json!(model));
    hf.insert("inputs".into(), inputs);
    if !params.is_empty() {
        hf.insert("parameters".into(), Value::Object(params));
    }
    let hf = serde_json::to_vec(&Value::Object(hf))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?;
    Ok((
        Bytes::from(hf),
        Request {
            model,
            inputs: count,
            threshold,
            as_url,
        },
    ))
}

fn num(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(0.0)
}

/// `[{label, score}]` of one model answer row.
fn label_row(row: &Value) -> Result<Vec<(String, f64)>, String> {
    row.as_array()
        .ok_or_else(|| "expected a list of labels".to_string())?
        .iter()
        .map(|o| {
            Ok((
                o.get("label")
                    .and_then(Value::as_str)
                    .ok_or("a label is missing")?
                    .to_string(),
                num(o.get("score")),
            ))
        })
        .collect()
}

/// Rows of label scores, one per input: the engines answer a list of lists (one list per text),
/// or one flat list for a single text.
fn label_rows(answer: &Value, inputs: usize) -> Result<Vec<Vec<(String, f64)>>, String> {
    let list = answer
        .as_array()
        .ok_or("expected a list of labels per text")?;
    let rows = if list.first().is_some_and(Value::is_array) {
        list.iter().map(label_row).collect::<Result<Vec<_>, _>>()?
    } else if inputs == 1 {
        vec![label_row(answer)?]
    } else {
        return Err("expected one list of labels per text".into());
    };
    if rows.len() != inputs {
        return Err(format!("{} answers for {inputs} texts", rows.len()));
    }
    Ok(rows)
}

fn labels_json(row: &[(String, f64)]) -> Value {
    let mut row = row.to_vec();
    row.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Value::Array(
        row.into_iter()
            .map(|(label, score)| json!({"label": label, "score": score}))
            .collect(),
    )
}

/// Entities of one text: the Inference API's `entity_group` (grouped) or `entity` (per token).
fn entity_row(row: &Value) -> Result<Value, String> {
    let list = row
        .as_array()
        .ok_or("expected a list of entities per text")?;
    list.iter()
        .map(|e| {
            let label = e
                .get("entity_group")
                .or_else(|| e.get("entity"))
                .and_then(Value::as_str)
                .ok_or("an entity has no label")?;
            Ok(json!({
                "label": label,
                "text": e.get("word").and_then(Value::as_str).unwrap_or_default(),
                "score": num(e.get("score")),
                "start": e.get("start").cloned().unwrap_or(Value::Null),
                "end": e.get("end").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::Array)
}

/// The route's response envelope for the model's `answer`.
pub fn shape(task: Task, request: &Request, model: &str, answer: &Value) -> Result<Value, String> {
    match task {
        Task::Classify => {
            let rows = label_rows(answer, request.inputs)?;
            Ok(json!({
                "object": "list",
                "model": model,
                "data": rows.iter().enumerate().map(|(i, r)| json!({
                    "object": "classification", "index": i, "labels": labels_json(r),
                })).collect::<Vec<_>>(),
            }))
        }
        Task::ZeroShot => {
            let labels = answer
                .get("labels")
                .and_then(Value::as_array)
                .ok_or("expected labels and scores")?;
            let scores = answer
                .get("scores")
                .and_then(Value::as_array)
                .ok_or("expected labels and scores")?;
            let row: Vec<(String, f64)> = labels
                .iter()
                .zip(scores)
                .filter_map(|(l, s)| Some((l.as_str()?.to_string(), s.as_f64()?)))
                .collect();
            Ok(json!({
                "object": "list",
                "model": model,
                "data": [{"object": "classification", "index": 0, "labels": labels_json(&row)}],
            }))
        }
        Task::Entities => {
            let list = answer.as_array().ok_or("expected a list of entities")?;
            let rows: Vec<&Value> = if list.first().is_some_and(Value::is_array) {
                list.iter().collect()
            } else if request.inputs == 1 {
                vec![answer]
            } else {
                return Err("expected one list of entities per text".into());
            };
            if rows.len() != request.inputs {
                return Err(format!(
                    "{} answers for {} texts",
                    rows.len(),
                    request.inputs
                ));
            }
            let data = rows
                .into_iter()
                .enumerate()
                .map(|(i, r)| {
                    Ok(json!({"object": "entities", "index": i, "entities": entity_row(r)?}))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(json!({"object": "list", "model": model, "data": data}))
        }
        Task::Detect => {
            let list = answer.as_array().ok_or("expected a list of detections")?;
            let data = list
                .iter()
                .map(|d| {
                    let boxed = d.get("box").filter(|b| b.is_object());
                    Ok(json!({
                        "object": "detection",
                        "label": d.get("label").and_then(Value::as_str).ok_or("a detection has no label")?,
                        "score": num(d.get("score")),
                        "box": boxed.ok_or("a detection has no box")?,
                    }))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(json!({"object": "list", "model": model, "data": data}))
        }
        Task::Upscale => {
            let image = answer
                .get("image")
                .and_then(Value::as_str)
                .filter(|i| !i.is_empty())
                .ok_or("expected an image")?;
            let dim = |k: &str| answer.get(k).and_then(Value::as_u64);
            let item = if request.as_url {
                json!({"url": format!("data:image/png;base64,{image}")})
            } else {
                json!({"b64_json": image})
            };
            Ok(json!({
                "created": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs()),
                "model": model,
                "data": [item],
                "width": dim("width"),
                "height": dim("height"),
                "scale": dim("scale"),
            }))
        }
        Task::Moderate => {
            let rows = label_rows(answer, request.inputs)?;
            Ok(json!({
                "id": format!("modr-{}", &crate::proxy::request_id()[4..]),
                "model": model,
                "results": rows.iter().map(|r| moderation_result(r, request.threshold)).collect::<Vec<_>>(),
            }))
        }
    }
}

/// Labels that mean "nothing to flag" (the model's own all-clear class).
fn is_clear(label: &str) -> bool {
    matches!(
        normalize(label).as_str(),
        "ok" | "neutral"
            | "non_toxic"
            | "nontoxic"
            | "not_toxic"
            | "non_offensive"
            | "not_offensive"
            | "safe"
            | "clean"
            | "normal"
            | "not_hate"
            | "non_hate"
            | "nothate"
            | "acceptable"
            | "benign"
            | "none"
    )
}

fn normalize(label: &str) -> String {
    label
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' ', '/'], "_")
}

/// The OpenAI moderation category of a model label, else the label itself (lower case,
/// underscores). Labels of the common toxicity and moderation models map to OpenAI's names.
pub fn category_of(label: &str) -> String {
    match normalize(label).as_str() {
        "hate" | "identity_hate" | "identity_attack" | "h" => "hate".into(),
        "hate_threatening" | "h2" => "hate/threatening".into(),
        "harassment" | "insult" | "hr" => "harassment".into(),
        "threat" | "threatening" | "harassment_threatening" => "harassment/threatening".into(),
        "sexual" | "obscene" | "sexual_explicit" | "s" => "sexual".into(),
        "sexual_minors" | "s3" => "sexual/minors".into(),
        "violence" | "v" => "violence".into(),
        "violence_graphic" | "v2" => "violence/graphic".into(),
        "self_harm" | "sh" => "self-harm".into(),
        other => other.to_string(),
    }
}

/// One text's moderation result: category scores (the highest of the labels that map to a
/// category), categories at or above `threshold`, and `flagged` when any is.
pub fn moderation_result(row: &[(String, f64)], threshold: f64) -> Value {
    let mut scores: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for (label, score) in row.iter().filter(|(l, _)| !is_clear(l)) {
        let slot = scores.entry(category_of(label)).or_insert(0.0);
        if *score > *slot {
            *slot = *score;
        }
    }
    let categories: Map<String, Value> = scores
        .iter()
        .map(|(k, s)| (k.clone(), json!(*s >= threshold)))
        .collect();
    let flagged = categories.values().any(|v| v == &json!(true));
    json!({
        "flagged": flagged,
        "categories": categories,
        "category_scores": scores,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(task: Task, body: Value) -> Result<(Value, Request), GatewayError> {
        request(task, body.to_string().as_bytes())
            .map(|(b, r)| (serde_json::from_slice(&b).unwrap(), r))
    }

    #[test]
    fn classification_requests_become_hf_bodies() {
        let (hf, r) = parsed(
            Task::Classify,
            json!({"model": "sst2", "input": "great", "top_k": 2}),
        )
        .unwrap();
        assert_eq!(
            hf,
            json!({"model": "sst2", "inputs": ["great"], "parameters": {"top_k": 2}})
        );
        assert_eq!(r.inputs, 1);
        let (hf, r) = parsed(Task::Entities, json!({"model": "ner", "input": ["a", "b"]})).unwrap();
        assert_eq!(hf, json!({"model": "ner", "inputs": ["a", "b"]}));
        assert_eq!(r.inputs, 2);
        let (hf, _) = parsed(
            Task::ZeroShot,
            json!({"model": "nli", "input": "x", "candidate_labels": "a, b", "multi_label": true}),
        )
        .unwrap();
        assert_eq!(
            hf,
            json!({"model": "nli", "inputs": "x",
                   "parameters": {"candidate_labels": ["a", "b"], "multi_label": true}})
        );
        let (hf, _) = parsed(
            Task::Detect,
            json!({"model": "detr", "image": "data:image/png;base64,AQID", "threshold": 0.7}),
        )
        .unwrap();
        assert_eq!(
            hf,
            json!({"model": "detr", "inputs": "data:image/png;base64,AQID",
                   "parameters": {"threshold": 0.7}})
        );
    }

    #[test]
    fn bad_requests_name_the_field() {
        let err = |task, body: Value| {
            let e = parsed(task, body).unwrap_err();
            (e.status, e.param.map(|p| p.to_string()))
        };
        assert_eq!(
            err(Task::Classify, json!({"input": "x"})),
            (400, Some("model".into()))
        );
        assert_eq!(
            err(Task::Classify, json!({"model": "m", "input": []})),
            (400, Some("input".into()))
        );
        assert_eq!(
            err(
                Task::Classify,
                json!({"model": "m", "input": "x", "top_k": 0})
            ),
            (400, Some("top_k".into()))
        );
        assert_eq!(
            err(Task::ZeroShot, json!({"model": "m", "input": "x"})),
            (400, Some("candidate_labels".into()))
        );
        assert_eq!(
            err(
                Task::ZeroShot,
                json!({"model": "m", "input": ["x"], "candidate_labels": ["a"]})
            ),
            (400, Some("input".into()))
        );
        assert_eq!(
            err(
                Task::Detect,
                json!({"model": "m", "image": "https://example.com/a.png"})
            ),
            (400, Some("image".into()))
        );
        assert_eq!(
            err(
                Task::Moderate,
                json!({"model": "m", "input": "x", "threshold": 2})
            ),
            (400, Some("threshold".into()))
        );
        let big = vec!["x"; MAX_TEXTS + 1];
        assert_eq!(
            err(Task::Moderate, json!({"model": "m", "input": big})),
            (400, Some("input".into()))
        );
        assert_eq!(
            request(Task::Classify, b"not json").unwrap_err().code,
            "invalid_json"
        );
    }

    #[test]
    fn labels_come_back_best_first_for_nested_and_flat_answers() {
        let req = Request {
            model: "m".into(),
            inputs: 1,
            threshold: 0.5,
            as_url: false,
        };
        let nested = json!([[{"label": "NEG", "score": 0.1}, {"label": "POS", "score": 0.9}]]);
        let flat = json!([{"label": "NEG", "score": 0.1}, {"label": "POS", "score": 0.9}]);
        for answer in [nested, flat] {
            let out = shape(Task::Classify, &req, "alias", &answer).unwrap();
            assert_eq!(out["model"], "alias");
            assert_eq!(out["data"][0]["labels"][0]["label"], "POS");
            assert_eq!(out["data"][0]["index"], 0);
        }
        let two = Request {
            inputs: 2,
            ..req.clone()
        };
        assert!(shape(
            Task::Classify,
            &two,
            "a",
            &json!([{"label": "x", "score": 1.0}])
        )
        .is_err());
    }

    #[test]
    fn entities_and_zero_shot_and_detections_are_reshaped() {
        let req = Request {
            model: "m".into(),
            inputs: 1,
            threshold: 0.5,
            as_url: false,
        };
        let ner =
            json!([{"entity_group": "PER", "word": "Ada", "score": 0.99, "start": 0, "end": 3}]);
        let out = shape(Task::Entities, &req, "ner", &ner).unwrap();
        assert_eq!(out["data"][0]["entities"][0]["label"], "PER");
        assert_eq!(out["data"][0]["entities"][0]["text"], "Ada");
        let per_token =
            json!([[{"entity": "B-LOC", "word": "Paris", "score": 0.9, "start": 5, "end": 10}]]);
        let out = shape(Task::Entities, &req, "ner", &per_token).unwrap();
        assert_eq!(out["data"][0]["entities"][0]["label"], "B-LOC");

        let zs = json!({"sequence": "s", "labels": ["b", "a"], "scores": [0.2, 0.8]});
        let out = shape(Task::ZeroShot, &req, "nli", &zs).unwrap();
        assert_eq!(out["data"][0]["labels"][0]["label"], "a");

        let det = json!([{"label": "cat", "score": 0.9, "box": {"xmin": 1, "ymin": 2, "xmax": 3, "ymax": 4}}]);
        let out = shape(Task::Detect, &req, "detr", &det).unwrap();
        assert_eq!(out["data"][0]["box"]["xmax"], 3);
        assert_eq!(out["data"][0]["object"], "detection");
    }

    #[test]
    fn moderation_maps_toxicity_labels_to_openai_categories() {
        let toxic_bert = [
            ("toxic", 0.97),
            ("severe_toxic", 0.2),
            ("obscene", 0.9),
            ("threat", 0.01),
            ("insult", 0.8),
            ("identity_hate", 0.05),
        ]
        .map(|(l, s)| (l.to_string(), s));
        let r = moderation_result(&toxic_bert, 0.5);
        assert_eq!(r["flagged"], true);
        assert_eq!(r["categories"]["toxic"], true);
        assert_eq!(r["categories"]["sexual"], true);
        assert_eq!(r["categories"]["harassment"], true);
        assert_eq!(r["categories"]["harassment/threatening"], false);
        assert_eq!(r["categories"]["hate"], false);
        assert_eq!(r["category_scores"]["harassment"], 0.8);
        // The model's all-clear class is no category; a clean text is not flagged.
        let clean = [("non-toxic".to_string(), 0.99), ("toxic".to_string(), 0.01)];
        let r = moderation_result(&clean, 0.5);
        assert_eq!(r["flagged"], false);
        assert!(r["categories"].get("non_toxic").is_none());
        assert_eq!(r["category_scores"]["toxic"], 0.01);
        // A custom threshold decides.
        assert_eq!(moderation_result(&clean, 0.005)["flagged"], true);
        // KoalaAI's codes.
        let koala = [
            ("OK".to_string(), 0.1),
            ("SH".to_string(), 0.9),
            ("V2".to_string(), 0.7),
        ];
        let r = moderation_result(&koala, 0.5);
        assert_eq!(r["categories"]["self-harm"], true);
        assert_eq!(r["categories"]["violence/graphic"], true);
    }

    #[test]
    fn answers_of_another_task_are_refused() {
        let req = Request {
            model: "m".into(),
            inputs: 1,
            threshold: 0.5,
            as_url: false,
        };
        let labels = json!([[{"label": "POS", "score": 0.9}]]);
        let entities =
            json!([{"entity_group": "PER", "word": "Ada", "score": 0.9, "start": 0, "end": 3}]);
        assert!(shape(Task::Entities, &req, "m", &labels).is_err());
        assert!(shape(Task::Classify, &req, "m", &entities).is_err());
        assert!(shape(Task::Moderate, &req, "m", &entities).is_err());
        assert!(shape(Task::Detect, &req, "m", &labels[0]).is_err());
        assert!(shape(Task::ZeroShot, &req, "m", &labels).is_err());
    }

    #[test]
    fn moderation_envelope_has_one_result_per_text() {
        let req = Request {
            model: "tox".into(),
            inputs: 2,
            threshold: 0.5,
            as_url: false,
        };
        let answer = json!([
            [{"label": "toxic", "score": 0.9}, {"label": "non-toxic", "score": 0.1}],
            [{"label": "toxic", "score": 0.1}, {"label": "non-toxic", "score": 0.9}]
        ]);
        let out = shape(Task::Moderate, &req, "tox", &answer).unwrap();
        assert!(out["id"].as_str().unwrap().starts_with("modr-"));
        assert_eq!(out["results"].as_array().unwrap().len(), 2);
        assert_eq!(out["results"][0]["flagged"], true);
        assert_eq!(out["results"][1]["flagged"], false);
    }

    #[test]
    fn an_upscale_request_becomes_an_hf_body_and_its_answer_an_image_list() {
        let (hf, req) = request(
            Task::Upscale,
            br#"{"model": "swin", "image": "data:image/png;base64,AQID", "scale": 2,
                "response_format": "url"}"#,
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&hf).unwrap();
        assert_eq!(v["model"], "swin");
        assert_eq!(v["inputs"], "data:image/png;base64,AQID");
        assert_eq!(v["parameters"], json!({"scale": 2}));
        assert!(req.as_url);
        let answer = json!({"image": "QUJD", "width": 40, "height": 24, "scale": 2});
        let out = shape(Task::Upscale, &req, "alias", &answer).unwrap();
        assert_eq!(out["data"][0]["url"], "data:image/png;base64,QUJD");
        assert_eq!(
            (
                out["width"].as_u64(),
                out["height"].as_u64(),
                out["scale"].as_u64()
            ),
            (Some(40), Some(24), Some(2))
        );
        assert_eq!(out["model"], "alias");
        let (_, plain) = request(Task::Upscale, br#"{"model": "m", "image": "AQID"}"#).unwrap();
        assert!(!plain.as_url);
        let out = shape(Task::Upscale, &plain, "m", &answer).unwrap();
        assert_eq!(out["data"][0]["b64_json"], "QUJD");
        // Another task's answer is not an image.
        assert!(shape(Task::Upscale, &plain, "m", &json!([{"label": "x"}])).is_err());
        let err = |body: Value| {
            let e = request(Task::Upscale, body.to_string().as_bytes()).unwrap_err();
            (e.status, e.param)
        };
        assert_eq!(err(json!({"model": "m"})), (400, Some("image".into())));
        assert_eq!(
            err(json!({"model": "m", "image": "http://example.com/a.png"})),
            (400, Some("image".into()))
        );
        assert_eq!(
            err(json!({"model": "m", "image": "AQID", "scale": 9})),
            (400, Some("scale".into()))
        );
        assert_eq!(
            err(json!({"model": "m", "image": "AQID", "response_format": "xml"})),
            (400, Some("response_format".into()))
        );
        // Only the upscale route answers a data URL; the other typed routes know b64_json only.
        assert_eq!(
            err(json!({"model": "m", "input": "x", "response_format": "url"})).0,
            400
        );
        let huge = "A".repeat(MAX_UPSCALE_IMAGE_BYTES * 2);
        assert_eq!(
            err(json!({"model": "m", "image": huge})),
            (413, Some("image".into()))
        );
    }

    #[test]
    fn python_tasks_are_tasks_of_the_worker() {
        for task in [
            Task::Classify,
            Task::Entities,
            Task::ZeroShot,
            Task::Detect,
            Task::Moderate,
            Task::Upscale,
        ] {
            assert!(python_tasks(task).contains(&task.model_task()));
            for t in python_tasks(task) {
                assert!(harness_core::engine::python_hf::TASKS.contains(t), "{t}");
            }
        }
    }

    #[test]
    fn routes_are_pipeline_scoped() {
        for task in [
            Task::Classify,
            Task::Entities,
            Task::ZeroShot,
            Task::Detect,
            Task::Moderate,
            Task::Upscale,
        ] {
            assert_eq!(
                harness_core::token_policy::GatewayEndpoint::of_path(task.endpoint()),
                Some(harness_core::token_policy::GatewayEndpoint::Pipelines)
            );
        }
    }
}
