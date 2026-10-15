//! Gateway authentication and token policy (ARCHITECTURE §13.6).
//!
//! * Credentials: `Authorization: Bearer <token>` or `x-api-key: <token>` (Anthropic
//!   clients). Accepted: named Control API tokens with the `inference` (or `admin`) scope and
//!   the gateway's own client token (`<data>/gateway-token`, Harness's clients).
//! * On loopback without `require_token` nothing is required (like a loopback llama-server);
//!   a valid token still names the client. On a network bind a token is always required
//!   (no keyless LAN; `validate_settings` also refuses such a bind without an inference
//!   token).
//! * Loopback bind: the `Host` header must be a loopback name (DNS rebinding).
//! * A named token's policy (`token_policy::TokenPolicy`) narrows what it may
//!   do: endpoint groups ([`check`] → `403 endpoint_not_allowed`), model aliases
//!   ([`Caller::check_model`] → `403 model_not_allowed`) and rate limits ([`Caller::admit`] →
//!   `429 rate_limited` + `Retry-After`). The policy binds whoever presents the token; on an
//!   open loopback gateway a client that sends no token at all is anonymous and unrestricted
//!   (turn on `require_token` to make the policy binding).

use axum::http::{header, HeaderMap, HeaderName, HeaderValue};
use harness_core::control_api::{self, Scope};
use harness_core::gateway::limits::{LimitKind, Permit, Rejection, Snapshot};
use harness_core::gateway::scheduler::SchedError;
use harness_core::token_policy::{GatewayEndpoint, TokenPolicy};

use crate::errors::GatewayError;
use crate::GwState;

/// Who called.
#[derive(Debug, Clone, Default)]
pub struct Caller {
    /// Named token's name, `harness` for the client token, `None` without a valid token.
    pub token_name: Option<String>,
    /// Named token's id (policy and rate-limit counters); `None` for the client token and
    /// anonymous callers.
    pub token_id: Option<String>,
    pub scopes: Vec<Scope>,
    /// The named token's policy (empty = unrestricted).
    pub policy: TokenPolicy,
    pub anthropic: bool,
}

impl Caller {
    pub fn allows_model(&self, alias: &str) -> bool {
        self.policy.allows_model(alias)
    }

    /// `403 model_not_allowed` unless the token may use `alias`.
    pub fn check_model(&self, alias: &str) -> Result<(), GatewayError> {
        if self.allows_model(alias) {
            return Ok(());
        }
        Err(GatewayError::new(
            403,
            "model_not_allowed",
            format!(
                "The token \"{}\" may not use the model \"{}\" (allowed: {}).",
                self.token_name.as_deref().unwrap_or("-"),
                alias,
                self.policy.models.join(", ")
            ),
        )
        .param("model")
        .anthropic(self.anthropic))
    }

    /// A scheduler error for this caller: a token limited to some models only sees those in
    /// the "available models" lists.
    pub fn sched_error(&self, e: &SchedError) -> GatewayError {
        let scrub = |a: &[String]| -> Vec<String> {
            a.iter().filter(|m| self.allows_model(m)).cloned().collect()
        };
        let scrubbed = match e {
            SchedError::ModelNotFound { model, available } => SchedError::ModelNotFound {
                model: model.clone(),
                available: scrub(available),
            },
            SchedError::ModelRequired { available } => SchedError::ModelRequired {
                available: scrub(available),
            },
            other => return GatewayError::from_sched(other).anthropic(self.anthropic),
        };
        GatewayError::from_sched(&scrubbed).anthropic(self.anthropic)
    }

    /// Admits the request under the token's rate limits. `Ok(None)` = no limits apply.
    pub fn admit(&self, state: &GwState) -> Result<Option<Permit>, GatewayError> {
        let Some(id) = self.token_id.as_deref() else {
            return Ok(None);
        };
        let limits = &self.policy.limits;
        if limits.is_unlimited() {
            return Ok(None);
        }
        state
            .limiter
            .admit(id, limits)
            .map(Some)
            .map_err(|r| rate_limited(&r, self.token_name.as_deref(), self.anthropic))
    }
}

/// `429 rate_limited` with `Retry-After`, the limit in `harness` details and the
/// `x-ratelimit-*` headers.
pub fn rate_limited(r: &Rejection, token: Option<&str>, anthropic: bool) -> GatewayError {
    let what = match r.kind {
        LimitKind::RequestsPerMinute => format!("{} requests per minute", r.limit),
        LimitKind::TokensPerMinute => format!("{} tokens per minute", r.limit),
        LimitKind::Concurrency => format!("{} requests at a time", r.limit),
    };
    GatewayError::new(
        429,
        "rate_limited",
        format!(
            "Rate limit reached for token \"{}\": {}. Retry after {} s.",
            token.unwrap_or("-"),
            what,
            r.retry_after_secs
        ),
    )
    .retry_after(Some(r.retry_after_secs))
    .details(serde_json::json!({
        "limit": r.kind.as_str(),
        "value": r.limit,
        "retry_after_seconds": r.retry_after_secs,
    }))
    .rate(r.snapshot)
    .anthropic(anthropic)
}

fn reset_value(secs: f64) -> String {
    // OpenAI style: "1s", "0.5s", "6m0s".
    if secs >= 60.0 {
        let whole = secs.ceil() as u64;
        format!("{}m{}s", whole / 60, whole % 60)
    } else {
        let rounded = (secs * 10.0).ceil() / 10.0;
        format!("{}s", rounded)
    }
}

/// Rate-limit headers of a token with limits: OpenAI's `x-ratelimit-*` on every endpoint and
/// Anthropic's `anthropic-ratelimit-*` counts on `/v1/messages`.
pub fn rate_headers(headers: &mut HeaderMap, s: &Snapshot, anthropic: bool) {
    let mut put = |name: &'static str, value: String| {
        if let Ok(v) = HeaderValue::from_str(&value) {
            headers.insert(HeaderName::from_static(name), v);
        }
    };
    if let Some(l) = s.requests_limit {
        put("x-ratelimit-limit-requests", l.to_string());
        put(
            "x-ratelimit-remaining-requests",
            s.requests_remaining.unwrap_or(0).to_string(),
        );
        put(
            "x-ratelimit-reset-requests",
            reset_value(s.requests_reset_secs.unwrap_or(0.0)),
        );
        if anthropic {
            put("anthropic-ratelimit-requests-limit", l.to_string());
            put(
                "anthropic-ratelimit-requests-remaining",
                s.requests_remaining.unwrap_or(0).to_string(),
            );
        }
    }
    if let Some(l) = s.tokens_limit {
        put("x-ratelimit-limit-tokens", l.to_string());
        put(
            "x-ratelimit-remaining-tokens",
            s.tokens_remaining.unwrap_or(0).to_string(),
        );
        put(
            "x-ratelimit-reset-tokens",
            reset_value(s.tokens_reset_secs.unwrap_or(0.0)),
        );
        if anthropic {
            put("anthropic-ratelimit-tokens-limit", l.to_string());
            put(
                "anthropic-ratelimit-tokens-remaining",
                s.tokens_remaining.unwrap_or(0).to_string(),
            );
        }
    }
    if let Some(l) = s.concurrent_limit {
        put("x-harness-ratelimit-concurrency", l.to_string());
        put("x-harness-ratelimit-in-flight", s.in_flight.to_string());
    }
}

/// Header names [`rate_headers`] may set (CORS `expose-headers`).
pub const RATE_HEADERS: [&str; 12] = [
    "x-ratelimit-limit-requests",
    "x-ratelimit-remaining-requests",
    "x-ratelimit-reset-requests",
    "x-ratelimit-limit-tokens",
    "x-ratelimit-remaining-tokens",
    "x-ratelimit-reset-tokens",
    "anthropic-ratelimit-requests-limit",
    "anthropic-ratelimit-requests-remaining",
    "anthropic-ratelimit-tokens-limit",
    "anthropic-ratelimit-tokens-remaining",
    "x-harness-ratelimit-concurrency",
    "x-harness-ratelimit-in-flight",
];

fn presented(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Some((scheme, token)) = v.split_once(' ') {
            if scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() {
                return Some(token.trim().to_string());
            }
        }
    }
    headers
        .get("x-api-key")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn host_allowed(state: &GwState, headers: &HeaderMap) -> bool {
    if !state.loopback {
        return true;
    }
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return true;
    };
    let port = state.addr.port();
    let host = host.to_ascii_lowercase();
    [
        format!("127.0.0.1:{}", port),
        format!("localhost:{}", port),
        format!("[::1]:{}", port),
    ]
    .contains(&host)
        || host == "127.0.0.1"
        || host == "localhost"
}

/// Checks a request to an `endpoint` group. `anthropic` picks the error shape.
pub fn check(
    state: &GwState,
    headers: &HeaderMap,
    anthropic: bool,
    endpoint: GatewayEndpoint,
) -> Result<Caller, GatewayError> {
    authenticate(state, headers, anthropic, Some(endpoint), false)
}

pub fn whoami(state: &GwState, headers: &HeaderMap) -> Result<Caller, GatewayError> {
    authenticate(state, headers, false, None, true)
}

fn authenticate(
    state: &GwState,
    headers: &HeaderMap,
    anthropic: bool,
    endpoint: Option<GatewayEndpoint>,
    require_token: bool,
) -> Result<Caller, GatewayError> {
    if !host_allowed(state, headers) {
        return Err(GatewayError::new(
            403,
            "forbidden_host",
            "Host header not allowed on a loopback gateway.",
        )
        .anthropic(anthropic));
    }
    let anonymous = Caller {
        anthropic,
        ..Default::default()
    };
    let required = require_token || state.scheduler.settings().token_required();
    let Some(token) = presented(headers) else {
        if required {
            return Err(unauthorized(anthropic));
        }
        return Ok(anonymous);
    };
    if control_api::ct_eq(token.as_bytes(), state.client_token.as_bytes()) {
        return Ok(Caller {
            token_name: Some("harness".into()),
            scopes: vec![Scope::Inference],
            anthropic,
            ..Default::default()
        });
    }
    match state.tokens.verify(&token) {
        Some((info, scopes)) if control_api::allows(&scopes, Scope::Inference) => {
            if let Some(endpoint) = endpoint.filter(|e| !info.policy.allows_endpoint(*e)) {
                return Err(GatewayError::new(
                    403,
                    "endpoint_not_allowed",
                    format!(
                        "The token \"{}\" may not use the {} endpoints (allowed: {}).",
                        info.name,
                        endpoint.as_str(),
                        info.policy
                            .endpoints
                            .iter()
                            .map(|e| e.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
                .anthropic(anthropic));
            }
            Ok(Caller {
                token_name: Some(info.name),
                token_id: Some(info.id),
                scopes,
                policy: info.policy,
                anthropic,
            })
        }
        Some(_) if required => Err(GatewayError::new(
            403,
            "insufficient_scope",
            "This token has no \"inference\" scope.",
        )
        .anthropic(anthropic)),
        // Clients often send a placeholder key ("sk-no-key", "llama"); on an open loopback
        // gateway that is fine.
        _ if !required => Ok(anonymous),
        _ => Err(unauthorized(anthropic)),
    }
}

fn unauthorized(anthropic: bool) -> GatewayError {
    GatewayError::new(
        401,
        "invalid_api_key",
        "A Harness token with the \"inference\" scope is required (Authorization: Bearer <token> or x-api-key).",
    )
    .anthropic(anthropic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_values_follow_openai_format() {
        assert_eq!(reset_value(0.0), "0s");
        assert_eq!(reset_value(0.42), "0.5s");
        assert_eq!(reset_value(20.0), "20s");
        assert_eq!(reset_value(60.0), "1m0s");
        assert_eq!(reset_value(361.2), "6m2s");
    }

    #[test]
    fn rate_headers_only_for_configured_limits() {
        let mut h = HeaderMap::new();
        rate_headers(
            &mut h,
            &Snapshot {
                requests_limit: Some(60),
                requests_remaining: Some(59),
                requests_reset_secs: Some(1.0),
                ..Default::default()
            },
            true,
        );
        assert_eq!(h["x-ratelimit-limit-requests"], "60");
        assert_eq!(h["x-ratelimit-remaining-requests"], "59");
        assert_eq!(h["x-ratelimit-reset-requests"], "1s");
        assert_eq!(h["anthropic-ratelimit-requests-remaining"], "59");
        assert!(!h.contains_key("x-ratelimit-limit-tokens"));
        assert!(!h.contains_key("x-harness-ratelimit-concurrency"));
        for name in RATE_HEADERS {
            HeaderName::from_static(name);
        }
    }
}
