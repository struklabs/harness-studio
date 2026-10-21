//! Per-request metadata of the gateway (the analytics hook).
//!
//! Every request that passes the gateway ends with one [`RequestRecord`] published on the
//! event bus (`CoreEvent::GatewayRequest`) and handed to the analytics recorder
//! (`crate::analytics::submit`, which writes `analytics.db` off the request path). Request
//! and response **content is never part of the record**; only counts, timings and
//! identifiers. Content is captured only for definitions that opt in
//! (`ModelDefinition::capture_content`) and travels separately (never on the bus).
//!
//! [`UsageObserver`] reads token counts and llama-server timings from the response while it
//! streams through (JSON body or SSE `data:` lines) without holding the body.

use serde::{Deserialize, Serialize};

/// Who sent a request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ClientInfo {
    /// `User-Agent` header.
    pub user_agent: Option<String>,
    /// Optional `X-Harness-Client` header (e.g. `opencode`, `harness-playground`).
    pub harness_client: Option<String>,
    /// Name of the named token used (`harness` for the gateway's own client token).
    pub token_name: Option<String>,
    /// Best label in this order: `X-Harness-Client`, token name, a known
    /// `User-Agent` product, else the raw agent.
    pub label: Option<String>,
}

impl ClientInfo {
    pub fn new(
        user_agent: Option<String>,
        harness_client: Option<String>,
        token_name: Option<String>,
    ) -> Self {
        let label = harness_client
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| token_name.clone())
            .or_else(|| user_agent.as_deref().map(product_of_agent));
        Self {
            user_agent,
            harness_client,
            token_name,
            label,
        }
    }
}

/// Short product name of a `User-Agent` (`claude-cli/2.1.3 (external, cli)` → `claude-cli`).
pub fn product_of_agent(agent: &str) -> String {
    let first = agent.split_whitespace().next().unwrap_or(agent);
    first.split('/').next().unwrap_or(first).to_string()
}

/// Token counts and llama-server timings read from a response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    /// Prompt tokens served from the KV cache (`timings.cache_n`, `cached_tokens`).
    pub cached_tokens: Option<u64>,
    pub prompt_ms: Option<f64>,
    pub predicted_ms: Option<f64>,
    pub prompt_per_second: Option<f64>,
    pub predicted_per_second: Option<f64>,
    /// Speculative decoding: drafted and accepted tokens.
    pub draft_n: Option<u64>,
    pub draft_n_accepted: Option<u64>,
    /// `finish_reason` / `stop_reason` / Responses `status`.
    pub finish_reason: Option<String>,
}

/// Metadata of one gateway request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RequestRecord {
    /// Gateway request id (`X-Harness-Request-Id`).
    pub id: String,
    /// A prompt-cache warm-up of the Harness Playground (see `x-harness-purpose` in the
    /// gateway): not a user request, so it is never published (statistics, analytics, traces).
    #[serde(skip)]
    pub warmup: bool,
    /// Unix milliseconds of arrival.
    pub started_at_ms: u64,
    /// Route, e.g. `/v1/chat/completions`.
    pub endpoint: String,
    /// `model` as sent.
    pub model: Option<String>,
    pub alias: Option<String>,
    pub definition_id: Option<String>,
    pub server_id: Option<String>,
    pub client: ClientInfo,
    pub stream: bool,
    /// HTTP status returned to the client.
    pub status: u16,
    /// Gateway error code (`queue_full`, `unsupported_tool_type`, `insufficient_memory`...).
    pub error_code: Option<String>,
    /// The client went away before the response ended (the upstream request was cancelled).
    pub cancelled: bool,
    /// Waiting for a free slot (and for a JIT load), ms.
    pub queue_wait_ms: u64,
    /// JIT load time when this request loaded the model, ms.
    pub load_ms: Option<u64>,
    /// Models evicted to make room for this request's load.
    pub evicted: Vec<String>,
    /// First response byte, ms from arrival.
    pub ttft_ms: Option<u64>,
    pub total_ms: u64,
    pub usage: Usage,
    /// Compatibility changes (`namespace_flattened=3`, `ignored=web_search`).
    pub compat: Vec<String>,
    /// Ran out of memory: no room to load, the load hit OOM, or it did and the retry after
    /// an eviction worked.
    #[serde(default)]
    pub oom: bool,
    /// Context window of one slot of the server that answered (`/props`).
    #[serde(default)]
    pub ctx_size: Option<u64>,
    /// ggml backend of that server's build (`vulkan`, `cuda`...).
    #[serde(default)]
    pub backend: Option<String>,
    /// Slots (concurrent requests) of the server that answered.
    #[serde(default)]
    pub slots: Option<u32>,
    /// USD this request cost at a BYOK provider (provider or user prices x tokens); `None` for
    /// local models and when no price is known.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// Why the request waited for its turn: `slots` (all busy) or `tokens` (the shared KV pool
    /// had no room yet); `None` = it ran at once. Feeds the slot recommendation.
    #[serde(default)]
    pub waited: Option<String>,
    // ── T4: tool-call diagnosis (`gateway::tool_diag`) ─────────────────────
    /// The request or its response involves tools (offered, called or reported on).
    #[serde(default)]
    pub tool_involved: bool,
    /// Tools the request offered.
    #[serde(default)]
    pub tools_offered: u32,
    /// Tool calls the response made.
    #[serde(default)]
    pub tool_calls: u32,
    /// Who is at fault when the round went wrong: `runtime`, `context`, `model` or `client`;
    /// `None` for a clean round.
    #[serde(default)]
    pub tool_diag_category: Option<String>,
    /// What went wrong exactly (`call_left_in_content`, `repeated_call`...), see
    /// `tool_diag::Code`.
    #[serde(default)]
    pub tool_diag_code: Option<String>,
    /// Evidence as JSON text (tool name, excerpt, schema path, linked request...).
    #[serde(default)]
    pub tool_diag_detail: Option<String>,
}

impl RequestRecord {
    /// Publishes the record on the event bus.
    pub fn publish(self) {
        if self.warmup {
            return;
        }
        crate::model_perf::observe_request(&self);
        crate::events::publish(crate::events::CoreEvent::GatewayRequest {
            record: Box::new(self),
        });
    }
}

fn u64_at(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|x| {
        x.as_u64()
            .or_else(|| x.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
    })
}

fn f64_at(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

impl Usage {
    /// Merges what one JSON object (a response body or one SSE event) says. Understands the
    /// OpenAI chat/completions, Responses, Anthropic Messages and llama-server `timings`
    /// shapes; later events override earlier ones.
    pub fn absorb(&mut self, v: &serde_json::Value) {
        // Responses stream events wrap the response object; Anthropic `message_start` wraps
        // the message.
        for inner in ["response", "message"] {
            if let Some(o) = v.get(inner).filter(|o| o.is_object()) {
                self.absorb(o);
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            if let Some(n) = u64_at(u, "prompt_tokens").or_else(|| u64_at(u, "input_tokens")) {
                self.prompt_tokens = Some(n);
            }
            if let Some(n) = u64_at(u, "completion_tokens").or_else(|| u64_at(u, "output_tokens")) {
                self.completion_tokens = Some(n);
            }
            if let Some(n) = u
                .get("completion_tokens_details")
                .or_else(|| u.get("output_tokens_details"))
                .and_then(|d| u64_at(d, "reasoning_tokens"))
            {
                self.reasoning_tokens = Some(n);
            }
            let cached = u
                .get("prompt_tokens_details")
                .or_else(|| u.get("input_tokens_details"))
                .and_then(|d| u64_at(d, "cached_tokens"))
                .or_else(|| u64_at(u, "cache_read_input_tokens"));
            if let Some(n) = cached {
                self.cached_tokens = Some(n);
            }
        }
        if let Some(t) = v.get("timings").filter(|t| t.is_object()) {
            if let Some(n) = u64_at(t, "prompt_n") {
                self.prompt_tokens.get_or_insert(n);
            }
            if let Some(n) = u64_at(t, "predicted_n") {
                self.completion_tokens.get_or_insert(n);
            }
            if let Some(n) = u64_at(t, "cache_n") {
                self.cached_tokens = Some(n);
            }
            self.prompt_ms = f64_at(t, "prompt_ms").or(self.prompt_ms);
            self.predicted_ms = f64_at(t, "predicted_ms").or(self.predicted_ms);
            self.prompt_per_second = f64_at(t, "prompt_per_second").or(self.prompt_per_second);
            self.predicted_per_second =
                f64_at(t, "predicted_per_second").or(self.predicted_per_second);
            self.draft_n = u64_at(t, "draft_n").or(self.draft_n);
            self.draft_n_accepted = u64_at(t, "draft_n_accepted").or(self.draft_n_accepted);
        }
        let finish = v
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("finish_reason"))
            .and_then(|f| f.as_str())
            .or_else(|| v.get("stop_reason").and_then(|s| s.as_str()))
            .or_else(|| {
                v.get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|s| s.as_str())
            })
            .or_else(|| {
                // Responses: `status` of the response object (not of stream events).
                (v.get("object").and_then(|o| o.as_str()) == Some("response"))
                    .then(|| v.get("status").and_then(|s| s.as_str()))
                    .flatten()
            });
        if let Some(f) = finish {
            self.finish_reason = Some(f.to_string());
        }
    }
}

/// Reads [`Usage`] from a response as its bytes pass by.
///
/// * JSON bodies: kept (up to [`JSON_LIMIT`]) and parsed once at the end.
/// * SSE: complete `data:` lines are parsed only when they can carry usage (`usage`,
///   `timings`, `finish_reason`, `stop_reason`, `response.completed`), so token deltas cost a
///   substring scan.
#[derive(Debug, Default)]
pub struct UsageObserver {
    sse: bool,
    line: Vec<u8>,
    json: Vec<u8>,
    overflow: bool,
    pub usage: Usage,
}

/// Largest non-streaming body the observer keeps for parsing.
pub const JSON_LIMIT: usize = 4 * 1024 * 1024;

impl UsageObserver {
    pub fn new(sse: bool) -> Self {
        Self {
            sse,
            ..Default::default()
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        if !self.sse {
            if !self.overflow {
                if self.json.len() + chunk.len() > JSON_LIMIT {
                    self.overflow = true;
                    self.json = Vec::new();
                } else {
                    self.json.extend_from_slice(chunk);
                }
            }
            return;
        }
        let mut rest = chunk;
        while let Some(pos) = rest.iter().position(|&b| b == b'\n') {
            self.line.extend_from_slice(&rest[..pos]);
            let line = std::mem::take(&mut self.line);
            self.line_done(&line);
            rest = &rest[pos + 1..];
        }
        if self.line.len() + rest.len() <= JSON_LIMIT {
            self.line.extend_from_slice(rest);
        } else {
            self.line.clear();
        }
    }

    fn line_done(&mut self, line: &[u8]) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else {
            return;
        };
        const KEYS: [&[u8]; 4] = [
            b"\"usage\"",
            b"\"timings\"",
            b"finish_reason",
            b"stop_reason",
        ];
        if !KEYS.iter().any(|k| data.windows(k.len()).any(|w| w == *k)) {
            return;
        }
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(data.trim_ascii()) {
            self.usage.absorb(&v);
        }
    }

    /// Call once when the body ended.
    pub fn finish(&mut self) {
        if self.sse {
            let line = std::mem::take(&mut self.line);
            self.line_done(&line);
        } else if !self.overflow && !self.json.is_empty() {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&self.json) {
                self.usage.absorb(&v);
            }
            self.json = Vec::new();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_label_order() {
        let c = ClientInfo::new(
            Some("opencode/0.9.1 ai-sdk/5".into()),
            Some("my-agent".into()),
            Some("ci".into()),
        );
        assert_eq!(c.label.as_deref(), Some("my-agent"));
        let c = ClientInfo::new(Some("claude-cli/2.1.3 (external, cli)".into()), None, None);
        assert_eq!(c.label.as_deref(), Some("claude-cli"));
        let c = ClientInfo::new(None, None, Some("codex".into()));
        assert_eq!(c.label.as_deref(), Some("codex"));
    }

    #[test]
    fn usage_from_shapes() {
        // llama-server b11236 chat completion (captured, content removed).
        let chat = serde_json::json!({
            "choices": [{"finish_reason": "stop", "index": 0}],
            "object": "chat.completion",
            "usage": {"completion_tokens": 9, "prompt_tokens": 14, "total_tokens": 23,
                      "prompt_tokens_details": {"cached_tokens": 0}},
            "timings": {"cache_n": 0, "prompt_n": 14, "prompt_ms": 21.5, "predicted_n": 9,
                        "predicted_ms": 80.2, "predicted_per_second": 112.2}
        });
        let mut u = Usage::default();
        u.absorb(&chat);
        assert_eq!(u.prompt_tokens, Some(14));
        assert_eq!(u.completion_tokens, Some(9));
        assert_eq!(u.cached_tokens, Some(0));
        assert_eq!(u.finish_reason.as_deref(), Some("stop"));
        assert_eq!(u.predicted_per_second, Some(112.2));

        u.absorb(
            &serde_json::json!({"usage":{"completion_tokens_details":{"reasoning_tokens":5}}}),
        );
        assert_eq!(u.reasoning_tokens, Some(5));
        u.absorb(&serde_json::json!({"usage":{"output_tokens_details":{"reasoning_tokens":8}}}));
        assert_eq!(u.reasoning_tokens, Some(8));

        // Anthropic stream: message_start (input), message_delta (output + stop_reason).
        let mut o = UsageObserver::new(true);
        o.feed(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":31,\"output_tokens\":0}}}\n\n");
        o.feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hi\"}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_to");
        o.feed(b"kens\":42}}\n\n");
        o.finish();
        assert_eq!(o.usage.prompt_tokens, Some(31));
        assert_eq!(o.usage.completion_tokens, Some(42));
        assert_eq!(o.usage.finish_reason.as_deref(), Some("tool_use"));

        // Responses: response.completed carries the response object.
        let mut o = UsageObserver::new(true);
        o.feed(b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"object\":\"response\",\"status\":\"completed\",\"usage\":{\"input_tokens\":50,\"output_tokens\":7}}}\n\n");
        o.finish();
        assert_eq!(o.usage.prompt_tokens, Some(50));
        assert_eq!(o.usage.finish_reason.as_deref(), Some("completed"));

        // JSON body split in chunks.
        let mut o = UsageObserver::new(false);
        o.feed(b"{\"usage\":{\"prompt_tokens\":3,");
        o.feed(b"\"completion_tokens\":4}}");
        o.finish();
        assert_eq!(o.usage.completion_tokens, Some(4));
    }
}
