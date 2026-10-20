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

    pub fn remote(key: &'static str) -> Self {
        let unsupported = matches!(
            key,
            "link.unsupported_endpoint" | "link.chained_not_allowed"
        );
        Self::new(
            if unsupported { 400 } else { 502 },
            if unsupported {
                key
            } else {
                "harness_link_unreachable"
            },
            match key {
                "link.unsupported_endpoint" => "This endpoint is unsupported for remote models.",
                "link.chained_not_allowed" => {
                    "Remote Harness requests cannot be chained to another remote Harness."
                }
                "link.certificate_invalid" => "Certificate could not be verified",
                "link.unauthorized" => "The remote Harness token was rejected.",
                "link.timeout" => "The remote Harness request timed out.",
                _ => "The remote Harness is unreachable.",
            },
        )
        .details(json!({"error_key": key}))
    }

    /// A BYOK cloud provider failed or refused (`cloud.*` keys).
    pub fn cloud(key: &str) -> Self {
        let (status, code, message) = match key {
            "cloud.unsupported_endpoint" => (
                400,
                "cloud_unsupported_endpoint",
                "Cloud provider models only accept POST /v1/chat/completions.",
            ),
            "cloud.unauthorized" => (
                502,
                "cloud_provider_unauthorized",
                "The cloud provider rejected the API key.",
            ),
            "cloud.key_missing" => (
                502,
                "cloud_provider_key_missing",
                "No API key is saved for this cloud provider.",
            ),
            "cloud.timeout" => (
                504,
                "cloud_provider_timeout",
                "The cloud provider did not answer in time.",
            ),
            _ => (
                502,
                "cloud_provider_unreachable",
                "The cloud provider is unreachable.",
            ),
        };
        Self::new(status, code, message).details(json!({"error_key": key}))
    }

    pub fn reject_remote(def: &harness_core::domain::ModelDefinition) -> Result<(), Self> {
        if harness_core::link::parse_model(&def.id).is_some() {
            Err(Self::remote("link.unsupported_endpoint"))
        } else if harness_core::cloud::parse_model(&def.id).is_some() {
            Err(Self::cloud("cloud.unsupported_endpoint"))
        } else {
            Ok(())
        }
    }

    pub fn from_sched(e: &harness_core::gateway::scheduler::SchedError) -> Self {
        use harness_core::gateway::scheduler::SchedError;
        if let SchedError::LoadFailed { code, message, .. } = e {
            if code == "link.unsupported_endpoint" {
                return Self::remote("link.unsupported_endpoint");
            }
            if code == "cloud_provider" {
                let key = harness_core::error_key::parse(message).map(|(k, _)| k);
                return Self::cloud(match key {
                    Some(k @ ("cloud.unauthorized" | "cloud.key_missing" | "cloud.timeout")) => k,
                    Some("cloud.unsupported_endpoint") => "cloud.unsupported_endpoint",
                    _ => "cloud.unreachable",
                });
            }
            if code == "harness_link_unreachable" {
                return Self::remote(if message.starts_with("link.unauthorized") {
                    "link.unauthorized"
                } else if message.starts_with("link.certificate_invalid") {
                    "link.certificate_invalid"
                } else {
                    "link.unreachable"
                });
            }
        }
        let mut details = match e {
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
        let blockers = match e {
            SchedError::InsufficientMemory { blocking, .. } => Some(blocking.as_slice()),
            SchedError::LoadFailed { .. } if e.is_oom() => Some([].as_slice()),
            SchedError::ModelBusy { busy, reason, .. } if reason == "insufficient_memory" => {
                Some(busy.as_slice())
            }
            _ => None,
        };
        // A keyed load error (`servers.no_free_port`...) goes to the UI as its key, so it is shown
        // localized instead of as the English message.
        if let SchedError::LoadFailed { message, .. } = e {
            if blockers.is_none() && harness_core::error_key::parse(message).is_some() {
                details["error_key"] = json!(message);
            }
        }
        if let Some(blockers) = blockers {
            use harness_core::error_key::err;
            let key = if blockers.is_empty() {
                err("gateway.gpu_memory_full")
            } else {
                err("gateway.gpu_memory_blocked").with("servers", blockers.join(", "))
            };
            details["error_key"] = json!(key.to_string());
        }
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
    fn remote_certificate_failures_keep_their_key_and_message() {
        let expected = GatewayError::remote("link.certificate_invalid").body();
        let error = harness_core::gateway::scheduler::SchedError::LoadFailed {
            alias: "remote:office/model".into(),
            code: "harness_link_unreachable".into(),
            message: "link.certificate_invalid".into(),
        };
        assert_eq!(GatewayError::from_sched(&error).body(), expected);
        assert_eq!(
            expected["error"]["message"],
            "Certificate could not be verified"
        );
        assert_eq!(
            expected["error"]["harness"]["error_key"],
            "link.certificate_invalid"
        );
    }

    #[test]
    fn memory_failures_carry_localizable_keys_and_blockers() {
        use harness_core::gateway::scheduler::SchedError;
        for error in [
            SchedError::InsufficientMemory {
                alias: "image".into(),
                need_bytes: Some(4_000_000_000),
                available_bytes: Some(1_000_000_000),
                blocking: vec!["chat".into()],
                after_full_eviction: false,
                options: vec![],
                tried: Box::default(),
            },
            SchedError::ModelBusy {
                alias: "image".into(),
                busy: vec!["chat".into()],
                reason: "insufficient_memory".into(),
                retry_after: 5,
            },
        ] {
            let body = GatewayError::from_sched(&error).body();
            let key = body["error"]["harness"]["error_key"].as_str().unwrap();
            let (key, params) = harness_core::error_key::parse(key).unwrap();
            assert_eq!(key, "gateway.gpu_memory_blocked");
            assert_eq!(params["servers"], "chat");
        }
        // Every port of the automatic range taken: a readable message and the key for the UI.
        let error = SchedError::LoadFailed {
            alias: "chat".into(),
            code: "servers.no_free_port".into(),
            message: "servers.no_free_port".into(),
        };
        let body = GatewayError::from_sched(&error).body();
        assert_eq!(
            body["error"]["harness"]["error_key"],
            "servers.no_free_port"
        );
        let message = body["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("no free port") && message.contains("11440-11499"),
            "{message}"
        );
        let error = SchedError::LoadFailed {
            alias: "image".into(),
            code: "servers.out_of_memory".into(),
            message: "allocation failed".into(),
        };
        let body = GatewayError::from_sched(&error).body();
        assert_eq!(
            body["error"]["harness"]["error_key"],
            "gateway.gpu_memory_full"
        );
        let error = SchedError::ModelBusy {
            alias: "image".into(),
            busy: vec!["chat".into()],
            reason: "max_loaded_models".into(),
            retry_after: 5,
        };
        assert!(GatewayError::from_sched(&error).body()["error"]["harness"]["error_key"].is_null());
    }

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
