//! Native decision models: llama-server's own `/v1/systemone` (llama.cpp README, "TypeSafe-
//! compatible API Endpoints"). Questions go upstream as `q0`, `q1`... so `/v1/decisions`
//! questions without a name, and names of any shape, map back by position.

use serde_json::{json, Map, Value};

use super::schema::{normalize, Answer, DecisionRequest, Kind, Outcome, Question};

fn key(i: usize) -> String {
    format!("q{i}")
}

/// The upstream request for the questions at `indices`.
pub fn request(req: &DecisionRequest, indices: &[usize]) -> Value {
    let mut questions = Map::new();
    for &i in indices {
        let q = &req.questions[i];
        let mut o = Map::new();
        let kind = match &q.kind {
            Kind::Noul { yes, no } => {
                let mut c = Map::new();
                if let Some(d) = no {
                    c.insert("false".into(), json!(d));
                }
                if let Some(d) = yes {
                    c.insert("true".into(), json!(d));
                }
                if !c.is_empty() {
                    o.insert("criteria".into(), Value::Object(c));
                }
                "noul"
            }
            Kind::Choice(options) => {
                let c: Map<String, Value> = options
                    .iter()
                    .map(|op| (op.key.clone(), json!(op.description)))
                    .collect();
                o.insert("criteria".into(), Value::Object(c));
                "choice"
            }
            Kind::Score(levels) => {
                let c: Vec<String> = levels.iter().map(|l| l.text()).collect();
                o.insert("criteria".into(), json!(c));
                "score"
            }
        };
        o.insert("type".into(), json!(kind));
        o.insert("instructions".into(), q.instructions.clone());
        questions.insert(key(i), Value::Object(o));
    }
    let mut body = json!({"state": req.state, "questions": questions});
    if !req.images.is_empty() {
        body["files"] = json!(req.images);
    }
    body
}

fn answer(q: &Question, a: &Value) -> Option<Answer> {
    let probs = a.get("probabilities");
    let at = |k: &str| {
        probs
            .and_then(|p| p.get(k))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    Some(match &q.kind {
        Kind::Noul { .. } => Answer::Noul(a.get("noul")?.as_f64()?.clamp(0.0, 1.0)),
        Kind::Choice(options) => {
            Answer::Choice(normalize(options.iter().map(|o| at(&o.key)).collect())?)
        }
        Kind::Score(levels) => Answer::Score(normalize(
            (0..levels.len()).map(|i| at(&i.to_string())).collect(),
        )?),
    })
}

/// The answers of a 200 response, in the order of `indices`; `Err` names the first question
/// the server left unanswered.
pub fn answers(
    req: &DecisionRequest,
    indices: &[usize],
    body: &Value,
) -> Result<(Vec<Answer>, u64, u64), String> {
    let mut out = Vec::with_capacity(indices.len());
    for &i in indices {
        let q = &req.questions[i];
        let a = body
            .get("answers")
            .and_then(|a| a.get(key(i)))
            .and_then(|a| answer(q, a))
            .ok_or_else(|| {
                format!(
                    "the decision model returned no answer for question {}",
                    q.name.as_deref().unwrap_or(&i.to_string())
                )
            })?;
        out.push(a);
    }
    let usage = |k: &str| body["usage"][k].as_u64().unwrap_or(0);
    Ok((out, usage("input_tokens"), usage("output_tokens")))
}

/// Merges answers computed in parts (upstream and local) back into question order.
pub fn merge(parts: Vec<(usize, Answer)>, total: usize) -> Vec<Answer> {
    let mut slots: Vec<Option<Answer>> = (0..total).map(|_| None).collect();
    for (i, a) in parts {
        slots[i] = Some(a);
    }
    slots.into_iter().flatten().collect()
}

impl Outcome {
    pub fn add_usage(&mut self, input: u64, output: u64) {
        self.input_tokens += input;
        self.output_tokens += output;
    }
}
