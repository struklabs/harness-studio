//! Inference presets: request-side settings (system prompt, sampling, reasoning, output
//! format). Field names are llama-server's request parameter names, so
//! [`InferencePreset::request_params`] is a plain copy into the request body.

use crate::error_key::err;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Sampling parameters (llama-server `/completion` and `/v1/chat/completions` names).
/// `None` / empty = not sent (the server's or the model's default applies).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SamplingParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typical_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_n_sigma: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_last_n: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_base: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_allowed_length: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_penalty_last_n: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dry_sequence_breakers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xtc_probability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xtc_threshold: Option<f64>,
    /// 0 = off, 1 = Mirostat, 2 = Mirostat 2.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirostat: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirostat_tau: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirostat_eta: Option<f64>,
    /// Dynamic temperature: temperature varies by +-range with the token entropy (0 = off).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynatemp_range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynatemp_exponent: Option<f64>,
    /// Adaptive-p: target probability of the picked token (negative / unset = off).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive_target: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive_decay: Option<f64>,
    /// Return per-token log probabilities (`choices[].logprobs.content`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    /// Alternatives per token in `logprobs` (0-20).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// OpenAI name; llama-server also accepts `n_predict`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    /// Sampler order (`["top_k", "top_p", "min_p", "temperature"]`...).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samplers: Vec<String>,
    /// `{"<token id or text>": bias}` or llama-server's `[[token, bias]]` form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<Value>,
    /// GBNF grammar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammar: Option<String>,
    /// Constrain output to a JSON schema (llama-server `json_schema`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<Value>,
}

/// Reasoning ("thinking") request settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReasoningSettings {
    /// `chat_template_kwargs.enable_thinking` (templates that support it, e.g. Qwen3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `reasoning_format`: none | deepseek | deepseek-legacy | auto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// `reasoning_effort` (gpt-oss): low | medium | high.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// `thinking_budget_tokens` (-1 = unlimited, 0 = off).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<i64>,
}

impl ReasoningSettings {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InferencePreset {
    pub id: String,
    pub name: String,
    /// Shipped with the app; read-only (copy it to change it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builtin: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub sampling: SamplingParams,
    #[serde(default, skip_serializing_if = "ReasoningSettings::is_empty")]
    pub reasoning: ReasoningSettings,
    /// OpenAI `response_format` (`{"type":"json_object"}` / `json_schema`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    /// OpenAI `tool_choice` (`"auto"`, `"none"`, `"required"` or a function).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

pub const BUILTIN_PREFIX: &str = "builtin:";

impl InferencePreset {
    fn blank(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            builtin: false,
            description: String::new(),
            system_prompt: None,
            sampling: SamplingParams::default(),
            reasoning: ReasoningSettings::default(),
            response_format: None,
            tool_choice: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// Request body fields of this preset (the system prompt is a message, not a field, so it
    /// is left to the caller).
    pub fn request_params(&self) -> Map<String, Value> {
        let mut out = match serde_json::to_value(&self.sampling) {
            Ok(Value::Object(m)) => m,
            _ => Map::new(),
        };
        let r = &self.reasoning;
        if let Some(f) = &r.format {
            out.insert("reasoning_format".into(), f.clone().into());
        }
        if let Some(e) = &r.effort {
            out.insert("reasoning_effort".into(), e.clone().into());
        }
        if let Some(b) = r.budget {
            out.insert("thinking_budget_tokens".into(), b.into());
        }
        if let Some(enabled) = r.enabled {
            out.insert(
                "chat_template_kwargs".into(),
                serde_json::json!({ "enable_thinking": enabled }),
            );
        }
        if let Some(v) = &self.response_format {
            out.insert("response_format".into(), v.clone());
        }
        if let Some(v) = &self.tool_choice {
            out.insert("tool_choice".into(), v.clone());
        }
        out
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err(err("presets.name_empty").into());
        }
        let s = &self.sampling;
        let range = |name: &str, v: Option<f64>, lo: f64, hi: f64| -> Result<(), String> {
            match v {
                Some(x) if !(lo..=hi).contains(&x) || !x.is_finite() => {
                    Err(err("presets.out_of_range")
                        .with("field", name)
                        .with("min", lo)
                        .with("max", hi)
                        .with("value", x)
                        .into())
                }
                _ => Ok(()),
            }
        };
        range("temperature", s.temperature, 0.0, 5.0)?;
        range("top_p", s.top_p, 0.0, 1.0)?;
        range("min_p", s.min_p, 0.0, 1.0)?;
        range("typical_p", s.typical_p, 0.0, 1.0)?;
        range("xtc_probability", s.xtc_probability, 0.0, 1.0)?;
        range("presence_penalty", s.presence_penalty, -2.0, 2.0)?;
        range("frequency_penalty", s.frequency_penalty, -2.0, 2.0)?;
        range("xtc_threshold", s.xtc_threshold, 0.0, 1.0)?;
        range("repeat_penalty", s.repeat_penalty, 0.0, 10.0)?;
        range("dry_multiplier", s.dry_multiplier, 0.0, 100.0)?;
        range("dry_base", s.dry_base, 0.0, 100.0)?;
        range("top_n_sigma", s.top_n_sigma, -1.0, 100.0)?;
        range("mirostat_tau", s.mirostat_tau, 0.0, 100.0)?;
        range("mirostat_eta", s.mirostat_eta, 0.0, 1.0)?;
        range("dynatemp_range", s.dynatemp_range, 0.0, 5.0)?;
        range("dynatemp_exponent", s.dynatemp_exponent, 0.0, 10.0)?;
        // A negative target means adaptive-p is off.
        range("adaptive_target", s.adaptive_target, -1.0, 1.0)?;
        range("adaptive_decay", s.adaptive_decay, 0.0, 0.99)?;
        if s.mirostat.is_some_and(|m| !(0..=2).contains(&m)) {
            return Err(err("presets.invalid_mirostat").into());
        }
        let at_least = |name: &str, v: Option<i64>, lo: i64| match v {
            Some(x) if x < lo => Err(String::from(
                err("presets.below_min")
                    .with("field", name)
                    .num("min", lo)
                    .num("value", x),
            )),
            _ => Ok(()),
        };
        at_least("top_k", s.top_k, 0)?;
        if s.top_logprobs.is_some_and(|n| !(0..=20).contains(&n)) {
            return Err(err("presets.invalid_top_logprobs").into());
        }
        at_least("max_tokens", s.max_tokens, -1)?;
        at_least("repeat_last_n", s.repeat_last_n, -1)?;
        at_least("dry_allowed_length", s.dry_allowed_length, 0)?;
        at_least("dry_penalty_last_n", s.dry_penalty_last_n, -1)?;
        if s.stop.iter().any(|x| x.is_empty()) {
            return Err(err("presets.empty_stop").into());
        }
        if let Some(name) = s
            .samplers
            .iter()
            .find(|x| !SAMPLER_NAMES.contains(&x.as_str()))
        {
            return Err(err("presets.unknown_sampler")
                .with("name", name)
                .with("valid", SAMPLER_NAMES.join(", "))
                .into());
        }
        if let Some(b) = s.logit_bias.as_ref() {
            if !(b.is_object() || b.is_array()) {
                return Err(err("presets.invalid_logit_bias").into());
            }
        }
        if let Some(j) = s.json_schema.as_ref() {
            if !j.is_object() {
                return Err(err("presets.json_schema_not_object").into());
            }
        }
        if s.grammar.as_deref().is_some_and(|g| !g.trim().is_empty()) && s.json_schema.is_some() {
            return Err(err("presets.grammar_with_json_schema").into());
        }
        if let Some(f) = self.response_format.as_ref() {
            if !f.is_object() {
                return Err(err("presets.response_format_not_object").into());
            }
        }
        if let Some(tc) = self.tool_choice.as_ref() {
            let ok = match tc {
                Value::String(s) => matches!(s.as_str(), "auto" | "none" | "required"),
                Value::Object(_) => true,
                _ => false,
            };
            if !ok {
                return Err(err("presets.invalid_tool_choice").into());
            }
        }
        let r = &self.reasoning;
        if let Some(f) = r.format.as_deref() {
            if !REASONING_FORMATS.contains(&f) {
                return Err(err("presets.invalid_reasoning_format")
                    .with("valid", REASONING_FORMATS.join(", "))
                    .into());
            }
        }
        if let Some(e) = r.effort.as_deref() {
            if !matches!(e, "low" | "medium" | "high") {
                return Err(err("presets.invalid_reasoning_effort").into());
            }
        }
        at_least("thinking_budget_tokens", r.budget, -1)?;
        Ok(())
    }
}

/// Sampler names llama-server accepts in `samplers`.
pub const SAMPLER_NAMES: &[&str] = &[
    "dry",
    "top_k",
    "typ_p",
    "top_p",
    "min_p",
    "xtc",
    "temperature",
    "top_n_sigma",
    "adaptive_p",
    "penalties",
    "infill",
];

/// `reasoning_format` values of llama-server.
pub const REASONING_FORMATS: &[&str] = &["none", "auto", "deepseek", "deepseek-legacy"];

/// File format of exported presets.
pub const PRESET_FORMAT: &str = "harness.preset.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetBundle {
    pub format: String,
    #[serde(default)]
    pub exported_at: u64,
    pub presets: Vec<InferencePreset>,
}

/// `harness.preset.v1` JSON of `presets` (built-ins lose their read-only mark: an imported
/// copy is a user preset).
pub fn export_presets(presets: &[InferencePreset]) -> String {
    let bundle = PresetBundle {
        format: PRESET_FORMAT.to_string(),
        exported_at: super::now_secs(),
        presets: presets
            .iter()
            .cloned()
            .map(|mut p| {
                p.builtin = false;
                p
            })
            .collect(),
    };
    serde_json::to_string_pretty(&bundle).unwrap_or_default()
}

/// Reads an exported preset file: a `harness.preset.v1` bundle, or (leniently) one preset
/// object or an array of presets. Every preset is validated; ids and timestamps are
/// dropped (the store assigns new ones), so an import never overwrites anything.
pub fn parse_presets(text: &str) -> Result<Vec<InferencePreset>, String> {
    let doc: Value = serde_json::from_str(text)
        .map_err(|e| String::from(err("json.invalid").with("error", e)))?;
    let items: Vec<Value> = match &doc {
        Value::Object(m) if m.contains_key("presets") => {
            match m.get("format").and_then(Value::as_str) {
                Some(PRESET_FORMAT) | None => {}
                Some(other) => {
                    return Err(err("presets.unsupported_format")
                        .with("found", other)
                        .with("expected", PRESET_FORMAT)
                        .into())
                }
            }
            m["presets"]
                .as_array()
                .cloned()
                .ok_or_else(|| String::from(err("presets.list_not_array")))?
        }
        Value::Object(_) => vec![doc.clone()],
        Value::Array(a) => a.clone(),
        _ => return Err(err("presets.file_not_object").into()),
    };
    if items.is_empty() {
        return Err(err("presets.file_empty").into());
    }
    items
        .into_iter()
        .enumerate()
        .map(|(i, mut v)| {
            if let Value::Object(m) = &mut v {
                m.entry("id").or_insert(Value::String(String::new()));
            }
            let mut p: InferencePreset = serde_json::from_value(v).map_err(|e| {
                String::from(
                    err("presets.item_invalid")
                        .num("index", (i + 1) as u64)
                        .with("error", e),
                )
            })?;
            p.id = String::new();
            p.builtin = false;
            p.created_at = 0;
            p.updated_at = 0;
            p.name = p.name.trim().to_string();
            p.validate().map_err(|e| {
                String::from(
                    err("presets.item_rejected")
                        .with("name", &p.name)
                        .with("error", e),
                )
            })?;
            Ok(p)
        })
        .collect()
}

impl InferencePreset {
    /// An empty user preset.
    pub fn new(name: &str) -> Self {
        Self::blank("", name)
    }
}

/// Presets shipped with the app (read-only). Values follow the model makers' common
/// recommendations; `Default` sends nothing so the model's own defaults apply.
pub fn builtin_presets() -> Vec<InferencePreset> {
    let mk = |key: &str, name: &str, desc: &str, f: &dyn Fn(&mut InferencePreset)| {
        let mut p = InferencePreset::blank(&format!("{}{}", BUILTIN_PREFIX, key), name);
        p.builtin = true;
        p.description = desc.to_string();
        f(&mut p);
        p
    };
    vec![
        mk(
            "default",
            "Default",
            "The server's and the model's own defaults; no sampling settings are sent.",
            &|_| {},
        ),
        mk(
            "coding",
            "Coding",
            "Low temperature and narrow sampling for code.",
            &|p| {
                p.sampling.temperature = Some(0.2);
                p.sampling.top_p = Some(0.95);
                p.sampling.top_k = Some(40);
                p.sampling.min_p = Some(0.05);
                p.sampling.repeat_penalty = Some(1.05);
            },
        ),
        mk(
            "reasoning",
            "Reasoning",
            "For thinking models (Qwen3 / DeepSeek-R1 recommendations); thinking is on.",
            &|p| {
                p.sampling.temperature = Some(0.6);
                p.sampling.top_p = Some(0.95);
                p.sampling.top_k = Some(20);
                p.sampling.min_p = Some(0.0);
                p.reasoning.enabled = Some(true);
                p.reasoning.format = Some("auto".into());
            },
        ),
        mk(
            "creative",
            "Creative",
            "Creative writing: high temperature, with min-p and XTC for variety.",
            &|p| {
                p.sampling.temperature = Some(1.0);
                p.sampling.top_p = Some(1.0);
                p.sampling.min_p = Some(0.05);
                p.sampling.xtc_probability = Some(0.5);
                p.sampling.xtc_threshold = Some(0.1);
                p.sampling.dry_multiplier = Some(0.8);
            },
        ),
        mk(
            "deterministic",
            "Deterministic",
            "The same output every time: greedy selection and a fixed seed.",
            &|p| {
                p.sampling.temperature = Some(0.0);
                p.sampling.top_k = Some(1);
                p.sampling.seed = Some(42);
            },
        ),
    ]
}

pub fn is_builtin_id(id: &str) -> bool {
    id.starts_with(BUILTIN_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_unique_and_read_only() {
        let all = builtin_presets();
        let names: Vec<_> = all.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Default",
                "Coding",
                "Reasoning",
                "Creative",
                "Deterministic"
            ]
        );
        for p in &all {
            assert!(p.builtin && is_builtin_id(&p.id));
            p.validate().unwrap();
        }
        assert!(all[0].request_params().is_empty(), "Default sends nothing");
    }

    #[test]
    fn request_params_use_llama_server_names() {
        let p = builtin_presets()
            .into_iter()
            .find(|p| p.id == "builtin:reasoning")
            .unwrap();
        let body = Value::Object(p.request_params());
        assert_eq!(body["temperature"], 0.6);
        assert_eq!(body["top_k"], 20);
        assert_eq!(body["reasoning_format"], "auto");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], true);
        assert!(body.get("stop").is_none(), "empty lists are not sent");
    }

    #[test]
    fn validation_rejects_out_of_range_values() {
        let mut p = InferencePreset::blank("x", "X");
        p.sampling.top_p = Some(1.5);
        assert!(p.validate().is_err());
        p.sampling.top_p = None;
        p.sampling.mirostat = Some(3);
        assert!(p.validate().is_err());
        p.sampling.mirostat = None;
        p.name = " ".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn dynatemp_adaptive_and_logprobs_validate_and_serialize() {
        let mut p = InferencePreset::blank("x", "X");
        p.sampling.dynatemp_range = Some(0.5);
        p.sampling.dynatemp_exponent = Some(1.0);
        p.sampling.adaptive_target = Some(0.7);
        p.sampling.adaptive_decay = Some(0.9);
        p.sampling.logprobs = Some(true);
        p.sampling.top_logprobs = Some(5);
        p.sampling.samplers = vec!["adaptive_p".into()];
        p.validate().unwrap();

        let body = Value::Object(p.request_params());
        assert_eq!(body["dynatemp_range"], 0.5);
        assert_eq!(body["dynatemp_exponent"], 1.0);
        assert_eq!(body["adaptive_target"], 0.7);
        assert_eq!(body["adaptive_decay"], 0.9);
        assert_eq!(body["logprobs"], true);
        assert_eq!(body["top_logprobs"], 5);

        let json = serde_json::to_string(&p).unwrap();
        let back: InferencePreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sampling, p.sampling);

        let mut q = InferencePreset::blank("y", "Y");
        assert!(!Value::Object(q.request_params())
            .to_string()
            .contains("logprobs"));
        q.sampling.top_logprobs = Some(21);
        assert!(q.validate().is_err());
        q.sampling.top_logprobs = Some(-1);
        assert!(q.validate().is_err());
        q.sampling.top_logprobs = Some(0);
        q.validate().unwrap();
        q.sampling.dynatemp_range = Some(-0.1);
        assert!(q.validate().is_err());
        q.sampling.dynatemp_range = None;
        q.sampling.adaptive_decay = Some(1.0);
        assert!(q.validate().is_err());
    }
}
