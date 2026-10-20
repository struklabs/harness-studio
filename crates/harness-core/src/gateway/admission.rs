//! Token-budget admission: how many KV-pool tokens a request will need.
//!
//! llama-server with several slots and a unified KV cache (`--kv-unified`, Harness's default)
//! shares one `-c` sized pool between them and does not check admission: when the running
//! requests outgrow the pool it fails *all* of them ("Context size has been exceeded", see
//! private hs-internal architecture notesd). The scheduler therefore holds each request's estimate against the
//! pool (`Scheduler::acquire_tokens`) and queues the ones that do not fit, like TGI's
//! `max_batch_total_tokens` or vLLM's block budget.
//!
//! The demand is the prompt plus the output limit. The prompt is estimated from the text
//! (bytes / 3 is cautious for English, ~4 bytes per token; code and numbers run lower) and,
//! for long prompts, counted exactly by the server's own `/tokenize` before admission. Media
//! costs a fixed amount, and a request without an output limit gets a default allowance, since
//! llama-server would generate until the pool is full.

use serde_json::Value;

/// Bytes of text per token. Cautious: real English text is nearer 4.
pub const BYTES_PER_TOKEN: u64 = 3;
/// Tokens charged for one embedded image / audio clip (`data:` URL or Ollama `images`).
pub const MEDIA_TOKENS: u64 = 1024;
/// Per message / item framing (role markers, template tokens).
pub const MESSAGE_OVERHEAD: u64 = 8;
/// Output allowance for a request without its own limit. llama-server would generate until
/// the pool is full; reserving the whole pool for each such request would serialize every
/// agent, so a typical answer size is assumed. A longer answer can still overflow the pool
/// (as it did before admission existed).
pub const DEFAULT_GENERATION_TOKENS: u64 = 1024;

/// Output limit keys of the OpenAI Chat/Responses, Anthropic and Ollama request bodies.
const LIMIT_KEYS: [&str; 5] = [
    "max_tokens",
    "max_completion_tokens",
    "max_output_tokens",
    "n_predict",
    "num_predict",
];

/// What a request asks of the KV pool.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Demand {
    /// The prompt's text (message contents, tool schemas) without media: what `/tokenize`
    /// counts exactly and bytes / 3 estimates.
    pub text: String,
    /// Tokens that are not text: media (fixed charge each) and per-message framing.
    pub fixed: u64,
    /// Output allowance: the request's own limit, or [`DEFAULT_GENERATION_TOKENS`].
    pub output: u64,
}

impl Demand {
    pub fn of(body: &Value) -> Self {
        let mut d = Demand {
            output: requested_output_tokens(body).unwrap_or(DEFAULT_GENERATION_TOKENS),
            ..Default::default()
        };
        walk(body, None, &mut d);
        d
    }

    /// `None` for a body that is not JSON (the proxy rejects those anyway).
    pub fn from_bytes(body: &[u8]) -> Option<Self> {
        serde_json::from_slice::<Value>(body)
            .ok()
            .map(|v| Self::of(&v))
    }

    /// Cheap estimate of the prompt: text bytes / [`BYTES_PER_TOKEN`] + the fixed part.
    pub fn estimate_prompt(&self) -> u64 {
        (self.text.len() as u64).div_ceil(BYTES_PER_TOKEN) + self.fixed
    }

    /// Estimated tokens in the pool: prompt + output allowance.
    pub fn estimate(&self) -> u64 {
        self.estimate_prompt() + self.output
    }

    /// With the exact text token count (`/tokenize`) instead of the estimate.
    pub fn with_text_tokens(&self, text_tokens: u64) -> u64 {
        text_tokens + self.fixed + self.output
    }
}

/// The request's own output limit (`max_tokens`, `max_completion_tokens`,
/// `max_output_tokens`, `n_predict`, Ollama `options.num_predict`); `None` when absent or
/// unlimited (`-1`, `-2`, `0`).
pub fn requested_output_tokens(body: &Value) -> Option<u64> {
    let positive = |v: &Value| v.as_i64().filter(|n| *n > 0).map(|n| n as u64);
    let mut best: Option<u64> = None;
    for scope in [Some(body), body.get("options")].into_iter().flatten() {
        for key in LIMIT_KEYS {
            if let Some(n) = scope.get(key).and_then(positive) {
                best = Some(best.map_or(n, |b| b.min(n)));
            }
        }
    }
    best
}

fn is_media(s: &str) -> bool {
    s.starts_with("data:") && s.contains(";base64,")
}

fn walk(v: &Value, key: Option<&str>, d: &mut Demand) {
    match v {
        Value::String(s) => {
            if is_media(s) || key == Some("images") {
                d.fixed += MEDIA_TOKENS;
            } else if key != Some("model") {
                d.text.push_str(s);
                d.text.push('\n');
            }
        }
        Value::Array(items) => {
            // Ollama `images` is an array of bare base64 strings.
            for item in items {
                walk(item, key, d);
            }
        }
        Value::Object(map) => {
            if map.contains_key("role") || map.contains_key("type") {
                d.fixed += MESSAGE_OVERHEAD;
            }
            for (k, item) in map {
                walk(item, Some(k), d);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn demand(v: Value) -> Demand {
        Demand::of(&v)
    }

    #[test]
    fn text_counts_bytes_over_three_and_output_limit_is_added() {
        let text = "a".repeat(3000);
        let d = demand(json!({"model": "m", "max_tokens": 200,
            "messages": [{"role": "user", "content": text}]}));
        let prompt = d.estimate_prompt();
        assert!((1000..1100).contains(&prompt), "{prompt}");
        assert_eq!(d.output, 200);
        assert_eq!(d.estimate(), prompt + 200);
        assert_eq!(d.with_text_tokens(900), 900 + d.fixed + 200);
    }

    #[test]
    fn unlimited_or_missing_output_uses_the_default_allowance() {
        for limit in [json!(-1), json!(-2), json!(0), Value::Null] {
            let body = json!({"messages": [{"role": "user", "content": "hi"}],
                "max_tokens": limit});
            assert_eq!(requested_output_tokens(&body), None);
            assert_eq!(Demand::of(&body).output, DEFAULT_GENERATION_TOKENS);
        }
    }

    #[test]
    fn every_api_flavour_names_its_limit() {
        for (body, want) in [
            (json!({"max_completion_tokens": 7}), 7),
            (json!({"max_output_tokens": 8}), 8),
            (json!({"n_predict": 9}), 9),
            (json!({"options": {"num_predict": 10}}), 10),
            (json!({"max_tokens": 50, "max_completion_tokens": 20}), 20),
        ] {
            assert_eq!(requested_output_tokens(&body), Some(want), "{body}");
        }
    }

    #[test]
    fn images_cost_a_fixed_amount_not_their_base64_length() {
        let blob = format!("data:image/png;base64,{}", "A".repeat(2_000_000));
        let d = demand(json!({"messages": [{"role": "user", "content": [
            {"type": "text", "text": "what is this"},
            {"type": "image_url", "image_url": {"url": blob}}]}]}));
        assert!(d.estimate_prompt() < 1200, "{}", d.estimate_prompt());
        assert!(d.fixed >= MEDIA_TOKENS);
        assert!(!d.text.contains("AAAA"));
        let ollama =
            demand(json!({"model": "m", "prompt": "x", "images": ["QUJD".repeat(100000)]}));
        assert!(ollama.estimate_prompt() < 1200);
    }

    #[test]
    fn invalid_bodies_have_no_demand() {
        assert_eq!(Demand::from_bytes(b"not json"), None);
        let d = Demand::from_bytes(br#"{"input":"hello"}"#).unwrap();
        assert!(d.estimate() >= DEFAULT_GENERATION_TOKENS);
    }
}
