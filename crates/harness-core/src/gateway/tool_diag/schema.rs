//! A strict JSON-Schema subset check for the arguments of a model's tool call.
//!
//! The diagnosis wants to know whether the *model* followed the tool's schema, so unlike the
//! lenient run-time check of `tools::registry::validate_args` (which forgives numeric text for
//! integers because the model's intent is clear) this one is exact. It understands what tool
//! definitions use in practice: `type` (also a list), `enum`, `const`, `required`,
//! `properties`, `additionalProperties`, `items`, `minimum` / `maximum`, `minLength` /
//! `maxLength`, `minItems` / `maxItems`, `anyOf` / `oneOf` / `allOf`. Keywords it does not know
//! (`$ref`, `pattern`, `format`...) are accepted: a check that cannot be sure says nothing.

use serde_json::Value;

/// The first rule an argument value broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Where, dotted (`days`, `edits[2].old`); empty for the root.
    pub path: String,
    /// The schema keyword that failed (`required`, `type`, `enum`, `maximum`...).
    pub rule: &'static str,
}

/// `Ok` when `value` fits `schema`, else the first violation (properties in schema order).
pub fn validate(schema: &Value, value: &Value) -> Result<(), Violation> {
    check(schema, value, &mut Vec::new())
}

fn here(path: &[String]) -> String {
    let mut out = String::new();
    for p in path {
        if p.starts_with('[') || out.is_empty() {
            out.push_str(p);
        } else {
            out.push('.');
            out.push_str(p);
        }
    }
    out
}

fn fail(path: &[String], rule: &'static str) -> Result<(), Violation> {
    Err(Violation {
        path: here(path),
        rule,
    })
}

fn type_ok(ty: &str, v: &Value) -> bool {
    match ty {
        "string" => v.is_string(),
        "integer" => {
            v.is_i64()
                || v.is_u64()
                || v.as_f64()
                    .is_some_and(|f| f.is_finite() && f.fract() == 0.0)
        }
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "null" => v.is_null(),
        _ => true,
    }
}

fn check(schema: &Value, v: &Value, path: &mut Vec<String>) -> Result<(), Violation> {
    let Some(s) = schema.as_object() else {
        return Ok(()); // `true` / unknown schema: anything fits
    };
    match s.get("type") {
        Some(Value::String(ty)) if !type_ok(ty, v) => return fail(path, "type"),
        Some(Value::Array(types)) => {
            let names: Vec<&str> = types.iter().filter_map(Value::as_str).collect();
            if !names.is_empty() && !names.iter().any(|t| type_ok(t, v)) {
                return fail(path, "type");
            }
        }
        _ => {}
    }
    if let Some(options) = s.get("enum").and_then(Value::as_array) {
        if !options.contains(v) {
            return fail(path, "enum");
        }
    }
    if let Some(c) = s.get("const") {
        if c != v {
            return fail(path, "const");
        }
    }
    if let Some(all) = s.get("allOf").and_then(Value::as_array) {
        for sub in all {
            check(sub, v, path)?;
        }
    }
    for (key, rule) in [("anyOf", "anyOf"), ("oneOf", "oneOf")] {
        if let Some(any) = s.get(key).and_then(Value::as_array) {
            if !any.is_empty()
                && !any
                    .iter()
                    .any(|sub| check(sub, v, &mut path.clone()).is_ok())
            {
                return fail(path, rule);
            }
        }
    }
    if let Some(n) = v.as_f64() {
        let bound = |key: &str| s.get(key).and_then(Value::as_f64);
        if bound("minimum").is_some_and(|m| n < m) {
            return fail(path, "minimum");
        }
        if bound("maximum").is_some_and(|m| n > m) {
            return fail(path, "maximum");
        }
        if bound("exclusiveMinimum").is_some_and(|m| n <= m) {
            return fail(path, "exclusiveMinimum");
        }
        if bound("exclusiveMaximum").is_some_and(|m| n >= m) {
            return fail(path, "exclusiveMaximum");
        }
    }
    if let Some(text) = v.as_str() {
        let len = text.chars().count() as u64;
        if s.get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| len < m)
        {
            return fail(path, "minLength");
        }
        if s.get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| len > m)
        {
            return fail(path, "maxLength");
        }
    }
    if let Some(items) = v.as_array() {
        let len = items.len() as u64;
        if s.get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| len < m)
        {
            return fail(path, "minItems");
        }
        if s.get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| len > m)
        {
            return fail(path, "maxItems");
        }
        if let Some(item_schema) = s.get("items").filter(|i| i.is_object()) {
            for (i, item) in items.iter().enumerate() {
                path.push(format!("[{i}]"));
                let r = check(item_schema, item, path);
                path.pop();
                r?;
            }
        }
    }
    if let Some(obj) = v.as_object() {
        let props = s.get("properties").and_then(Value::as_object);
        if let Some(required) = s.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                if !obj.contains_key(name) {
                    path.push(name.to_string());
                    let r = fail(path, "required");
                    path.pop();
                    return r;
                }
            }
        }
        // Schema order first so the report is stable and names the first declared property.
        let mut names: Vec<&String> = Vec::new();
        if let Some(props) = props {
            names.extend(props.keys().filter(|k| obj.contains_key(*k)));
        }
        names.extend(
            obj.keys()
                .filter(|k| props.is_none_or(|p| !p.contains_key(*k))),
        );
        for name in names {
            let value = &obj[name];
            path.push(name.clone());
            let r = match props.and_then(|p| p.get(name)) {
                Some(sub) => check(sub, value, path),
                None => match s.get("additionalProperties") {
                    Some(Value::Bool(false)) => fail(path, "additionalProperties"),
                    Some(sub @ Value::Object(_)) => check(sub, value, path),
                    _ => Ok(()),
                },
            };
            path.pop();
            r?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn broken(schema: Value, value: Value) -> (String, &'static str) {
        let v = validate(&schema, &value).unwrap_err();
        (v.path, v.rule)
    }

    #[test]
    fn accepts_what_fits() {
        let schema = json!({"type": "object", "properties": {
            "city": {"type": "string"}, "days": {"type": "integer", "minimum": 1, "maximum": 3},
            "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}},
            "required": ["city"]});
        assert!(validate(&schema, &json!({"city": "Ankara"})).is_ok());
        assert!(validate(
            &schema,
            &json!({"city": "Ankara", "days": 3, "unit": "celsius"})
        )
        .is_ok());
        // 3.0 is a whole number: an integer for JSON Schema.
        assert!(validate(&schema, &json!({"city": "Ankara", "days": 3.0})).is_ok());
    }

    #[test]
    fn names_the_first_broken_rule() {
        let schema = json!({"type": "object", "properties": {
            "days": {"type": "integer", "minimum": 1, "maximum": 3}, "unit": {"enum": ["c", "f"]}},
            "required": ["days"]});
        assert_eq!(
            broken(schema.clone(), json!({})),
            ("days".into(), "required")
        );
        assert_eq!(
            broken(schema.clone(), json!({"days": 10})),
            ("days".into(), "maximum")
        );
        assert_eq!(
            broken(schema.clone(), json!({"days": "3"})),
            ("days".into(), "type")
        );
        assert_eq!(
            broken(schema.clone(), json!({"days": 1.5})),
            ("days".into(), "type")
        );
        assert_eq!(
            broken(schema, json!({"days": 2, "unit": "k"})),
            ("unit".into(), "enum")
        );
    }

    #[test]
    fn nested_paths_and_arrays() {
        let schema = json!({"type": "object", "properties": {
            "edits": {"type": "array", "minItems": 1, "items": {"type": "object",
                "properties": {"old": {"type": "string", "minLength": 1}}, "required": ["old"]}}}});
        assert_eq!(
            broken(
                schema.clone(),
                json!({"edits": [{"old": "a"}, {"old": ""}]})
            ),
            ("edits[1].old".into(), "minLength")
        );
        assert_eq!(
            broken(schema.clone(), json!({"edits": []})),
            ("edits".into(), "minItems")
        );
        assert_eq!(
            broken(schema, json!({"edits": [{"new": "b"}]})),
            ("edits[0].old".into(), "required")
        );
    }

    #[test]
    fn additional_properties_union_types_and_combinators() {
        let strict = json!({"type": "object", "properties": {"a": {"type": ["string", "null"]}},
            "additionalProperties": false});
        assert!(validate(&strict, &json!({"a": null})).is_ok());
        assert_eq!(
            broken(strict.clone(), json!({"a": 1})),
            ("a".into(), "type")
        );
        assert_eq!(
            broken(strict, json!({"b": 1})),
            ("b".into(), "additionalProperties")
        );
        let any = json!({"anyOf": [{"type": "string"}, {"type": "integer"}]});
        assert!(validate(&any, &json!(4)).is_ok());
        assert_eq!(broken(any, json!(true)), (String::new(), "anyOf"));
    }

    #[test]
    fn unknown_keywords_are_accepted() {
        let schema = json!({"type": "object", "properties": {"a": {"$ref": "#/defs/x"},
            "b": {"type": "string", "pattern": "^x"}}});
        assert!(validate(&schema, &json!({"a": 1, "b": "y"})).is_ok());
        assert!(validate(&json!(true), &json!(1)).is_ok());
    }
}
