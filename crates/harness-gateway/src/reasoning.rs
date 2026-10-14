//! Normalize client thinking requests after acquiring the served model's capabilities.
use harness_core::reasoning::{apply_level, ReasoningControl, ThinkingLevel};
use serde_json::Value;

pub fn translate(body: &[u8], control: ReasoningControl) -> Option<Vec<u8>> {
    if !control.supported() {
        return None;
    }
    let mut value: Value = serde_json::from_slice(body).ok()?;
    let object = value.as_object_mut()?;
    let level = object
        .get("think")
        .and_then(|v| match v {
            Value::Bool(false) => Some(ThinkingLevel::Off),
            Value::Bool(true) => Some(ThinkingLevel::Normal),
            Value::String(s) => ThinkingLevel::from_effort(s),
            _ => None,
        })
        .or_else(|| {
            object
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .and_then(ThinkingLevel::from_effort)
        })
        .or_else(|| {
            object
                .get("reasoning")
                .and_then(|v| v.get("effort"))
                .and_then(Value::as_str)
                .and_then(ThinkingLevel::from_effort)
        })?;
    object.remove("think");
    if let Some(reasoning) = object.get_mut("reasoning").and_then(Value::as_object_mut) {
        reasoning.remove("effort");
    }
    apply_level(object, control, level);
    serde_json::to_vec(&value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn chat_responses_and_ollama_use_same_model_mapping() {
        let toggle = ReasoningControl {
            toggle: true,
            budget: true,
            effort: false,
        };
        for body in [
            json!({"reasoning_effort":"low"}),
            json!({"reasoning":{"effort":"minimal"}}),
            json!({"think":"low"}),
        ] {
            let mapped: Value = serde_json::from_slice(
                &translate(&serde_json::to_vec(&body).unwrap(), toggle).unwrap(),
            )
            .unwrap();
            assert_eq!(mapped["thinking_budget_tokens"], 1024);
            assert_eq!(mapped["chat_template_kwargs"]["enable_thinking"], true);
            assert!(mapped.get("reasoning_effort").is_none());
        }
        for body in [
            json!({"reasoning_effort":"none"}),
            json!({"reasoning":{"effort":"none"}}),
            json!({"think":false}),
        ] {
            let mapped: Value = serde_json::from_slice(
                &translate(&serde_json::to_vec(&body).unwrap(), toggle).unwrap(),
            )
            .unwrap();
            assert_eq!(mapped["chat_template_kwargs"]["enable_thinking"], false);
        }
        let effort = ReasoningControl {
            effort: true,
            ..Default::default()
        };
        let mapped: Value =
            serde_json::from_slice(&translate(br#"{"think":false}"#, effort).unwrap()).unwrap();
        assert_eq!(mapped["reasoning_effort"], "low");
    }
    #[test]
    fn preserves_explicit_controls_and_absent_requests() {
        let control = ReasoningControl {
            toggle: true,
            budget: true,
            effort: false,
        };
        let mapped: Value = serde_json::from_slice(&translate(br#"{"reasoning_effort":"high","chat_template_kwargs":{"enable_thinking":false,"custom":1},"thinking_budget_tokens":23}"#, control).unwrap()).unwrap();
        assert_eq!(mapped["thinking_budget_tokens"], 23);
        assert_eq!(
            mapped["chat_template_kwargs"],
            json!({"enable_thinking":false,"custom":1})
        );
        assert!(translate(br#"{"messages":[]}"#, control).is_none());
        assert!(translate(
            br#"{"reasoning_effort":"high"}"#,
            ReasoningControl::default()
        )
        .is_none());
        let toggle_only = ReasoningControl {
            toggle: true,
            ..Default::default()
        };
        let mapped: Value =
            serde_json::from_slice(&translate(br#"{"think":"high"}"#, toggle_only).unwrap())
                .unwrap();
        assert!(mapped.get("thinking_budget_tokens").is_none());
    }
}
