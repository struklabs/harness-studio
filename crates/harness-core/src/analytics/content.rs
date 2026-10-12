//! Optional content capture (per definition, off by default).
//!
//! When a definition has `capture_content` on, the gateway keeps the request body and the
//! response bytes (up to [`CAPTURE_LIMIT`] each) and hands them to the recorder next to the
//! metadata. Cleaning before storage: `data:` URIs (base64 images, audio) are replaced by a
//! size note, the text is cut at [`CAPTURE_LIMIT`] bytes on a character boundary. Content
//! never travels on the event bus and is never logged.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Largest request / response text stored per request (bytes, after cleaning).
pub const CAPTURE_LIMIT: usize = 256 * 1024;
/// Largest raw response the gateway buffers for capture.
const RAW_LIMIT: usize = 4 * CAPTURE_LIMIT;

/// Content captured for one request, as handed to the recorder.
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedContent {
    pub request: String,
    pub response: String,
    pub request_truncated: bool,
    pub response_truncated: bool,
    /// Days until the content is deleted.
    pub retention_days: u32,
}

/// Stored content of one request (Request inspector).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StoredContent {
    pub captured_at_ms: u64,
    pub expires_at_ms: u64,
    /// Request body (JSON as sent, cleaned).
    pub request: String,
    /// Response as received (JSON, or the SSE event stream), cleaned.
    pub response: String,
    pub request_truncated: bool,
    pub response_truncated: bool,
}

/// Accumulates what the gateway sees of one request while capture is on.
#[derive(Debug)]
pub struct ContentCapture {
    request: Vec<u8>,
    response: Vec<u8>,
    request_overflow: bool,
    response_overflow: bool,
    retention_days: u32,
}

impl ContentCapture {
    pub fn new(request: &[u8], retention_days: u32) -> Self {
        let keep = request.len().min(RAW_LIMIT);
        Self {
            request: request[..keep].to_vec(),
            response: Vec::new(),
            request_overflow: request.len() > RAW_LIMIT,
            response_overflow: false,
            retention_days: retention_days.clamp(1, 3650),
        }
    }

    pub fn push_response(&mut self, chunk: &[u8]) {
        let room = RAW_LIMIT.saturating_sub(self.response.len());
        if chunk.len() > room {
            self.response_overflow = true;
        }
        self.response
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
    }

    pub fn finish(self) -> CapturedContent {
        let (request, request_truncated) = clean(&self.request);
        let (response, response_truncated) = clean(&self.response);
        CapturedContent {
            request,
            response,
            request_truncated: request_truncated || self.request_overflow,
            response_truncated: response_truncated || self.response_overflow,
            retention_days: self.retention_days,
        }
    }
}

fn data_uri() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"data:[A-Za-z0-9.+/-]+(;[A-Za-z0-9=.-]+)*;base64,[A-Za-z0-9+/=\\]+")
            .expect("valid regex")
    })
}

/// Applies the cleaning rules; `true` when the text was cut.
pub fn clean(raw: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(raw);
    let text = data_uri().replace_all(&text, |c: &regex::Captures| {
        format!("[data URI removed, {} bytes]", c[0].len())
    });
    if text.len() <= CAPTURE_LIMIT {
        return (text.into_owned(), false);
    }
    let mut end = CAPTURE_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning_removes_data_uris_and_cuts_long_text() {
        let body = br#"{"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,iVBORw0KGgo="}}]}]}"#;
        let (text, cut) = clean(body);
        assert!(!cut);
        assert!(!text.contains("iVBORw0KGgo"));
        assert!(text.contains("[data URI removed, 34 bytes]"), "{text}");

        let long = "ş".repeat(CAPTURE_LIMIT); // 2 bytes each
        let (text, cut) = clean(long.as_bytes());
        assert!(cut);
        assert!(text.len() <= CAPTURE_LIMIT);

        let mut c = ContentCapture::new(b"{}", 0);
        c.push_response(&vec![b'a'; RAW_LIMIT + 10]);
        let out = c.finish();
        assert!(out.response_truncated);
        assert_eq!(out.retention_days, 1);
    }
}
