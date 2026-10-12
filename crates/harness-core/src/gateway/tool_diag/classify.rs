//! The classification itself: a pure function over what the request and the response said.
//!
//! Four places can break a tool round, and the cure differs, so the diagnosis names the place:
//!
//! * **runtime** (llama-server and its chat template): the model wrote a call that was not
//!   parsed, the template has no tool support, or the streamed call parts are broken;
//! * **context** (configuration): the prompt did not fit or nearly filled the context, or the
//!   output limit cut the call off;
//! * **model**: invalid JSON arguments, an undeclared tool name, arguments that break the
//!   tool's schema, the same call over and over, no call although one was demanded;
//! * **client** (the agent tool, e.g. OpenCode): the tool ran but reported an error; seen in the
//!   result the client sends with its *next* request.
//!
//! One round gets at most one primary issue (the first rule that fires, in the order above
//! with hard evidence first); a client-reported tool error that was *not* the primary issue
//! is kept in the detail as `also`. Rows without an issue still count as measurements (a clean
//! call, or a model that answered in text).

use serde::Serialize;
use serde_json::{json, Value};

use super::request::{canonical_args, ResultError, ToolChoice, ToolRequest};
use super::response::ToolResponse;
use super::schema;

/// Where the problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Runtime,
    Context,
    Model,
    Client,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Runtime => "runtime",
            Category::Context => "context",
            Category::Model => "model",
            Category::Client => "client",
        }
    }
}

/// What exactly went wrong (stable strings: stored in `analytics.db`, translated by the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    /// runtime: the model wrote a call as text (`<tools>`, `<tool_call>`, a bare JSON object)
    /// and llama-server did not turn it into `tool_calls`.
    CallLeftInContent,
    /// runtime: tools were sent but the chat template has no tool handling (`/props`
    /// `chat_template_caps.supports_tools` is false), so the model never saw them.
    TemplateNoTools,
    /// runtime: the streamed call parts are broken (no name, no `function`, bad chunk) or the
    /// finish reason promised a call that is not there.
    StreamBroken,
    /// context: the upstream refused the prompt (`exceed_context_size_error`).
    ContextExceeded,
    /// context: prompt and answer filled 90 % or more of the context window of the slot.
    ContextNearFull,
    /// context: the output limit (`max_tokens`) cut the answer inside a tool call.
    OutputTruncated,
    /// model: the arguments are not a JSON object.
    InvalidJsonArgs,
    /// model: the call names a tool the request did not declare.
    UnknownTool,
    /// model: the arguments break the declared schema.
    SchemaMismatch,
    /// model: the same call (name and arguments) for the third time.
    RepeatedCall,
    /// model: `tool_choice` demanded a call and there is none.
    NoCallWhenRequired,
    /// client: the tool result of the previous round is an error.
    ToolResultError,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::CallLeftInContent => "call_left_in_content",
            Code::TemplateNoTools => "template_no_tools",
            Code::StreamBroken => "stream_broken",
            Code::ContextExceeded => "context_exceeded",
            Code::ContextNearFull => "context_near_full",
            Code::OutputTruncated => "output_truncated",
            Code::InvalidJsonArgs => "invalid_json_args",
            Code::UnknownTool => "unknown_tool",
            Code::SchemaMismatch => "schema_mismatch",
            Code::RepeatedCall => "repeated_call",
            Code::NoCallWhenRequired => "no_call_when_required",
            Code::ToolResultError => "tool_result_error",
        }
    }

    pub fn category(self) -> Category {
        match self {
            Code::CallLeftInContent | Code::TemplateNoTools | Code::StreamBroken => {
                Category::Runtime
            }
            Code::ContextExceeded | Code::ContextNearFull | Code::OutputTruncated => {
                Category::Context
            }
            Code::InvalidJsonArgs
            | Code::UnknownTool
            | Code::SchemaMismatch
            | Code::RepeatedCall
            | Code::NoCallWhenRequired => Category::Model,
            Code::ToolResultError => Category::Client,
        }
    }
}

/// A problem found in one round.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub code: Code,
    /// Evidence (tool name, excerpt, schema path...), a JSON object.
    pub detail: Value,
}

impl Issue {
    pub fn category(&self) -> Category {
        self.code.category()
    }
}

/// The verdict on one round.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolDiag {
    /// The round involves tools at all (offered, answered or reported on).
    pub involved: bool,
    /// Tools the request offered.
    pub tools_offered: u32,
    /// Tool calls the response made.
    pub calls: u32,
    pub issue: Option<Issue>,
}

/// What the gateway knows about the finished round besides the two bodies.
#[derive(Debug, Clone, Default)]
pub struct Round<'a> {
    /// HTTP status returned to the client.
    pub status: u16,
    pub finish_reason: Option<&'a str>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    /// Context window of one slot of the server.
    pub ctx_size: Option<u64>,
    /// `chat_template_caps.supports_tools` of the server's template (`None`: not known).
    pub template_tools: Option<bool>,
}

/// Fraction of the context window from which a tool round counts as a context problem.
pub const NEAR_FULL: f64 = 0.90;

/// Tags a model writes around a tool call that a parser did not recognise. `<tools>` is what
/// Qwen2.5-Coder writes with Qwen's own template (measured, b11342); the others are the formats
/// of the template families llama.cpp knows.
const CALL_MARKERS: &[&str] = &[
    "<tool_call>",
    "<tools>",
    "<function=",
    "<function_call>",
    "[TOOL_CALLS]",
    "<|python_tag|>",
    "<|tool_call|>",
    "<|tool_call_start|>",
    "<start_function_call>",
    "<｜tool▁call▁begin｜>",
    "<|channel|>commentary to=",
];

/// Finds a tool call written as text: one of the [`CALL_MARKERS`] anywhere, or an answer that
/// starts with a JSON object that looks like a call (`name` + `arguments` / `parameters`).
/// Returns the marker (`json` for the bare object).
pub fn text_call_marker(text: &str) -> Option<&'static str> {
    if let Some(m) = CALL_MARKERS.iter().find(|m| text.contains(**m)) {
        return Some(m);
    }
    let t = text.trim_start();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t)
        .trim_start();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    let looks = |v: &Value| {
        v.get("name").is_some()
            && (v.get("arguments").is_some()
                || v.get("parameters").is_some()
                || v.get("input").is_some())
    };
    match serde_json::from_str::<Value>(t.trim_end().trim_end_matches("```").trim_end()) {
        Ok(v @ Value::Object(_)) if looks(&v) => Some("json"),
        Ok(Value::Array(a)) if a.first().is_some_and(looks) => Some("json"),
        Ok(_) => None,
        // Cut off or followed by prose: judge by the opening.
        Err(_) => {
            let head: String = t.chars().filter(|c| !c.is_whitespace()).take(10).collect();
            head.starts_with("{\"name\"").then_some("json")
        }
    }
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The first issue of the response itself (runtime, context, model), `None` when it is clean.
fn response_issue(req: &ToolRequest, resp: &ToolResponse, round: &Round<'_>) -> Option<Issue> {
    let finish = round.finish_reason.unwrap_or("");
    let offered = !req.tools.is_empty();
    let made_call = !resp.calls.is_empty();
    let truncated = matches!(finish, "length" | "max_tokens");

    // Hard errors of the upstream: the prompt did not fit.
    if let Some(e) = &resp.error {
        if e.kind.as_deref() == Some("exceed_context_size_error") {
            return Some(Issue {
                code: Code::ContextExceeded,
                detail: json!({"n_prompt_tokens": e.n_prompt_tokens, "n_ctx": e.n_ctx}),
            });
        }
        return None; // another upstream error: the status already says it
    }
    if round.status >= 400 {
        return None;
    }

    let fill = match (round.ctx_size, round.prompt_tokens) {
        (Some(c), Some(p)) if c > 0 => {
            Some((p + round.completion_tokens.unwrap_or(0)) as f64 / c as f64)
        }
        _ => None,
    };

    // Cut off inside the call: the arguments (or the markup) end mid-way.
    let marker = text_call_marker(&resp.text);
    if truncated && (made_call || marker.is_some()) {
        return Some(Issue {
            code: Code::OutputTruncated,
            detail: json!({
                "finish_reason": finish,
                "completion_tokens": round.completion_tokens,
                "in_call": made_call,
            }),
        });
    }

    // Runtime: the template never offered the tools.
    if offered && !made_call && round.template_tools == Some(false) {
        return Some(Issue {
            code: Code::TemplateNoTools,
            detail: json!({"finish_reason": finish}),
        });
    }
    // Runtime: the call is in the text.
    if offered && !made_call {
        if let Some(m) = marker {
            return Some(Issue {
                code: Code::CallLeftInContent,
                detail: json!({"marker": m, "excerpt": clip(resp.text.trim(), 160)}),
            });
        }
    }
    // Runtime: broken stream parts, or a promised call that is missing.
    if let Some(why) = resp.malformed {
        return Some(Issue {
            code: Code::StreamBroken,
            detail: json!({"reason": why}),
        });
    }
    if !made_call && matches!(finish, "tool_calls" | "tool_use") {
        return Some(Issue {
            code: Code::StreamBroken,
            detail: json!({"reason": "finish_without_call"}),
        });
    }

    // Context: nearly full while the round did not produce a call.
    if offered && !made_call {
        if let Some(f) = fill.filter(|f| *f >= NEAR_FULL) {
            return Some(Issue {
                code: Code::ContextNearFull,
                detail: json!({
                    "fill": (f * 1000.0).round() / 1000.0,
                    "prompt_tokens": round.prompt_tokens,
                    "ctx_size": round.ctx_size,
                }),
            });
        }
    }

    // Model: look at each call.
    for call in &resp.calls {
        let args_text = call.args.trim();
        let parsed: Result<Value, _> = serde_json::from_str(if args_text.is_empty() {
            "{}"
        } else {
            args_text
        });
        let value = match parsed {
            Ok(v @ Value::Object(_)) => v,
            _ => {
                return Some(Issue {
                    code: Code::InvalidJsonArgs,
                    detail: json!({"tool": call.name, "excerpt": clip(args_text, 120)}),
                })
            }
        };
        if offered {
            match req.tool(&call.name) {
                None => {
                    let declared: Vec<&str> = req.tool_names().into_iter().take(20).collect();
                    return Some(Issue {
                        code: Code::UnknownTool,
                        detail: json!({"tool": call.name, "declared": declared}),
                    });
                }
                Some(decl) => {
                    if let Some(s) = &decl.schema {
                        if let Err(v) = schema::validate(s, &value) {
                            return Some(Issue {
                                code: Code::SchemaMismatch,
                                detail: json!({"tool": call.name, "path": v.path, "rule": v.rule}),
                            });
                        }
                    }
                }
            }
        }
        let canon = canonical_args(args_text);
        let before = req
            .history
            .iter()
            .filter(|h| h.name == call.name && h.args == canon)
            .count();
        if before >= 2 {
            return Some(Issue {
                code: Code::RepeatedCall,
                detail: json!({"tool": call.name, "times": before + 1}),
            });
        }
    }

    // Model: a call was demanded.
    if !made_call {
        let demanded = match &req.choice {
            ToolChoice::Required => Some("required".to_string()),
            ToolChoice::Named(n) => Some(n.clone()),
            _ => None,
        };
        if let (true, Some(choice)) = (offered, demanded) {
            return Some(Issue {
                code: Code::NoCallWhenRequired,
                detail: json!({"tool_choice": choice, "finish_reason": finish}),
            });
        }
    }
    None
}

/// The client-reported tool error of the previous round (the first erroring result).
fn client_issue(req: &ToolRequest) -> Option<Issue> {
    let r = req.results.iter().find(|r| r.error.is_some())?;
    let how: ResultError = r.error?;
    Some(Issue {
        code: Code::ToolResultError,
        detail: json!({
            "call_id": r.call_id,
            "tool": r.call_id.as_deref().and_then(|id| req.call_name(id)),
            "how": how.as_str(),
            "excerpt": r.excerpt,
        }),
    })
}

/// Classifies one finished round. Pure: the same inputs always give the same verdict.
pub fn classify(req: &ToolRequest, resp: &ToolResponse, round: &Round<'_>) -> ToolDiag {
    let finish_call = matches!(
        round.finish_reason,
        Some("tool_calls" | "tool_use" | "function_call")
    );
    let involved =
        !req.tools.is_empty() || req.has_tool_messages || !resp.calls.is_empty() || finish_call;
    let mut diag = ToolDiag {
        involved,
        tools_offered: req.tools.len() as u32,
        calls: resp.calls.len() as u32,
        issue: None,
    };
    if !involved {
        return diag;
    }
    let primary = response_issue(req, resp, round);
    let client = client_issue(req);
    diag.issue = match (primary, client) {
        (Some(mut p), Some(c)) => {
            if let Some(o) = p.detail.as_object_mut() {
                o.insert("also".into(), json!([c.code.as_str()]));
                o.insert("client".into(), c.detail);
            }
            Some(p)
        }
        (Some(p), None) => Some(p),
        (None, c) => c,
    };
    diag
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::tool_diag::response::ToolObserver;

    /// Real bytes captured from llama-server b11342 (see `tests/data/tooldiag`).
    macro_rules! fx {
        ($name:literal) => {
            include_bytes!(concat!("../../../tests/data/tooldiag/b11342-", $name)).as_slice()
        };
    }

    fn observe(sse: bool, body: &[u8]) -> ToolResponse {
        let mut o = ToolObserver::new(sse);
        // Feed in odd-sized pieces: the split must not matter.
        for piece in body.chunks(97) {
            o.feed(piece);
        }
        o.finish()
    }

    fn round(finish: &str) -> Round<'_> {
        Round {
            status: 200,
            finish_reason: Some(finish),
            ..Default::default()
        }
    }

    fn issue_of(d: &ToolDiag) -> Option<(Category, Code)> {
        d.issue.as_ref().map(|i| (i.category(), i.code))
    }

    // ── clean rounds ────────────────────────────────────────────────────────

    #[test]
    fn clean_native_call_in_every_wire_format() {
        // (label, request, response, streamed, finish reason)
        type Case = (
            &'static str,
            &'static [u8],
            &'static [u8],
            bool,
            &'static str,
        );
        let cases: [Case; 6] = [
            (
                "chat",
                fx!("qwen3-4b.chat.req.json"),
                fx!("qwen3-4b.chat.resp.json"),
                false,
                "tool_calls",
            ),
            (
                "chat stream",
                fx!("qwen3-4b.chat-stream.req.json"),
                fx!("qwen3-4b.chat-stream.resp.sse"),
                true,
                "tool_calls",
            ),
            (
                "anthropic",
                fx!("qwen3-4b.anthropic.req.json"),
                fx!("qwen3-4b.anthropic.resp.json"),
                false,
                "tool_use",
            ),
            (
                "anthropic stream",
                fx!("qwen3-4b.anthropic-stream.req.json"),
                fx!("qwen3-4b.anthropic-stream.resp.sse"),
                true,
                "tool_use",
            ),
            (
                "responses",
                fx!("qwen3-4b.responses.req.json"),
                fx!("qwen3-4b.responses.resp.json"),
                false,
                "completed",
            ),
            (
                "responses stream",
                fx!("qwen3-4b.responses-stream.req.json"),
                fx!("qwen3-4b.responses-stream.resp.sse"),
                true,
                "completed",
            ),
        ];
        for (name, req, resp, sse, finish) in cases {
            let req = ToolRequest::parse(req).unwrap();
            let resp = observe(sse, resp);
            assert_eq!(resp.calls.len(), 1, "{name}: calls");
            assert_eq!(resp.calls[0].name, "get_weather", "{name}");
            let args: Value = serde_json::from_str(&resp.calls[0].args).unwrap();
            assert!(
                args["city"].as_str().unwrap().contains("Ankara"),
                "{name}: {args}"
            );
            let d = classify(&req, &resp, &round(finish));
            assert!(d.involved, "{name}");
            assert_eq!((d.tools_offered, d.calls), (1, 1), "{name}");
            assert_eq!(d.issue, None, "{name}");
        }
    }

    #[test]
    fn a_text_answer_without_a_call_is_not_an_issue() {
        // Tools offered, the model chose to answer in text (auto).
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-notool.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat-notool.resp.json"));
        let d = classify(&req, &resp, &round("stop"));
        assert!(d.involved);
        assert_eq!((d.tools_offered, d.calls), (1, 0));
        assert_eq!(d.issue, None);
    }

    #[test]
    fn a_request_without_tools_is_not_involved() {
        let req =
            ToolRequest::parse(br#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#)
                .unwrap();
        let resp = observe(
            false,
            br#"{"choices":[{"finish_reason":"stop","message":{"content":"hi"}}]}"#,
        );
        let d = classify(&req, &resp, &round("stop"));
        assert_eq!(d, ToolDiag::default());
    }

    #[test]
    fn the_answer_after_a_good_tool_result_is_clean() {
        let req = ToolRequest::parse(fx!("qwen3-4b.chat2.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat2.resp.json"));
        let d = classify(&req, &resp, &round("stop"));
        assert!(d.involved);
        assert_eq!(d.issue, None);
    }

    // ── runtime ─────────────────────────────────────────────────────────────

    #[test]
    fn call_written_as_text_by_qwen_coder_is_a_runtime_issue() {
        // Qwen2.5-Coder-14B writes `<tools>{"name": ...}</tools>`; llama-server returns it as text.
        let req = ToolRequest::parse(fx!("qwen2.5-coder-14b.chat.req.json")).unwrap();
        let resp = observe(false, fx!("qwen2.5-coder-14b.chat.resp.json"));
        assert!(resp.calls.is_empty());
        let d = classify(&req, &resp, &round("stop"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Runtime, Code::CallLeftInContent))
        );
        let detail = &d.issue.unwrap().detail;
        assert_eq!(detail["marker"], "<tools>");
        assert!(detail["excerpt"].as_str().unwrap().contains("get_weather"));
    }

    #[test]
    fn the_same_in_a_stream() {
        let req = ToolRequest::parse(fx!("qwen2.5-coder-14b.chat-stream.req.json")).unwrap();
        let resp = observe(true, fx!("qwen2.5-coder-14b.chat-stream.resp.sse"));
        assert!(resp.text.contains("<tools>"), "{}", resp.text);
        let d = classify(&req, &resp, &round("stop"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Runtime, Code::CallLeftInContent))
        );
    }

    #[test]
    fn a_text_call_that_breaks_the_schema_is_still_a_runtime_issue_first() {
        // Qwen2.5-Coder writes `days: 10` (schema maximum 3) as text: the call never became a
        // call, so the runtime finding comes first; the same model answers a prompt that
        // mentions a tool it was never offered with the tool list and a call as text.
        for case in ["range", "unknown"] {
            let (req, resp) = match case {
                "range" => (
                    fx!("qwen2.5-coder-14b.range.req.json"),
                    fx!("qwen2.5-coder-14b.range.resp.json"),
                ),
                _ => (
                    fx!("qwen2.5-coder-14b.unknown.req.json"),
                    fx!("qwen2.5-coder-14b.unknown.resp.json"),
                ),
            };
            let req = ToolRequest::parse(req).unwrap();
            let resp = observe(false, resp);
            assert!(resp.calls.is_empty(), "{case}");
            let d = classify(&req, &resp, &round("stop"));
            assert_eq!(
                issue_of(&d),
                Some((Category::Runtime, Code::CallLeftInContent)),
                "{case}"
            );
        }
    }

    #[test]
    fn template_without_tool_support_is_named_before_anything_else() {
        // gemma-3-4b: `chat_template_caps.supports_tools` is false (b11342 /props), the model
        // never sees the tools and answers in prose.
        let req = ToolRequest::parse(fx!("gemma-3-4b.chat.req.json")).unwrap();
        let resp = observe(false, fx!("gemma-3-4b.chat.resp.json"));
        let r = Round {
            template_tools: Some(false),
            ..round("length")
        };
        let d = classify(&req, &resp, &r);
        assert_eq!(
            issue_of(&d),
            Some((Category::Runtime, Code::TemplateNoTools))
        );
        // Without the template knowledge the round is just a text answer.
        let d = classify(&req, &resp, &round("length"));
        assert_eq!(d.issue, None);
    }

    #[test]
    fn broken_streams() {
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-stream.req.json")).unwrap();
        // The real stream with the name of the call cut out: the call has no name.
        let text = String::from_utf8_lossy(fx!("qwen3-4b.chat-stream.resp.sse"))
            .replace("\"name\":\"get_weather\",", "");
        let d = classify(&req, &observe(true, text.as_bytes()), &round("tool_calls"));
        assert_eq!(issue_of(&d), Some((Category::Runtime, Code::StreamBroken)));
        assert_eq!(d.issue.unwrap().detail["reason"], "call_without_name");
        // Finish reason says tool call, nothing arrived.
        let text = String::from_utf8_lossy(fx!("qwen3-4b.chat-stream.resp.sse"))
            .lines()
            .filter(|l| !l.contains("tool_calls\":[{"))
            .collect::<Vec<_>>()
            .join("\n");
        let d = classify(&req, &observe(true, text.as_bytes()), &round("tool_calls"));
        assert_eq!(issue_of(&d), Some((Category::Runtime, Code::StreamBroken)));
        assert_eq!(d.issue.unwrap().detail["reason"], "finish_without_call");
    }

    // ── context ─────────────────────────────────────────────────────────────

    #[test]
    fn prompt_over_the_context_is_refused_by_the_upstream() {
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-ctx-exceeded.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat-ctx-exceeded.resp.json"));
        let d = classify(
            &req,
            &resp,
            &Round {
                status: 400,
                ..Default::default()
            },
        );
        assert_eq!(
            issue_of(&d),
            Some((Category::Context, Code::ContextExceeded))
        );
        let detail = d.issue.unwrap().detail;
        assert_eq!(detail["n_prompt_tokens"], 1380);
        assert_eq!(detail["n_ctx"], 512);
    }

    #[test]
    fn output_limit_cut_inside_the_call() {
        // max_tokens 8: the markup is cut after the tool name; max_tokens 14: the arguments.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-trunc8.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat-trunc8.resp.json"));
        assert!(resp.calls.is_empty());
        let d = classify(&req, &resp, &round("length"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Context, Code::OutputTruncated))
        );
        assert_eq!(d.issue.unwrap().detail["in_call"], false);

        let req = ToolRequest::parse(fx!("qwen3-4b.chat-trunc14.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat-trunc14.resp.json"));
        assert_eq!(resp.calls[0].args, "{\"city");
        let d = classify(&req, &resp, &round("length"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Context, Code::OutputTruncated))
        );
        assert_eq!(d.issue.unwrap().detail["in_call"], true);

        let resp = observe(true, fx!("qwen3-4b.chat-stream-trunc14.resp.sse"));
        let d = classify(&req, &resp, &round("length"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Context, Code::OutputTruncated))
        );
    }

    #[test]
    fn a_nearly_full_context_without_a_call() {
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-notool.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat-notool.resp.json"));
        let full = Round {
            prompt_tokens: Some(1900),
            completion_tokens: Some(10),
            ctx_size: Some(2048),
            ..round("stop")
        };
        let d = classify(&req, &resp, &full);
        assert_eq!(
            issue_of(&d),
            Some((Category::Context, Code::ContextNearFull))
        );
        let roomy = Round {
            prompt_tokens: Some(1500),
            ..full.clone()
        };
        assert_eq!(classify(&req, &resp, &roomy).issue, None);
        // A round that made its call fine is not blamed on the context.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat.resp.json"));
        assert_eq!(classify(&req, &resp, &full).issue, None);
    }

    // ── model ───────────────────────────────────────────────────────────────

    #[test]
    fn a_tool_that_the_request_did_not_declare() {
        // Real call to get_weather, answered to a request that only offers get_forecast.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-range.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat.resp.json"));
        let d = classify(&req, &resp, &round("tool_calls"));
        assert_eq!(issue_of(&d), Some((Category::Model, Code::UnknownTool)));
        let detail = d.issue.unwrap().detail;
        assert_eq!(detail["tool"], "get_weather");
        assert_eq!(detail["declared"][0], "get_forecast");
    }

    #[test]
    fn arguments_against_the_declared_schema() {
        // The model's real answer {"city": "Ankara"} against the schema of the request that also
        // requires `country_code`.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-req2.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat.resp.json"));
        let d = classify(&req, &resp, &round("tool_calls"));
        assert_eq!(issue_of(&d), Some((Category::Model, Code::SchemaMismatch)));
        let detail = d.issue.unwrap().detail;
        assert_eq!(
            (detail["path"].as_str(), detail["rule"].as_str()),
            (Some("country_code"), Some("required"))
        );
        // The range schema (days <= 3) against what the Coder model wrote as text is not a call;
        // against a real parsed call with days: 1 it fits.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-range.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat-range.resp.json"));
        assert_eq!(classify(&req, &resp, &round("tool_calls")).issue, None);
    }

    #[test]
    fn invalid_json_arguments_when_the_call_finished() {
        // The real answer with the arguments broken the way a model breaks them.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat.req.json")).unwrap();
        let text = String::from_utf8_lossy(fx!("qwen3-4b.chat.resp.json"))
            .replace(r#"\"city\": \"Ankara\""#, r#"city: \"Ankara\""#);
        let resp = observe(false, text.as_bytes());
        assert_eq!(resp.calls[0].args, "{city: \"Ankara\"}");
        let d = classify(&req, &resp, &round("tool_calls"));
        assert_eq!(issue_of(&d), Some((Category::Model, Code::InvalidJsonArgs)));
    }

    #[test]
    fn the_same_call_for_the_third_time_is_a_loop() {
        // Qwen2.5-0.5B and gpt-oss-20b both repeat get_weather("Ankara") after three tries.
        for (req, resp) in [
            (
                fx!("qwen2.5-0.5b.loop.req.json"),
                fx!("qwen2.5-0.5b.loop.resp.json"),
            ),
            (
                fx!("gpt-oss-20b.loop.req.json"),
                fx!("gpt-oss-20b.loop.resp.json"),
            ),
        ] {
            let req = ToolRequest::parse(req).unwrap();
            assert_eq!(req.history.len(), 3);
            let resp = observe(false, resp);
            let d = classify(&req, &resp, &round("tool_calls"));
            assert_eq!(issue_of(&d), Some((Category::Model, Code::RepeatedCall)));
            assert_eq!(d.issue.unwrap().detail["times"], 4);
        }
        // A second attempt is not a loop (a retry is legitimate); the third is.
        let mut req = ToolRequest::parse(fx!("qwen2.5-0.5b.loop.req.json")).unwrap();
        req.history.truncate(1);
        req.results.clear();
        let resp = observe(false, fx!("qwen2.5-0.5b.loop.resp.json"));
        assert_eq!(classify(&req, &resp, &round("tool_calls")).issue, None);
        req.history.push(req.history[0].clone());
        let d = classify(&req, &resp, &round("tool_calls"));
        assert_eq!(d.issue.unwrap().detail["times"], 3);
    }

    #[test]
    fn no_call_although_one_was_required() {
        // llama-server (b11342) does not force a call for `tool_choice: required`: the model
        // answered "Hello!" until the token limit.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat-required.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat-required.resp.json"));
        let d = classify(&req, &resp, &round("length"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Model, Code::NoCallWhenRequired))
        );
        assert_eq!(d.issue.unwrap().detail["tool_choice"], "required");
    }

    // ── client ──────────────────────────────────────────────────────────────

    #[test]
    fn the_clients_tool_reported_an_error() {
        // The request carries the (real) tool call of the previous round and an error result;
        // the model's reaction (real) is a clean text answer, so the client gets the blame.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat2err.req.json")).unwrap();
        let resp = observe(false, fx!("qwen3-4b.chat2err.resp.json"));
        let d = classify(&req, &resp, &round("stop"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Client, Code::ToolResultError))
        );
        let detail = d.issue.unwrap().detail;
        assert_eq!(detail["tool"], "get_weather");
        assert_eq!(detail["how"], "text");
        assert!(detail["excerpt"]
            .as_str()
            .unwrap()
            .starts_with("Error: city not found"));
        assert_eq!(detail["call_id"], "6iAYqX7s3VcfGGpsXBGYmvqw3m4byXBm");
    }

    #[test]
    fn a_model_fault_outranks_the_clients_and_keeps_it_as_also() {
        // The model's reaction to the failed tool is again a call in text.
        let req = ToolRequest::parse(fx!("qwen3-4b.chat2err.req.json")).unwrap();
        let resp = observe(
            false,
            br#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"<tool_call>\n{\"name\": \"get_weather\", \"arguments\": {\"city\": \"Ankara\"}}\n</tool_call>"}}]}"#,
        );
        let d = classify(&req, &resp, &round("stop"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Runtime, Code::CallLeftInContent))
        );
        let detail = d.issue.unwrap().detail;
        assert_eq!(detail["also"][0], "tool_result_error");
        assert_eq!(detail["client"]["tool"], "get_weather");
    }

    #[test]
    fn anthropic_is_error_is_the_clients() {
        // What an agent client sends back (client-written bytes, not model output).
        let req = ToolRequest::parse(
            br#"{"model":"m","max_tokens":64,
            "tools":[{"name":"edit","input_schema":{"type":"object"}}],
            "messages":[{"role":"user","content":"fix a.py"},
              {"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"edit","input":{"path":"a.py"}}]},
              {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,
                "content":"No match found for the old string"}]}]}"#,
        )
        .unwrap();
        let resp = observe(false, br#"{"type":"message","content":[{"type":"text","text":"Let me look at the file."}],"stop_reason":"end_turn"}"#);
        let d = classify(&req, &resp, &round("end_turn"));
        assert_eq!(
            issue_of(&d),
            Some((Category::Client, Code::ToolResultError))
        );
        assert_eq!(d.issue.unwrap().detail["how"], "is_error");
    }

    #[test]
    fn the_text_call_marker() {
        assert_eq!(text_call_marker("<tool_call>\n{}"), Some("<tool_call>"));
        assert_eq!(
            text_call_marker("[TOOL_CALLS][{\"name\":\"a\"}]"),
            Some("[TOOL_CALLS]")
        );
        assert_eq!(
            text_call_marker("<|python_tag|>print(1)"),
            Some("<|python_tag|>")
        );
        assert_eq!(
            text_call_marker("{\"name\": \"edit\", \"arguments\": {\"p\": 1}}"),
            Some("json")
        );
        assert_eq!(
            text_call_marker("```json\n{\"name\": \"edit\", \"parameters\": {}}\n```"),
            Some("json")
        );
        // Cut off, but it opens like a call.
        assert_eq!(text_call_marker("{\"name\": \"get_wea"), Some("json"));
        // Prose and JSON that is not a call.
        assert_eq!(text_call_marker("The name of the tool is edit."), None);
        assert_eq!(text_call_marker("{\"temp_c\": 21}"), None);
        assert_eq!(
            text_call_marker("Here is the config: {\"name\": \"x\"}"),
            None
        );
    }
}
