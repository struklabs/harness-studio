//! Model-aware per-request thinking controls. The level table is shared with the UI.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

static LEVELS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../src/lib/thinking-levels.json"))
        .expect("valid shared thinking table")
});

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReasoningControl {
    pub effort: bool,
    pub toggle: bool,
    pub budget: bool,
}

impl ReasoningControl {
    pub fn from_template(template: &str) -> Self {
        Self {
            effort: template.contains("reasoning_effort") || template.contains("<|channel|>"),
            toggle: template.contains("enable_thinking")
                || template.contains("thinking_budget")
                || (template.contains("<think>") && !template.contains("reasoning_content")),
            // A template alone cannot prove runtime support for a request budget.
            budget: false,
        }
    }

    pub fn supported(self) -> bool {
        self.effort || self.toggle
    }

    pub fn from_props(props: &Value) -> Self {
        let mut control = Self::from_template(props["chat_template"].as_str().unwrap_or_default());
        control.effort |= props
            .pointer("/chat_template_caps/supports_reasoning_effort")
            .and_then(Value::as_bool)
            == Some(true);
        // b11321's server-common.cpp accepts thinking_budget_tokens (and
        // reasoning_budget_tokens) when the template supplies thinking_end_tags.
        // Do not infer this from the launch flag alone on unknown/older runtimes.
        let build = props["build_info"]
            .as_str()
            .and_then(|s| s.strip_prefix('b'))
            .and_then(|s| s.split('-').next())
            .and_then(|s| s.parse::<u32>().ok());
        control.budget = control.toggle
            && (build.is_some_and(|b| b >= 11321)
                || props
                    .pointer("/chat_template_caps/supports_reasoning_budget")
                    .and_then(Value::as_bool)
                    == Some(true));
        control
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Light,
    Normal,
    Deep,
}

impl ThinkingLevel {
    pub fn from_effort(effort: &str) -> Option<Self> {
        match effort {
            "none" | "off" | "false" => Some(Self::Off),
            "minimal" | "low" => Some(Self::Light),
            "medium" | "true" => Some(Self::Normal),
            "high" | "max" => Some(Self::Deep),
            _ => None,
        }
    }
}

/// Fills model-specific fields, preserving explicit client kwargs and budgets.
pub fn apply_level(body: &mut Map<String, Value>, control: ReasoningControl, level: ThinkingLevel) {
    if !control.supported() {
        return;
    }
    let table = &*LEVELS;
    let key = serde_json::to_value(level).expect("thinking level");
    let row = &table[key.as_str().expect("level name")];
    let explicit_toggle = body
        .get("chat_template_kwargs")
        .and_then(|v| v.get("enable_thinking"))
        .is_some();
    if control.effort {
        if explicit_toggle {
            body.remove("reasoning_effort");
        } else {
            body.insert("reasoning_effort".into(), row["effort"].clone());
        }
    } else {
        body.remove("reasoning_effort");
        let kwargs = body
            .entry("chat_template_kwargs")
            .or_insert_with(|| json!({}));
        if let Some(kwargs) = kwargs.as_object_mut() {
            kwargs
                .entry("enable_thinking")
                .or_insert_with(|| row["enabled"].clone());
        }
        if control.budget
            && !body.contains_key("thinking_budget_tokens")
            && !body.contains_key("reasoning_budget_tokens")
            && !body.contains_key("reasoning_budget")
        {
            // An explicit toggle wins, including Off. Do not add a contradictory budget.
            if !explicit_toggle && level != ThinkingLevel::Off {
                body.insert("thinking_budget_tokens".into(), row["budget"].clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_real_templates_and_runtime_budget() {
        let props: Value = serde_json::from_str(include_str!(
            "../tests/data/b11321-qwen3-0.6b-hybrid.props.json"
        ))
        .unwrap();
        assert_eq!(
            ReasoningControl::from_props(&props),
            ReasoningControl {
                effort: false,
                toggle: true,
                budget: true
            }
        );
        let mut old = props.clone();
        old["build_info"] = json!("unknown");
        assert!(!ReasoningControl::from_props(&old).budget);
        assert!(ReasoningControl::from_template("<|channel|>").effort);
        assert!(ReasoningControl::from_template("<think>").toggle);
        assert!(!ReasoningControl::from_template("<think> reasoning_content").supported());
        assert!(!ReasoningControl::from_template("ordinary chat").supported());
    }
    #[test]
    fn shared_levels_and_client_overrides() {
        let toggle = ReasoningControl {
            toggle: true,
            budget: true,
            ..Default::default()
        };
        for (level, budget) in [
            (ThinkingLevel::Light, 1024),
            (ThinkingLevel::Normal, 4096),
            (ThinkingLevel::Deep, -1),
        ] {
            let mut body = Map::new();
            apply_level(&mut body, toggle, level);
            assert_eq!(body["thinking_budget_tokens"], budget);
            assert_eq!(body["chat_template_kwargs"]["enable_thinking"], true);
        }
        let mut body = json!({"chat_template_kwargs":{"enable_thinking":false,"other":1}, "reasoning_budget_tokens":7}).as_object().unwrap().clone();
        apply_level(&mut body, toggle, ThinkingLevel::Deep);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert!(!body.contains_key("thinking_budget_tokens"));
        let mut body = Map::new();
        apply_level(
            &mut body,
            ReasoningControl {
                effort: true,
                ..Default::default()
            },
            ThinkingLevel::Off,
        );
        assert_eq!(body["reasoning_effort"], "low");
        let mut body = Map::new();
        apply_level(&mut body, toggle, ThinkingLevel::Off);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    }
}
