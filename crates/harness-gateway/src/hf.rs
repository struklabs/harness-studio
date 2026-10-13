//! `/v1/hf/{model}`: Hugging Face Inference API requests for task pipelines.
//!
//! The upstream (`harness-onnx` or the Python worker, `POST /hf`) always gets a JSON object
//! with the model of the path as its `model`, the field the proxy resolves. A raw image or
//! audio body (the Inference API also takes the file itself, with `?parameters=<json>`) is
//! turned into `{"inputs": "data:<type>;base64,...", "parameters": ...}`.

use crate::errors::GatewayError;
use base64::Engine as _;
use bytes::Bytes;
use serde_json::{Map, Value};

/// Largest raw file sent as the body: base64 makes it 4/3 larger, and the upstreams accept
/// 64 MiB, so the encoded request must still fit with room for the JSON around it.
pub const MAX_RAW_FILE: usize = 47 * 1024 * 1024;

/// Media types sent as the file itself rather than as JSON.
fn is_binary(content_type: &str) -> bool {
    let ct = content_type.trim().to_ascii_lowercase();
    ct.starts_with("image/")
        || ct.starts_with("audio/")
        || ct.starts_with("video/")
        || ct.starts_with("application/octet-stream")
}

/// The upstream body. A body that is neither a JSON object nor a binary file is passed on
/// unchanged for the proxy to refuse; a raw file over [`MAX_RAW_FILE`] is refused here (413).
pub fn body(
    model: &str,
    content_type: Option<&str>,
    parameters: Option<&str>,
    body: Bytes,
) -> Result<Bytes, GatewayError> {
    let mut map = match content_type.filter(|ct| is_binary(ct)) {
        Some(_) if body.len() > MAX_RAW_FILE => {
            return Err(GatewayError::new(
                413,
                "payload_too_large",
                format!(
                    "the file is {} MiB; at most {} MiB can be sent to a task model",
                    body.len() >> 20,
                    MAX_RAW_FILE >> 20
                ),
            ))
        }
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
            _ => return Ok(body),
        },
    };
    map.insert("model".into(), Value::String(model.to_string()));
    Ok(serde_json::to_vec(&map).map(Bytes::from).unwrap_or(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(b: Result<Bytes, GatewayError>) -> Value {
        serde_json::from_slice(&b.unwrap()).unwrap()
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
    fn oversized_files_are_refused_before_encoding() {
        let big = Bytes::from(vec![0u8; MAX_RAW_FILE + 1]);
        let err = body("m", Some("image/png"), None, big).unwrap_err();
        assert_eq!((err.status, err.code.as_str()), (413, "payload_too_large"));
        // The encoded largest file still fits the upstream's 64 MiB.
        assert!(MAX_RAW_FILE.div_ceil(3) * 4 + 4096 < 64 << 20);
    }

    #[test]
    fn other_bodies_pass_unchanged() {
        let raw = Bytes::from_static(b"plain text");
        assert_eq!(
            body("m", Some("text/plain"), None, raw.clone()).unwrap(),
            raw
        );
        assert_eq!(
            body("m", None, None, Bytes::from_static(b"[1]")).unwrap(),
            Bytes::from_static(b"[1]")
        );
    }
}
