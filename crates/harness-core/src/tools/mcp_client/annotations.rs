//! What a tool's MCP metadata means for Harness: the permission class from `annotations`, and
//! the `x-mcp-header` parameters the modern Streamable HTTP binding mirrors into headers.

use serde_json::Value;

use crate::tools::PermissionClass;

/// The permission class a tool gets from its MCP `annotations` (until the user sets one).
///
/// MCP annotations are hints a server makes about itself, and the spec tells clients to
/// treat them as untrusted unless the server is. So they can only *lower* the default, never
/// make a tool free to run:
///
/// | annotations                                         | class        |
/// |-----------------------------------------------------|--------------|
/// | none, or `readOnlyHint` absent / `false`            | `dangerous`  |
/// | `readOnlyHint: true` and no `destructiveHint: true` | `restricted` |
/// | `readOnlyHint: true` and `destructiveHint: true`    | `dangerous`  |
///
/// (`destructiveHint` and `openWorldHint` only matter next to `readOnlyHint: true`: a tool
/// that is not read-only is `dangerous` whatever else it claims, and a read-only tool that
/// reaches the open world is still `restricted`, the class of network access.) `safe` is never
/// assigned from a hint: a server cannot declare itself harmless.
pub fn class_from_annotations(annotations: Option<&Value>) -> PermissionClass {
    let flag = |k: &str| annotations.and_then(|a| a.get(k)).and_then(Value::as_bool);
    if flag("readOnlyHint") == Some(true) && flag("destructiveHint") != Some(true) {
        PermissionClass::Restricted
    } else {
        PermissionClass::Dangerous
    }
}

/// One `x-mcp-header` parameter of a tool's input schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderParam {
    /// The name after `Mcp-Param-`.
    pub header: String,
    /// The `properties` chain to the value in the call arguments.
    pub path: Vec<String>,
}

fn is_tchar(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

/// Collects the `x-mcp-header` annotations of an input schema. `Err` names why the tool
/// definition is invalid (the HTTP binding then drops the tool).
pub fn header_params(schema: &Value) -> Result<Vec<HeaderParam>, String> {
    let mut out: Vec<HeaderParam> = Vec::new();
    walk(schema, &mut Vec::new(), true, &mut out)?;
    Ok(out)
}

fn walk(
    node: &Value,
    path: &mut Vec<String>,
    reachable: bool,
    out: &mut Vec<HeaderParam>,
) -> Result<(), String> {
    let Some(obj) = node.as_object() else {
        if let Some(arr) = node.as_array() {
            for v in arr {
                walk(v, path, false, out)?;
            }
        }
        return Ok(());
    };
    if let Some(h) = obj.get("x-mcp-header") {
        let name = h.as_str().unwrap_or("");
        if !reachable || path.is_empty() {
            return Err(format!(
                "x-mcp-header \"{name}\" is not on a property reachable through `properties`"
            ));
        }
        if name.is_empty() || !name.chars().all(is_tchar) {
            return Err(format!(
                "x-mcp-header \"{name}\" is not a valid header name"
            ));
        }
        if !matches!(
            obj.get("type").and_then(Value::as_str),
            Some("string" | "integer" | "boolean")
        ) {
            return Err(format!(
                "x-mcp-header \"{name}\" must be on a string, integer or boolean property"
            ));
        }
        if out.iter().any(|p| p.header.eq_ignore_ascii_case(name)) {
            return Err(format!("x-mcp-header \"{name}\" is used twice"));
        }
        out.push(HeaderParam {
            header: name.to_string(),
            path: path.clone(),
        });
    }
    for (k, v) in obj {
        if k == "properties" && reachable {
            if let Some(props) = v.as_object() {
                for (pk, pv) in props {
                    path.push(pk.clone());
                    walk(pv, path, true, out)?;
                    path.pop();
                }
            }
        } else if v.is_object() || v.is_array() {
            walk(v, path, false, out)?;
        }
    }
    Ok(())
}

/// The `(name, encoded value)` pairs to send for a call's arguments.
pub fn header_values(params: &[HeaderParam], args: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in params {
        let mut v = args;
        let mut found = true;
        for key in &p.path {
            match v.get(key) {
                Some(next) => v = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }
        let text = match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) if n.is_i64() || n.is_u64() => n.to_string(),
            _ => continue, // null and the rest: no header
        };
        out.push((p.header.clone(), encode_header_value(&text)));
    }
    out
}

/// A value as an HTTP header: plain when it is visible ASCII without edge whitespace,
/// otherwise the `=?base64?…?=` form (also when a plain value looks like that form).
pub fn encode_header_value(v: &str) -> String {
    let plain = v.bytes().all(|b| (0x20..=0x7e).contains(&b))
        && !v.starts_with(' ')
        && !v.ends_with(' ')
        && !(v.starts_with("=?base64?") && v.ends_with("?="));
    if plain {
        v.to_string()
    } else {
        format!("=?base64?{}?=", base64(v.as_bytes()))
    }
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = match chunk.len() {
            3 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32,
            2 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8,
            _ => (chunk[0] as u32) << 16,
        };
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn annotations_only_lower_the_default() {
        use PermissionClass::*;
        let c = |v: Value| class_from_annotations(Some(&v));
        assert_eq!(class_from_annotations(None), Dangerous);
        assert_eq!(c(json!({})), Dangerous);
        assert_eq!(c(json!({"title": "x"})), Dangerous);
        assert_eq!(c(json!({"readOnlyHint": false})), Dangerous);
        assert_eq!(c(json!({"readOnlyHint": true})), Restricted);
        assert_eq!(
            c(json!({"readOnlyHint": true, "openWorldHint": true})),
            Restricted
        );
        assert_eq!(
            c(json!({"readOnlyHint": true, "destructiveHint": false})),
            Restricted
        );
        assert_eq!(
            c(json!({"readOnlyHint": true, "destructiveHint": true})),
            Dangerous
        );
        // Not read-only: nothing else makes it milder.
        assert_eq!(
            c(json!({"destructiveHint": false, "openWorldHint": false, "idempotentHint": true})),
            Dangerous
        );
        // A hint of the wrong type is no hint.
        assert_eq!(c(json!({"readOnlyHint": "true"})), Dangerous);
    }

    #[test]
    fn header_params_follow_the_spec_constraints() {
        let ok = json!({"type": "object", "properties": {
            "region": {"type": "string", "x-mcp-header": "Region"},
            "opts": {"type": "object", "properties": {
                "n": {"type": "integer", "x-mcp-header": "Count"}}},
            "plain": {"type": "string"}}});
        let p = header_params(&ok).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].path, ["opts", "n"]);
        let vals = header_values(
            &p,
            &json!({"region": "us-west1", "opts": {"n": 7}, "plain": "x"}),
        );
        assert_eq!(
            vals,
            [
                ("Region".into(), "us-west1".into()),
                ("Count".into(), "7".into())
            ]
        );
        // A missing or null value sends no header.
        assert!(header_values(&p, &json!({"opts": {"n": null}})).is_empty());

        for bad in [
            json!({"properties": {"a": {"type": "string", "x-mcp-header": ""}}}),
            json!({"properties": {"a": {"type": "string", "x-mcp-header": "bad name"}}}),
            json!({"properties": {"a": {"type": "number", "x-mcp-header": "A"}}}),
            json!({"properties": {
                "a": {"type": "string", "x-mcp-header": "A"},
                "b": {"type": "string", "x-mcp-header": "a"}}}),
            json!({"properties": {"l": {"type": "array", "items": {"type": "string", "x-mcp-header": "X"}}}}),
            json!({"properties": {"o": {"anyOf": [{"type": "string", "x-mcp-header": "X"}]}}}),
        ] {
            assert!(header_params(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn header_values_are_encoded_like_the_spec_examples() {
        assert_eq!(encode_header_value("us-west1"), "us-west1");
        assert_eq!(
            encode_header_value("Hello, 世界"),
            "=?base64?SGVsbG8sIOS4lueVjA==?="
        );
        assert_eq!(encode_header_value(" padded "), "=?base64?IHBhZGRlZCA=?=");
        assert_eq!(
            encode_header_value("line1\nline2"),
            "=?base64?bGluZTEKbGluZTI=?="
        );
        assert_eq!(
            encode_header_value("=?base64?literal?="),
            "=?base64?PT9iYXNlNjQ/bGl0ZXJhbD89?="
        );
    }
}
