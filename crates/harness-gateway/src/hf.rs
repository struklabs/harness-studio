//! `/v1/hf/{model}`: Hugging Face Inference API requests for task pipelines.
//!
//! The upstream (`harness-onnx` or the Python worker, `POST /hf`) always gets a JSON object
//! with the model of the path as its `model`, the field the proxy resolves. A raw image or
//! audio body (the Inference API also takes the file itself, with `?parameters=<json>`) is
//! turned into `{"inputs": "data:<type>;base64,...", "parameters": ...}`.

use base64::Engine as _;
use bytes::Bytes;
use serde_json::{Map, Value};

/// Media types sent as the file itself rather than as JSON.
fn is_binary(content_type: &str) -> bool {
    let ct = content_type.trim().to_ascii_lowercase();
    ct.starts_with("image/")
        || ct.starts_with("audio/")
        || ct.starts_with("video/")
        || ct.starts_with("application/octet-stream")
}

/// The upstream body. A body that is neither a JSON object nor a binary file is passed on
/// unchanged for the proxy to refuse.
pub fn body(
    model: &str,
    content_type: Option<&str>,
    parameters: Option<&str>,
    body: Bytes,
) -> Bytes {
    let mut map = match content_type.filter(|ct| is_binary(ct)) {
        Some(ct) => {
            let mime = ct.split(';').next().unwrap_or(ct).trim();
            let mut map = Map::new();
            let data = base64::engine::general_purpose::STANDARD.encode(&body);
            map.insert(
                "inputs".into(),
                Value::String(format!("data:{mime};base64,{data}")),
            );
            if let Some(p) = parameters.and_then(|p| serde_json::from_str::<Value>(p).ok()) {
                if p.is_object() {
                    map.insert("parameters".into(), p);
                }
            }
            map
        }
        None => match serde_json::from_slice::<Value>(&body) {
            Ok(Value::Object(map)) => map,
            _ => return body,
        },
    };
    map.insert("model".into(), Value::String(model.to_string()));
    serde_json::to_vec(&map).map(Bytes::from).unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(b: Bytes) -> Value {
        serde_json::from_slice(&b).unwrap()
    }

    #[test]
    fn json_bodies_get_the_model_of_the_path() {
        let out = body(
            "sst2",
            Some("application/json"),
            None,
            Bytes::from(r#"{"inputs":"hi","model":"other"}"#),
        );
        assert_eq!(
            json(out),
            serde_json::json!({"inputs": "hi", "model": "sst2"})
        );
    }

    #[test]
    fn raw_files_become_data_urls_with_their_parameters() {
        let out = body(
            "vit",
            Some("image/png; charset=binary"),
            Some(r#"{"top_k":2}"#),
            Bytes::from_static(&[1, 2, 3]),
        );
        assert_eq!(
            json(out),
            serde_json::json!({"inputs": "data:image/png;base64,AQID", "parameters": {"top_k": 2}, "model": "vit"})
        );
        // Parameters that are not a JSON object are left out.
        let out = body(
            "w2v",
            Some("audio/wav"),
            Some("nope"),
            Bytes::from_static(b"RIFF"),
        );
        assert!(json(out).get("parameters").is_none());
    }

    #[test]
    fn other_bodies_pass_unchanged() {
        let raw = Bytes::from_static(b"plain text");
        assert_eq!(body("m", Some("text/plain"), None, raw.clone()), raw);
        assert_eq!(
            body("m", None, None, Bytes::from_static(b"[1]")),
            Bytes::from_static(b"[1]")
        );
    }
}
