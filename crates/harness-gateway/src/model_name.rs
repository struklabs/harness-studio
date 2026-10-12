//! The model id in the answers of a server that does not know the Harness alias. A mistral.rs
//! server answers with its own id (the model folder's path for chat completions, `default` for
//! Responses); the client asked for the alias and gets it back, as from llama-server (`--alias`),
//! and the local path does not leak.

use serde_json::Value;

/// Rewrites `model` (and `response.model` of Responses stream events) to `alias`. A stream is
/// rewritten line by line (`data: {...}`); a JSON body is held until the end and rewritten whole.
pub struct ModelRenamer {
    alias: String,
    sse: bool,
    pending: Vec<u8>,
}

impl ModelRenamer {
    pub fn new(alias: &str, sse: bool) -> Self {
        Self {
            alias: alias.to_string(),
            sse,
            pending: Vec::new(),
        }
    }

    /// Bytes ready to send for this chunk: the complete lines of a stream, nothing of a JSON body.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        if !self.sse {
            return Vec::new();
        }
        let Some(end) = self.pending.iter().rposition(|&b| b == b'\n') else {
            return Vec::new();
        };
        let lines: Vec<u8> = self.pending.drain(..=end).collect();
        self.rewrite_lines(&lines)
    }

    /// Whatever is left at the end of the body.
    pub fn finish(&mut self) -> Vec<u8> {
        let rest = std::mem::take(&mut self.pending);
        if self.sse {
            self.rewrite_lines(&rest)
        } else {
            self.rewrite_json(&rest).unwrap_or(rest)
        }
    }

    /// A whole JSON body with its model renamed; `None` when it is not a JSON object.
    pub fn rewrite_json(&self, body: &[u8]) -> Option<Vec<u8>> {
        let mut v: Value = serde_json::from_slice(body).ok()?;
        if !rename(&mut v, &self.alias) {
            return None;
        }
        serde_json::to_vec(&v).ok()
    }

    fn rewrite_lines(&self, bytes: &[u8]) -> Vec<u8> {
        let text = String::from_utf8_lossy(bytes);
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            let body = line.trim_end_matches(['\r', '\n']);
            let ending = &line[body.len()..];
            let renamed = body
                .strip_prefix("data:")
                .map(str::trim_start)
                .and_then(|json| self.rewrite_json(json.as_bytes()))
                .and_then(|j| String::from_utf8(j).ok());
            match renamed {
                Some(json) => {
                    out.push_str("data: ");
                    out.push_str(&json);
                    out.push_str(ending);
                }
                None => out.push_str(line),
            }
        }
        out.into_bytes()
    }
}

/// Sets `model` and `response.model` where they are strings; true when anything changed.
fn rename(v: &mut Value, alias: &str) -> bool {
    let mut changed = false;
    if let Some(m) = v.get_mut("model").filter(|m| m.is_string()) {
        if m != alias {
            *m = Value::from(alias);
            changed = true;
        }
    }
    if let Some(r) = v.get_mut("response").filter(|r| r.is_object()) {
        changed |= rename(r, alias);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_json_body_gets_the_alias() {
        let mut r = ModelRenamer::new("qwen3", false);
        assert!(r
            .push(br#"{"model":"C:\\m\\Qwen3-0.6B","choices":[]}"#)
            .is_empty());
        let out: Value = serde_json::from_slice(&r.finish()).unwrap();
        assert_eq!(out["model"], "qwen3");
        // Not JSON (an upstream error page): passed on as it is.
        let mut r = ModelRenamer::new("qwen3", false);
        r.push(b"oops");
        assert_eq!(r.finish(), b"oops");
    }

    #[test]
    fn stream_lines_are_renamed_across_chunk_boundaries() {
        let mut r = ModelRenamer::new("qwen3", true);
        let mut out = r.push(b"data: {\"model\":\"default\",\"choices\":[]}\n\ndata: {\"mo");
        out.extend(r.push(b"del\":\"default\"}\n\n"));
        out.extend(r.push(b"event: response.completed\ndata: {\"response\":{\"model\":\"default\"}}\n\ndata: [DONE]\n\n"));
        out.extend(r.finish());
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("default"), "{text}");
        assert_eq!(text.matches("qwen3").count(), 3, "{text}");
        assert!(text.contains("event: response.completed\n") && text.ends_with("data: [DONE]\n\n"));
    }
}
