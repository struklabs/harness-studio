//! `POST /hf`: the Hugging Face Inference API shape for the encoder task heads, so the gateway
//! (`/v1/hf/{model}`) and the Playground speak one format with every engine that serves a
//! `pipeline` task. The body is `{"inputs": ..., "parameters": {...}}`; the answer is what the
//! Inference API returns for the task. Pure: requests become [`HfJob`]s, results become JSON.

use crate::api::ApiError;
use crate::package::Task;
use crate::tasks::{Answer, Entity, LabelScore};
use serde_json::{json, Value};

/// Hypothesis of a zero-shot request without its own (the transformers default).
pub const DEFAULT_HYPOTHESIS: &str = "This example is {}.";
/// Mask fills returned when the request does not say (the transformers default).
pub const DEFAULT_FILLS: usize = 5;

/// What one request asks of the model.
#[derive(Debug, Clone, PartialEq)]
pub enum HfJob {
    /// Texts, optional pair texts, `top_k` (`None`: every label).
    Classify {
        texts: Vec<String>,
        pairs: Option<Vec<String>>,
        top_k: Option<usize>,
        /// `inputs` was one text: answer with one list.
        single: bool,
    },
    ZeroShot {
        text: String,
        candidates: Vec<String>,
        template: String,
        multi_label: bool,
    },
    Tokens {
        texts: Vec<String>,
        group: bool,
        single: bool,
    },
    FillMask {
        text: String,
        top_k: usize,
    },
    Answer {
        question: String,
        context: String,
        top_k: usize,
    },
}

fn invalid(msg: impl Into<String>) -> ApiError {
    ApiError::invalid(msg)
}

/// `inputs` as texts: a string or an array of strings; `single` when it was one string.
fn texts_of(inputs: &Value) -> Result<(Vec<String>, bool), ApiError> {
    match inputs {
        Value::String(s) => Ok((vec![s.clone()], true)),
        Value::Array(a) if !a.is_empty() => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or_else(|| invalid("inputs must be a string or an array of strings"))
            })
            .collect::<Result<_, _>>()
            .map(|t| (t, false)),
        _ => Err(invalid(
            "inputs must be a non-empty string or array of strings",
        )),
    }
}

fn usize_param(p: &Value, name: &str) -> Result<Option<usize>, ApiError> {
    match p.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| *n > 0)
            .map(|n| Some(n as usize))
            .ok_or_else(|| invalid(format!("parameters.{name} must be a positive integer"))),
    }
}

/// The candidate labels of a zero-shot request: an array, or one comma-separated string.
fn candidates_of(v: &Value) -> Result<Vec<String>, ApiError> {
    let list: Vec<String> = match v {
        Value::String(s) => s.split(',').map(|x| x.trim().to_string()).collect(),
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .map(|x| x.trim().to_string())
            .collect(),
        _ => Vec::new(),
    };
    let list: Vec<String> = list.into_iter().filter(|x| !x.is_empty()).collect();
    if list.is_empty() {
        return Err(invalid(
            "parameters.candidate_labels needs at least one label",
        ));
    }
    Ok(list)
}

/// Reads a request body for a model of `task`.
pub fn parse(task: Task, body: &Value) -> Result<HfJob, ApiError> {
    let inputs = body
        .get("inputs")
        .ok_or_else(|| invalid("the request needs inputs"))?;
    let params = body.get("parameters").cloned().unwrap_or(Value::Null);
    match task {
        Task::Classify | Task::Rerank => {
            if let Some(c) = params.get("candidate_labels") {
                let text = inputs
                    .as_str()
                    .ok_or_else(|| invalid("zero-shot inputs must be one string"))?
                    .to_string();
                return Ok(HfJob::ZeroShot {
                    text,
                    candidates: candidates_of(c)?,
                    template: params
                        .get("hypothesis_template")
                        .and_then(Value::as_str)
                        .unwrap_or(DEFAULT_HYPOTHESIS)
                        .to_string(),
                    multi_label: params
                        .get("multi_label")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
            let top_k = usize_param(&params, "top_k")?;
            // `{"text": ..., "text_pair": ...}`: one pair.
            if let Some(obj) = inputs.as_object() {
                let text = obj
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("inputs.text must be a string"))?;
                let pair = obj.get("text_pair").and_then(Value::as_str);
                return Ok(HfJob::Classify {
                    texts: vec![text.to_string()],
                    pairs: pair.map(|p| vec![p.to_string()]),
                    top_k,
                    single: true,
                });
            }
            let (texts, single) = texts_of(inputs)?;
            Ok(HfJob::Classify {
                texts,
                pairs: None,
                top_k,
                single,
            })
        }
        Task::TokenClassify => {
            let (texts, single) = texts_of(inputs)?;
            let strategy = params
                .get("aggregation_strategy")
                .and_then(Value::as_str)
                .unwrap_or("simple");
            let group = match strategy {
                "none" => false,
                "simple" | "first" | "average" | "max" => true,
                other => {
                    return Err(invalid(format!(
                        "unknown aggregation_strategy {other:?} (none, simple, first, average, max)"
                    )))
                }
            };
            Ok(HfJob::Tokens {
                texts,
                group,
                single,
            })
        }
        Task::FillMask => Ok(HfJob::FillMask {
            text: inputs
                .as_str()
                .ok_or_else(|| invalid("fill-mask inputs must be one string"))?
                .to_string(),
            top_k: usize_param(&params, "top_k")?.unwrap_or(DEFAULT_FILLS),
        }),
        Task::QuestionAnswering => {
            let field = |name: &str| {
                inputs
                    .get(name)
                    .and_then(Value::as_str)
                    .map(String::from)
                    .ok_or_else(|| invalid(format!("inputs.{name} must be a string")))
            };
            Ok(HfJob::Answer {
                question: field("question")?,
                context: field("context")?,
                top_k: usize_param(&params, "top_k")?.unwrap_or(1),
            })
        }
        Task::Embedding => Err(ApiError::unavailable(
            "this model is an embedding model: use /v1/embeddings",
        )),
    }
}

pub fn label_scores(list: &[LabelScore]) -> Value {
    Value::Array(
        list.iter()
            .map(|l| json!({"label": l.label, "score": l.score}))
            .collect(),
    )
}

/// `text-classification`: one list per text (the Inference API answers a single string with
/// a list holding one list too).
pub fn classify_response(rows: &[Vec<LabelScore>]) -> Value {
    Value::Array(rows.iter().map(|r| label_scores(r)).collect())
}

pub fn zero_shot_response(text: &str, scores: &[LabelScore]) -> Value {
    json!({
        "sequence": text,
        "labels": scores.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
        "scores": scores.iter().map(|s| s.score).collect::<Vec<_>>(),
    })
}

/// `token-classification`: grouped entities carry `entity_group`, single tokens `entity`.
pub fn entities(list: &[Entity], grouped: bool) -> Value {
    let key = if grouped { "entity_group" } else { "entity" };
    Value::Array(
        list.iter()
            .map(|e| {
                json!({key: e.entity, "score": e.score, "word": e.word, "start": e.start, "end": e.end})
            })
            .collect(),
    )
}

pub fn tokens_response(per_text: &[Vec<Entity>], grouped: bool, single: bool) -> Value {
    if single {
        entities(per_text.first().map_or(&[][..], |v| v), grouped)
    } else {
        Value::Array(per_text.iter().map(|e| entities(e, grouped)).collect())
    }
}

pub fn fill_mask_response(fills: &[(String, u32, f32, String)]) -> Value {
    Value::Array(
        fills
            .iter()
            .map(|(token_str, token, score, sequence)| {
                json!({"score": score, "token": token, "token_str": token_str, "sequence": sequence})
            })
            .collect(),
    )
}

/// `question-answering`: one answer object, or a list when more than one was asked for.
pub fn answer_response(answers: &[Answer], top_k: usize) -> Value {
    let one =
        |a: &Answer| json!({"answer": a.answer, "score": a.score, "start": a.start, "end": a.end});
    if top_k <= 1 {
        answers
            .first()
            .map(one)
            .unwrap_or_else(|| json!({"answer": "", "score": 0.0, "start": 0, "end": 0}))
    } else {
        Value::Array(answers.iter().map(one).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_requests() {
        let j = parse(Task::Classify, &json!({"inputs": "great movie"})).unwrap();
        assert_eq!(
            j,
            HfJob::Classify {
                texts: vec!["great movie".into()],
                pairs: None,
                top_k: None,
                single: true
            }
        );
        let j = parse(
            Task::Classify,
            &json!({"inputs": {"text": "a", "text_pair": "b"}, "parameters": {"top_k": 1}}),
        )
        .unwrap();
        assert!(matches!(
            j,
            HfJob::Classify {
                pairs: Some(_),
                top_k: Some(1),
                ..
            }
        ));
        assert!(parse(Task::Classify, &json!({"inputs": 3})).is_err());
        assert!(parse(
            Task::Classify,
            &json!({"inputs": "x", "parameters": {"top_k": 0}})
        )
        .is_err());
    }

    #[test]
    fn zero_shot_requests_need_candidates() {
        let j = parse(
            Task::Classify,
            &json!({"inputs": "I love football", "parameters": {"candidate_labels": "sports, politics"}}),
        )
        .unwrap();
        let HfJob::ZeroShot {
            candidates,
            template,
            multi_label,
            ..
        } = j
        else {
            panic!()
        };
        assert_eq!(candidates, vec!["sports", "politics"]);
        assert_eq!(template, DEFAULT_HYPOTHESIS);
        assert!(!multi_label);
        assert!(parse(
            Task::Classify,
            &json!({"inputs": "x", "parameters": {"candidate_labels": []}})
        )
        .is_err());
    }

    #[test]
    fn token_fill_and_qa_requests() {
        assert!(matches!(
            parse(
                Task::TokenClassify,
                &json!({"inputs": "x", "parameters": {"aggregation_strategy": "none"}})
            )
            .unwrap(),
            HfJob::Tokens {
                group: false,
                single: true,
                ..
            }
        ));
        assert!(parse(
            Task::TokenClassify,
            &json!({"inputs": "x", "parameters": {"aggregation_strategy": "weird"}})
        )
        .is_err());
        assert_eq!(
            parse(
                Task::FillMask,
                &json!({"inputs": "Paris is the [MASK] of France."})
            )
            .unwrap(),
            HfJob::FillMask {
                text: "Paris is the [MASK] of France.".into(),
                top_k: DEFAULT_FILLS
            }
        );
        assert!(matches!(
            parse(
                Task::QuestionAnswering,
                &json!({"inputs": {"question": "q", "context": "c"}})
            )
            .unwrap(),
            HfJob::Answer { top_k: 1, .. }
        ));
        assert!(parse(Task::QuestionAnswering, &json!({"inputs": "q"})).is_err());
        assert_eq!(
            parse(Task::Embedding, &json!({"inputs": "x"}))
                .unwrap_err()
                .status,
            501
        );
    }

    #[test]
    fn responses_follow_the_inference_api() {
        let ls = vec![LabelScore {
            label: "POSITIVE".into(),
            score: 0.9,
        }];
        assert_eq!(
            classify_response(std::slice::from_ref(&ls))[0][0]["label"],
            "POSITIVE"
        );
        let z = zero_shot_response("t", &ls);
        assert_eq!(z["labels"][0], "POSITIVE");
        let e = Entity {
            entity: "PER".into(),
            score: 0.99,
            word: "Wolfgang".into(),
            start: 0,
            end: 8,
        };
        assert_eq!(
            tokens_response(&[vec![e.clone()]], true, true)[0]["entity_group"],
            "PER"
        );
        assert_eq!(
            tokens_response(&[vec![e]], false, false)[0][0]["entity"],
            "PER"
        );
        let a = Answer {
            answer: "Paris".into(),
            score: 0.8,
            start: 3,
            end: 8,
        };
        assert_eq!(
            answer_response(std::slice::from_ref(&a), 1)["answer"],
            "Paris"
        );
        assert!(answer_response(&[a], 2).is_array());
    }
}
