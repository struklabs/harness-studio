//! Error bodies in the shape each client family expects.
//!
//! * OpenAI (`/v1/chat/completions`, `/v1/responses`, ...):
//!   `{"error": {"message", "type", "code", "param", "harness": {...}}}`
//! * Anthropic (`/v1/messages`, `/v1/messages/count_tokens`):
//!   `{"type": "error", "error": {"type", "message", "code"}}`

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct GatewayError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub param: Option<Box<str>>,
    pub retry_after: Option<u64>,
    /// Extra machine-readable details (`harness` object).
    pub details: Option<Box<Value>>,
    pub anthropic: bool,
    /// Rate-limit state of the caller's token (`x-ratelimit-*` headers).
    pub rate: Option<Box<harness_core::gateway::limits::Snapshot>>,
}

impl GatewayError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.to_string(),
            message: message.into(),
            param: None,
            retry_after: None,
            details: None,
            anthropic: false,
            rate: None,
        }
    }

    pub fn rate(mut self, s: harness_core::gateway::limits::Snapshot) -> Self {
        self.rate = Some(Box::new(s));
        self
    }

    pub fn anthropic(mut self, yes: bool) -> Self {
        self.anthropic = yes;
        self
    }

    pub fn param(mut self, p: impl Into<String>) -> Self {
        self.param = Some(p.into().into_boxed_str());
        self
    }

    pub fn retry_after(mut self, s: Option<u64>) -> Self {
        self.retry_after = s;
        self
    }

    pub fn details(mut self, d: Value) -> Self {
        self.details = Some(Box::new(d));
        self
    }

    pub fn from_sched(e: &harness_core::gateway::scheduler::SchedError) -> Self {
        use harness_core::gateway::scheduler::SchedError;
        let details = match e {
            SchedError::ModelNotFound { available, .. }
            | SchedError::ModelRequired { available } => json!({"available_models": available}),
            SchedError::InsufficientMemory {
                need_bytes,
                available_bytes,
                blocking,
                options,
                tried,
                ..
            } => json!({
                "need_bytes": need_bytes, "available_bytes": available_bytes,
                "blocking_models": blocking, "suggestions": options,
                "tried": tried
            }),
            SchedError::MaxLoaded { limit, .. } => json!({"max_loaded_models": limit}),
            SchedError::ModelBusy {
                alias,
                busy,
                reason,
                retry_after,
            } => json!({
                "model": alias, "busy_models": busy, "reason": reason,
                "retry_after": retry_after, "retry_when": "requests_finished"
            }),
            SchedError::LoadFailed { code, .. } => json!({"failure": code}),
            _ => Value::Null,
        };
        let mut err =
            Self::new(e.http_status(), e.code(), e.message()).retry_after(e.retry_after());
        if matches!(
            e,
            SchedError::ModelNotFound { .. } | SchedError::ModelRequired { .. }
        ) {
            err = err.param("model");
        }
        if !details.is_null() {
            err = err.details(details);
        }
        err
    }

    fn openai_type(&self) -> &'static str {
        if self.code == "rate_limited" {
            return "rate_limit_error";
        }
        match self.status {
            400 | 404 | 413 | 422 => "invalid_request_error",
            401 => "authentication_error",
            403 => "permission_error",
            503 | 429 => "server_overloaded",
            _ => "server_error",
        }
    }

    fn anthropic_type(&self) -> &'static str {
        match self.status {
            400 | 413 | 422 => "invalid_request_error",
            401 => "authentication_error",
            403 => "permission_error",
            404 => "not_found_error",
            429 => "rate_limit_error",
            503 => "overloaded_error",
            504 => "timeout_error",
            _ => "api_error",
        }
    }

    pub fn body(&self) -> Value {
        if self.anthropic {
            let mut e = json!({
                "type": self.anthropic_type(),
                "message": self.message,
                "code": self.code,
            });
            if let Some(d) = &self.details {
                e["harness"] = (**d).clone();
            }
            json!({"type": "error", "error": e})
        } else {
            let mut e = json!({
                "message": self.message,
                "type": self.openai_type(),
                "code": self.code,
                "param": self.param,
            });
            if let Some(d) = &self.details {
                e["harness"] = (**d).clone();
            }
            json!({"error": e})
        }
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut resp = (status, axum::Json(self.body())).into_response();
        if let Some(s) = self.retry_after {
            if let Ok(v) = HeaderValue::from_str(&s.to_string()) {
                resp.headers_mut().insert(header::RETRY_AFTER, v);
            }
        }
        if let Ok(v) = HeaderValue::from_str(&self.code) {
            resp.headers_mut().insert("x-harness-error", v);
        }
        if let Some(r) = &self.rate {
            crate::auth::rate_headers(resp.headers_mut(), r, self.anthropic);
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_models_have_retry_header_and_details_in_both_protocols() {
        let error = harness_core::gateway::scheduler::SchedError::ModelBusy {
            alias: "next".into(),
            busy: vec!["active".into()],
            reason: "insufficient_memory".into(),
            retry_after: 5,
        };
        for anthropic in [false, true] {
            let mapped = GatewayError::from_sched(&error).anthropic(anthropic);
            let body = mapped.body();
            assert_eq!(body["error"]["harness"]["busy_models"], json!(["active"]));
            assert_eq!(body["error"]["harness"]["retry_when"], "requests_finished");
            let response = mapped.into_response();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(response.headers()[header::RETRY_AFTER], "5");
        }
    }
}
