//! Fine-grained gateway policy of a named token: which model aliases and which
//! gateway endpoint groups it may use, and its rate limits.
//!
//! Every part is optional and the empty value means "no restriction", so tokens created
//! before the policy existed (and tokens created without one) keep their old behaviour.
//!
//! * **Models:** alias patterns, `*` = any run of characters, `?` = one character, compared
//!   case-insensitively with the alias the request resolved to (`qwen3-*`, `*:embed`, `coder`).
//! * **Endpoints:** groups of gateway routes ([`GatewayEndpoint`]).
//! * **Limits:** requests per minute, tokens per minute (prompt + completion, charged when a
//!   request ends), requests in flight. Counters live in memory
//!   (`gateway::limits::RateLimiter`); the limits are stored with the token.
//!
//! The Control API's own routes are governed by [`crate::control_api::Scope`]; this policy
//! only applies to the gateway (`/v1/*`, `/api/*`, `/metrics`).

use serde::{Deserialize, Serialize};

/// Error keys (i18n).
pub const ERR_MODEL_PATTERN: &str = "api.token_model_pattern_invalid";
pub const ERR_LIMIT: &str = "api.token_limit_invalid";

/// Most model patterns a token can carry.
pub const MAX_MODEL_PATTERNS: usize = 64;
/// Longest model pattern (characters).
pub const MAX_PATTERN_LEN: usize = 128;
/// Upper bounds of the limits (anything above is a typo, not a limit).
pub const MAX_REQUESTS_PER_MINUTE: u32 = 1_000_000;
pub const MAX_TOKENS_PER_MINUTE: u32 = 1_000_000_000;
pub const MAX_CONCURRENT: u32 = 1024;

/// Gateway route groups a token can be limited to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum GatewayEndpoint {
    /// `/v1/chat/completions`, `/v1/completions`, `/v1/responses`.
    Chat,
    /// `/v1/embeddings`.
    Embeddings,
    /// `/v1/rerank`.
    Rerank,
    /// `/v1/audio/transcriptions`, `/v1/audio/translations` (speech to text) and
    /// `/v1/audio/speech` (text to speech).
    Audio,
    /// `/v1/images/generations` and `/v1/images/edits` (image generation and editing).
    Images,
    /// `GET /v1/models`, `GET /v1/models/{model}`.
    Models,
    /// Anthropic `/v1/messages`, `/v1/messages/count_tokens`.
    Messages,
    /// The Ollama-compatible `/api/*` (chat, generate, embed, tags, ps, show, pull).
    Ollama,
    /// `GET /metrics` (Prometheus).
    Metrics,
    /// `/v1/hf/{model}`: Hugging Face pipeline tasks (classification, NER, QA...).
    Pipelines,
    /// `/v1/videos` and `/v1/videos/{id}[/content]` (video generation jobs).
    Videos,
}

impl GatewayEndpoint {
    pub const ALL: [GatewayEndpoint; 11] = [
        GatewayEndpoint::Chat,
        GatewayEndpoint::Embeddings,
        GatewayEndpoint::Rerank,
        GatewayEndpoint::Audio,
        GatewayEndpoint::Images,
        GatewayEndpoint::Models,
        GatewayEndpoint::Messages,
        GatewayEndpoint::Ollama,
        GatewayEndpoint::Metrics,
        GatewayEndpoint::Pipelines,
        GatewayEndpoint::Videos,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            GatewayEndpoint::Chat => "chat",
            GatewayEndpoint::Embeddings => "embeddings",
            GatewayEndpoint::Rerank => "rerank",
            GatewayEndpoint::Audio => "audio",
            GatewayEndpoint::Images => "images",
            GatewayEndpoint::Videos => "videos",
            GatewayEndpoint::Models => "models",
            GatewayEndpoint::Messages => "messages",
            GatewayEndpoint::Ollama => "ollama",
            GatewayEndpoint::Metrics => "metrics",
            GatewayEndpoint::Pipelines => "pipelines",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        Self::ALL
            .into_iter()
            .find(|e| e.as_str().eq_ignore_ascii_case(s))
    }

    /// The group of a gateway path (`None` = not a gated route, e.g. `/health`).
    pub fn of_path(path: &str) -> Option<Self> {
        let p = path.trim_end_matches('/');
        Some(match p {
            "/v1/chat/completions"
            | "/v1/completions"
            | "/v1/responses"
            | "/v1/systemone"
            | "/v1/decisions" => Self::Chat,
            "/v1/embeddings" => Self::Embeddings,
            "/v1/rerank" => Self::Rerank,
            "/v1/audio/transcriptions" | "/v1/audio/translations" | "/v1/audio/speech" => {
                Self::Audio
            }
            "/v1/images/generations" | "/v1/images/edits" => Self::Images,
            "/v1/messages" | "/v1/messages/count_tokens" => Self::Messages,
            "/metrics" => Self::Metrics,
            "/v1/ocr" | "/v1/document/qa" => Self::Pipelines,
            _ if p == "/v1/models" || p.starts_with("/v1/models/") => Self::Models,
            _ if p == "/v1/videos" || p.starts_with("/v1/videos/") => Self::Videos,
            _ if p.starts_with("/api/") => Self::Ollama,
            _ if p == "/v1/hf" || p.starts_with("/v1/hf/") => Self::Pipelines,
            _ => return None,
        })
    }
}

/// Rate limits of one token (`None` = unlimited).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RateLimits {
    /// Requests admitted per minute (token bucket: bursts up to this many, refilled evenly).
    pub requests_per_minute: Option<u32>,
    /// Prompt + completion tokens per minute. A request is admitted while the budget is
    /// positive; its tokens are charged when it ends (so one long answer can overdraw it,
    /// and the next request waits until the budget recovers).
    pub tokens_per_minute: Option<u32>,
    /// Requests of this token in flight at once (streams count until they end).
    pub max_concurrent: Option<u32>,
}

impl RateLimits {
    pub fn is_unlimited(&self) -> bool {
        self.requests_per_minute.is_none()
            && self.tokens_per_minute.is_none()
            && self.max_concurrent.is_none()
    }

    /// Canonical limits or [`ERR_LIMIT`]: 0 is not a limit (it would lock the token out;
    /// revoke it instead), and absurd values are typos.
    pub fn validate(self) -> Result<Self, String> {
        let check = |v: Option<u32>, max: u32| match v {
            Some(0) => Err(ERR_LIMIT.to_string()),
            Some(n) if n > max => Err(ERR_LIMIT.to_string()),
            other => Ok(other),
        };
        Ok(Self {
            requests_per_minute: check(self.requests_per_minute, MAX_REQUESTS_PER_MINUTE)?,
            tokens_per_minute: check(self.tokens_per_minute, MAX_TOKENS_PER_MINUTE)?,
            max_concurrent: check(self.max_concurrent, MAX_CONCURRENT)?,
        })
    }
}

/// The gateway policy of a named token (stored with it; see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenPolicy {
    /// Model alias patterns (`*`, `?`); empty = every model.
    pub models: Vec<String>,
    /// Gateway endpoint groups; empty = every endpoint.
    pub endpoints: Vec<GatewayEndpoint>,
    pub limits: RateLimits,
}

impl TokenPolicy {
    pub fn is_unrestricted(&self) -> bool {
        self.models.is_empty() && self.endpoints.is_empty() && self.limits.is_unlimited()
    }

    pub fn allows_endpoint(&self, endpoint: GatewayEndpoint) -> bool {
        self.endpoints.is_empty() || self.endpoints.contains(&endpoint)
    }

    /// Whether `alias` matches one of the model patterns (or there are none).
    pub fn allows_model(&self, alias: &str) -> bool {
        self.models.is_empty() || self.models.iter().any(|p| glob_match(p, alias))
    }

    /// Canonical policy: trimmed, de-duplicated, sorted endpoints, validated limits.
    pub fn validate(&self) -> Result<Self, String> {
        let mut models: Vec<String> = Vec::new();
        for raw in &self.models {
            let p = raw.trim();
            if p.is_empty() {
                continue;
            }
            if p.chars().count() > MAX_PATTERN_LEN
                || p.chars().any(|c| c.is_control() || c.is_whitespace())
            {
                return Err(ERR_MODEL_PATTERN.to_string());
            }
            if !models.iter().any(|m| m.eq_ignore_ascii_case(p)) {
                models.push(p.to_string());
            }
        }
        if models.len() > MAX_MODEL_PATTERNS {
            return Err(ERR_MODEL_PATTERN.to_string());
        }
        // `*` alone allows everything: store the canonical "no restriction".
        if models.iter().any(|m| m.chars().all(|c| c == '*')) {
            models.clear();
        }
        let mut endpoints = self.endpoints.clone();
        endpoints.sort();
        endpoints.dedup();
        if endpoints.len() == GatewayEndpoint::ALL.len() {
            endpoints.clear();
        }
        Ok(Self {
            models,
            endpoints,
            limits: self.limits.validate()?,
        })
    }
}

/// Case-insensitive glob: `*` matches any run of characters (also none), `?` exactly one.
/// Two-pointer match that only backtracks to the last `*`: O(pattern × text) at worst, never
/// exponential on hostile patterns.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_patterns() {
        assert!(glob_match("qwen3-*", "qwen3-4b"));
        assert!(glob_match("qwen3-*", "Qwen3-"));
        assert!(!glob_match("qwen3-*", "qwen2-7b"));
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("coder", "CODER"));
        assert!(!glob_match("coder", "coder2"));
        assert!(glob_match("*:embed", "nomic:embed"));
        assert!(!glob_match("*:embed", "nomic:embedx"));
        assert!(glob_match("m?del", "model"));
        assert!(!glob_match("m?del", "mdel"));
        assert!(glob_match("a*b*c", "a-x-b-y-c"));
        assert!(!glob_match("a*b*c", "a-x-c-y-b"));
        assert!(glob_match("**a", "bbba"));
        assert!(glob_match("gemma-3-*-it", "gemma-3-27b-it"));
        assert!(!glob_match("gemma-3-*-it", "gemma-3-27b-pt"));
        assert!(
            glob_match("ı*", "ıx"),
            "non-ASCII characters are single characters"
        );
        // Hostile pattern stays fast (no exponential backtracking).
        let long = "a".repeat(5000);
        let started = std::time::Instant::now();
        assert!(!glob_match(&format!("{}b", "*a".repeat(100)), &long));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn endpoint_groups_of_paths() {
        use GatewayEndpoint::*;
        assert_eq!(GatewayEndpoint::of_path("/v1/chat/completions"), Some(Chat));
        assert_eq!(GatewayEndpoint::of_path("/v1/responses"), Some(Chat));
        assert_eq!(GatewayEndpoint::of_path("/v1/completions"), Some(Chat));
        assert_eq!(GatewayEndpoint::of_path("/v1/embeddings"), Some(Embeddings));
        assert_eq!(GatewayEndpoint::of_path("/v1/rerank"), Some(Rerank));
        assert_eq!(
            GatewayEndpoint::of_path("/v1/hf/ner-model"),
            Some(Pipelines)
        );
        assert_eq!(GatewayEndpoint::of_path("/v1/models"), Some(Models));
        assert_eq!(GatewayEndpoint::of_path("/v1/models/a/b"), Some(Models));
        assert_eq!(GatewayEndpoint::of_path("/v1/messages"), Some(Messages));
        assert_eq!(
            GatewayEndpoint::of_path("/v1/messages/count_tokens"),
            Some(Messages)
        );
        assert_eq!(GatewayEndpoint::of_path("/api/chat"), Some(Ollama));
        assert_eq!(GatewayEndpoint::of_path("/api/tags"), Some(Ollama));
        assert_eq!(GatewayEndpoint::of_path("/metrics"), Some(Metrics));
        assert_eq!(GatewayEndpoint::of_path("/health"), None);
        for e in GatewayEndpoint::ALL {
            assert_eq!(serde_json::to_value(e).unwrap(), e.as_str());
            assert_eq!(GatewayEndpoint::parse(&e.as_str().to_uppercase()), Some(e));
        }
        assert_eq!(GatewayEndpoint::parse("nope"), None);
    }

    #[test]
    fn empty_policy_allows_everything() {
        let p = TokenPolicy::default();
        assert!(p.is_unrestricted());
        assert!(p.allows_model("anything"));
        for e in GatewayEndpoint::ALL {
            assert!(p.allows_endpoint(e));
        }
        // Old token files have no policy fields at all.
        let parsed: TokenPolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, p);
    }

    #[test]
    fn policy_validation_is_canonical() {
        let p = TokenPolicy {
            models: vec![
                " qwen3-* ".into(),
                "".into(),
                "QWEN3-*".into(),
                "coder".into(),
            ],
            endpoints: vec![
                GatewayEndpoint::Ollama,
                GatewayEndpoint::Chat,
                GatewayEndpoint::Chat,
            ],
            limits: RateLimits {
                requests_per_minute: Some(60),
                tokens_per_minute: None,
                max_concurrent: Some(2),
            },
        }
        .validate()
        .unwrap();
        assert_eq!(p.models, vec!["qwen3-*", "coder"]);
        assert_eq!(
            p.endpoints,
            vec![GatewayEndpoint::Chat, GatewayEndpoint::Ollama]
        );
        assert!(p.allows_model("Qwen3-4B"));
        assert!(p.allows_model("coder"));
        assert!(!p.allows_model("llama"));
        assert!(p.allows_endpoint(GatewayEndpoint::Chat));
        assert!(!p.allows_endpoint(GatewayEndpoint::Embeddings));

        // `*` and "every endpoint" collapse to "no restriction".
        let all = TokenPolicy {
            models: vec!["x".into(), "*".into()],
            endpoints: GatewayEndpoint::ALL.to_vec(),
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert!(all.is_unrestricted());

        for bad in ["has space", "tab\there", &"x".repeat(MAX_PATTERN_LEN + 1)] {
            let p = TokenPolicy {
                models: vec![bad.to_string()],
                ..Default::default()
            };
            assert_eq!(p.validate().unwrap_err(), ERR_MODEL_PATTERN, "{bad:?}");
        }
        let many = TokenPolicy {
            models: (0..=MAX_MODEL_PATTERNS).map(|i| format!("m{i}")).collect(),
            ..Default::default()
        };
        assert_eq!(many.validate().unwrap_err(), ERR_MODEL_PATTERN);

        for limits in [
            RateLimits {
                requests_per_minute: Some(0),
                ..Default::default()
            },
            RateLimits {
                max_concurrent: Some(MAX_CONCURRENT + 1),
                ..Default::default()
            },
            RateLimits {
                tokens_per_minute: Some(MAX_TOKENS_PER_MINUTE + 1),
                ..Default::default()
            },
        ] {
            let p = TokenPolicy {
                limits,
                ..Default::default()
            };
            assert_eq!(p.validate().unwrap_err(), ERR_LIMIT);
        }
    }
}
