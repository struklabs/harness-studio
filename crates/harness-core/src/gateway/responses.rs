//! Stateful Responses (`store`, `previous_response_id`) for the gateway's `/v1/responses`.
//!
//! llama-server's Responses API is stateless (it rejects `previous_response_id`), so the
//! gateway keeps the conversation itself, like OpenAI:
//!
//! * `store` (default `true`) keeps the response in `state.db` (`gateway_responses`, schema
//!   v6): the request's own input items and the response object as the client got it.
//! * `previous_response_id` prepends the stored conversation (every link of the chain, oldest
//!   first: its input items, then its output items) to the new input before the request goes
//!   to llama-server. The `instructions` of earlier responses are **not** carried over.
//! * Stored responses belong to the caller ([`scope_of`]): a token never reads or continues
//!   another token's responses.
//! * Retention ([`ResponsesSettings`]): age (since last use), row count and total bytes; the
//!   least recently used rows go first. Nothing grows without bound.
//!
//! The pure helpers here (input normalization, context building, input-item pages) are used
//! by the HTTP side in `harness-gateway`; storage lives in `crate::state` (`StateDb::*_response*`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Defaults of [`ResponsesSettings`].
pub const DEFAULT_RETENTION_DAYS: u32 = 30;
pub const DEFAULT_MAX_STORED: u32 = 1000;
pub const DEFAULT_MAX_MIB: u32 = 100;
/// Longest chain followed for `previous_response_id` (a guard; chains are acyclic).
pub const MAX_CHAIN: usize = 10_000;

/// `settings.json` → `gateway.responses`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResponsesSettings {
    /// Store responses that ask for it (`store` is `true` unless the request says `false`).
    /// Off = nothing new is stored; already stored responses stay readable (and can be
    /// continued) until retention drops them or they are cleared.
    pub store: bool,
    /// Days a stored response is kept after it was created or last continued.
    pub retention_days: u32,
    /// Most stored responses kept (all callers together).
    pub max_stored: u32,
    /// Most bytes kept (JSON of input items and responses), MiB.
    pub max_mib: u32,
    /// Summarizing the older links of long chains.
    pub compaction: CompactionSettings,
}

impl Default for ResponsesSettings {
    fn default() -> Self {
        Self {
            store: true,
            retention_days: DEFAULT_RETENTION_DAYS,
            max_stored: DEFAULT_MAX_STORED,
            max_mib: DEFAULT_MAX_MIB,
            compaction: CompactionSettings::default(),
        }
    }
}

impl ResponsesSettings {
    /// Clamped to sane bounds (every limit at least 1, nothing unbounded).
    pub fn validated(&self) -> Self {
        Self {
            store: self.store,
            retention_days: self.retention_days.clamp(1, 3650),
            max_stored: self.max_stored.clamp(1, 100_000),
            max_mib: self.max_mib.clamp(1, 10_240),
            compaction: self.compaction.validated(),
        }
    }

    pub fn retention(&self) -> Retention {
        let s = self.validated();
        Retention {
            max_age_ms: i64::from(s.retention_days) * 86_400_000,
            max_rows: u64::from(s.max_stored),
            max_bytes: u64::from(s.max_mib) * 1024 * 1024,
        }
    }
}

/// Limits applied by `StateDb::prune_responses` (and hiding expired rows on read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    pub max_age_ms: i64,
    pub max_rows: u64,
    pub max_bytes: u64,
}

/// A fresh response id (`resp_` + 32 hex digits) for a background response, which has to
/// answer before llama-server has produced its own.
pub fn new_response_id() -> String {
    format!("resp_{}", uuid::Uuid::new_v4().simple())
}

/// Whether a response `status` is final (`completed`, `failed`, `incomplete`, `cancelled`).
pub fn is_final_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "incomplete" | "cancelled")
}

/// Who owns a stored response: a named token (`token:<id>`), Harness's own client token
/// (`harness`) or a caller without a token (`anonymous`, open loopback gateway only).
pub fn scope_of(token_id: Option<&str>, token_name: Option<&str>) -> String {
    match (token_id, token_name) {
        (Some(id), _) => format!("token:{id}"),
        (None, Some(_)) => "harness".to_string(),
        (None, None) => "anonymous".to_string(),
    }
}

/// The request's `input` as an item array: a string becomes one user message
/// (`{"type":"message","role":"user","content":[{"type":"input_text",...}]}`, as OpenAI
/// lists it); an array is kept; anything else (or nothing) is empty.
pub fn normalize_input(input: Option<&Value>) -> Vec<Value> {
    match input {
        Some(Value::String(s)) => vec![json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": s}],
        })],
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// Gives every item without an `id` a stable one (`msg_…` for messages, `item_…` otherwise),
/// so input-item pages can use `after`/`before` like OpenAI's.
pub fn assign_item_ids(items: &mut [Value], response_id: &str) {
    let tail: String = response_id
        .trim_start_matches("resp_")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(24)
        .collect();
    for (i, item) in items.iter_mut().enumerate() {
        let Some(obj) = item.as_object_mut() else {
            continue;
        };
        if obj.get("id").and_then(Value::as_str).is_some() {
            continue;
        }
        let is_message = obj.get("type").and_then(Value::as_str).unwrap_or("message") == "message"
            && obj.contains_key("role");
        let prefix = if is_message { "msg" } else { "item" };
        obj.insert("id".into(), json!(format!("{prefix}_{tail}_{i}")));
    }
}

/// One link of a stored conversation: the items a request sent and the output it got.
#[derive(Debug, Clone, PartialEq)]
pub struct Link<'a> {
    pub input: &'a [Value],
    pub response: &'a Value,
}

/// The conversation of a chain (oldest link first): for each link its input items, then the
/// `output` items of its response. `instructions` are never included.
pub fn chain_context(chain: &[Link<'_>]) -> Vec<Value> {
    let mut out = Vec::new();
    for link in chain {
        out.extend(link.input.iter().cloned());
        if let Some(items) = link.response.get("output").and_then(Value::as_array) {
            out.extend(items.iter().cloned());
        }
    }
    out
}

/// Context compaction of stored Responses chains (`gateway.responses.compaction`):
/// when the conversation a `previous_response_id` continues no longer fits
/// the model comfortably, its older links are replaced, in the request, by a summary the model
/// writes. The summary is stored on the new response (`harness_compaction`), so later turns
/// start from it instead of the whole chain. Always opt-in: a request asks for it with
/// `context_management: [{"type": "compaction", "compact_threshold": <tokens>}]`, or the
/// setting turns it on for every chained request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CompactionSettings {
    /// Compact every chained request that crosses the threshold, not only the ones that ask.
    pub auto: bool,
    /// Share of the model's context window (per slot) the conversation may fill before it is
    /// compacted.
    pub threshold_percent: u32,
    /// Newest links kept as they are (their input and output); older ones are summarized.
    pub keep_links: u32,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            auto: false,
            threshold_percent: 80,
            keep_links: 2,
        }
    }
}

impl CompactionSettings {
    pub fn validated(&self) -> Self {
        Self {
            auto: self.auto,
            threshold_percent: self.threshold_percent.clamp(10, 95),
            keep_links: self.keep_links.clamp(1, 50),
        }
    }
}

/// Key of the compaction record on a stored response object.
pub const COMPACTION_KEY: &str = "harness_compaction";

/// The compaction a response carries: the links up to and including `through` are replaced by
/// `summary` for every chain that continues from this response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainSummary {
    /// Response id of the newest link the summary covers.
    pub through: String,
    pub summary: String,
    /// Tokens of what it replaced and of what the context was after (summary + kept links).
    pub tokens_before: u64,
    pub tokens_after: u64,
    /// Alias of the model that wrote it.
    #[serde(default)]
    pub model: Option<String>,
    /// Links summarized by this compaction (not counting earlier summaries).
    #[serde(default)]
    pub links: u32,
    pub created_at: i64,
}

/// The compaction record of a stored response object, if it has one.
pub fn summary_of(response: &Value) -> Option<ChainSummary> {
    serde_json::from_value(response.get(COMPACTION_KEY)?.clone()).ok()
}

/// What a request asks of compaction (`context_management`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompactionAsk {
    /// Compact above this many tokens (`compact_threshold`); `None` = the settings' percent.
    pub threshold: Option<u64>,
}

/// Reads `context_management` (OpenAI: `[{"type": "compaction", "compact_threshold": N}]`).
/// `Ok(None)` = absent or empty; `Err(message)` = not understood.
pub fn parse_context_management(v: &Value) -> Result<Option<CompactionAsk>, String> {
    let items = match v {
        Value::Null => return Ok(None),
        Value::Array(a) => a.as_slice(),
        _ => return Err("'context_management' must be an array.".into()),
    };
    let mut ask = None;
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("compaction") => {
                let threshold = match item.get("compact_threshold") {
                    None | Some(Value::Null) => None,
                    Some(t) => Some(t.as_u64().filter(|n| *n >= 256).ok_or_else(|| {
                        "'compact_threshold' must be an integer of at least 256 tokens.".to_string()
                    })?),
                };
                ask = Some(CompactionAsk { threshold });
            }
            other => {
                return Err(format!(
                    "Unsupported context_management type '{}': only 'compaction' is supported.",
                    other.unwrap_or("")
                ))
            }
        }
    }
    Ok(ask)
}

/// Longest tool result kept in a summarizer transcript (like the Playground's compaction).
const TOOL_RESULT_CHARS: usize = 300;

fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                p.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| p.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// One conversation item as a line of a summarizer transcript (`None` = nothing to say:
/// reasoning, empty messages).
pub fn item_entry(item: &Value) -> Option<String> {
    let kind = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    match kind {
        "message" => {
            let text = content_text(item.get("content").unwrap_or(&Value::Null));
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            let who = match item.get("role").and_then(Value::as_str).unwrap_or("user") {
                "assistant" => "Assistant",
                "system" | "developer" => "System",
                _ => "User",
            };
            Some(format!("{who}: {text}"))
        }
        "function_call" => Some(format!(
            "Assistant called tool `{}` with {}",
            item.get("name").and_then(Value::as_str).unwrap_or("?"),
            item.get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
        )),
        "function_call_output" => {
            let out = match item.get("output") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => content_text(other),
                None => String::new(),
            };
            let out = out.trim();
            let shown = if out.chars().count() > TOOL_RESULT_CHARS {
                let head: String = out.chars().take(TOOL_RESULT_CHARS).collect();
                format!(
                    "{head} … [rest of the tool result cleared, {} characters]",
                    out.chars().count() - TOOL_RESULT_CHARS
                )
            } else {
                out.to_string()
            };
            Some(format!("Tool result: {shown}"))
        }
        _ => None,
    }
}

/// The transcript entries of links (input items, then output items, per link).
pub fn link_entries(chain: &[Link<'_>]) -> Vec<String> {
    chain_context(chain).iter().filter_map(item_entry).collect()
}

/// `instructions` with the summary block appended: the summary reaches the model in the one
/// system message (some chat templates reject a system message after the first).
pub fn instructions_with_summary(instructions: &Value, summary: &str) -> Value {
    let block = crate::compaction::summary_block(summary);
    match instructions.as_str().filter(|s| !s.trim().is_empty()) {
        Some(own) => Value::String(format!("{}\n\n{}", own.trim_end(), block)),
        None => Value::String(block),
    }
}

/// Query of `GET /v1/responses/{id}/input_items` (OpenAI: `after`, `before`, `limit` 1–100,
/// default 20, `order` `asc`|`desc`, default `desc`, and optional `include`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ItemsQuery {
    pub after: Option<String>,
    pub before: Option<String>,
    pub limit: Option<u32>,
    pub order: Option<String>,
    #[serde(alias = "include[]")]
    pub include: Option<String>,
}

/// One page of input items as OpenAI's list object:
/// `{"object":"list","data":[...],"first_id","last_id","has_more"}`. `Err` = bad `order`.
pub fn items_page(items: &[Value], q: &ItemsQuery) -> Result<Value, String> {
    let desc = match q.order.as_deref() {
        None | Some("desc") => true,
        Some("asc") => false,
        Some(other) => return Err(format!("order must be 'asc' or 'desc', got '{other}'")),
    };
    let limit = q.limit.unwrap_or(20).clamp(1, 100) as usize;
    let mut ordered: Vec<&Value> = items.iter().collect();
    if desc {
        ordered.reverse();
    }
    let id_of = |v: &Value| v.get("id").and_then(Value::as_str).map(str::to_string);
    let mut start = 0;
    if let Some(after) = q.after.as_deref() {
        if let Some(pos) = ordered
            .iter()
            .position(|v| id_of(v).as_deref() == Some(after))
        {
            start = pos + 1;
        }
    }
    let mut end = ordered.len();
    if let Some(before) = q.before.as_deref() {
        if let Some(pos) = ordered
            .iter()
            .position(|v| id_of(v).as_deref() == Some(before))
        {
            end = pos;
        }
    }
    let window: Vec<&Value> = if start < end {
        ordered[start..end].to_vec()
    } else {
        Vec::new()
    };
    // With only `before`, the page is the items right before it.
    let (data, has_more) = if q.before.is_some() && q.after.is_none() {
        let skip = window.len().saturating_sub(limit);
        (window[skip..].to_vec(), skip > 0)
    } else {
        let has_more = window.len() > limit;
        (window.into_iter().take(limit).collect(), has_more)
    };
    let first_id = data.first().and_then(|v| id_of(v));
    let last_id = data.last().and_then(|v| id_of(v));
    Ok(json!({
        "object": "list",
        "data": data,
        "first_id": first_id,
        "last_id": last_id,
        "has_more": has_more,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_never_mix() {
        assert_eq!(scope_of(Some("abc"), Some("ci")), "token:abc");
        assert_eq!(scope_of(None, Some("harness")), "harness");
        assert_eq!(scope_of(None, None), "anonymous");
    }

    #[test]
    fn string_input_becomes_one_user_message() {
        let items = normalize_input(Some(&json!("hello")));
        assert_eq!(
            items,
            vec![
                json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]})
            ]
        );
        assert!(normalize_input(None).is_empty());
        assert!(normalize_input(Some(&json!(42))).is_empty());
    }

    #[test]
    fn chain_context_orders_links_and_skips_instructions() {
        let in1 = normalize_input(Some(&json!("My name is Ada.")));
        let r1 = json!({"id":"resp_1","instructions":"Be terse.","output":[
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hi Ada"}]}
        ]});
        let in2 = vec![json!({"role":"user","content":"Weather in Paris?"})];
        let r2 = json!({"id":"resp_2","output":[
            {"type":"function_call","call_id":"call_1","name":"get_weather","arguments":"{\"city\":\"Paris\"}"}
        ]});
        let in3 = vec![json!({"type":"function_call_output","call_id":"call_1","output":"Sunny"})];
        let r3 = json!({"id":"resp_3","output":[]});
        let ctx = chain_context(&[
            Link {
                input: &in1,
                response: &r1,
            },
            Link {
                input: &in2,
                response: &r2,
            },
            Link {
                input: &in3,
                response: &r3,
            },
        ]);
        let types: Vec<&str> = ctx
            .iter()
            .map(|v| v.get("type").and_then(Value::as_str).unwrap_or("message"))
            .collect();
        assert_eq!(
            types,
            [
                "message",
                "message",
                "message",
                "function_call",
                "function_call_output"
            ]
        );
        assert_eq!(ctx[1]["content"][0]["text"], "Hi Ada");
        assert_eq!(ctx[3]["call_id"], "call_1");
        assert_eq!(ctx[4]["output"], "Sunny");
        assert!(!serde_json::to_string(&ctx).unwrap().contains("Be terse"));
    }

    #[test]
    fn item_ids_are_stable_and_kept() {
        let mut items = vec![
            json!({"role":"user","content":"a"}),
            json!({"type":"function_call_output","call_id":"c","output":"x"}),
            json!({"type":"message","role":"assistant","id":"msg_keep","content":[]}),
        ];
        assign_item_ids(&mut items, "resp_AbC123");
        assert_eq!(items[0]["id"], "msg_AbC123_0");
        assert_eq!(items[1]["id"], "item_AbC123_1");
        assert_eq!(items[2]["id"], "msg_keep");
    }

    #[test]
    fn items_pages_follow_openai_cursors() {
        let items: Vec<Value> = (0..5).map(|i| json!({"id": format!("i{i}")})).collect();
        let ids = |v: &Value| -> Vec<String> {
            v["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x["id"].as_str().unwrap().to_string())
                .collect()
        };
        // Default: newest first, 20 per page.
        let p = items_page(&items, &ItemsQuery::default()).unwrap();
        assert_eq!(ids(&p), ["i4", "i3", "i2", "i1", "i0"]);
        assert_eq!(p["has_more"], false);
        assert_eq!(p["first_id"], "i4");
        assert_eq!(p["last_id"], "i0");
        // Ascending, two per page, then continue after the last id.
        let q = ItemsQuery {
            order: Some("asc".into()),
            limit: Some(2),
            ..Default::default()
        };
        let p = items_page(&items, &q).unwrap();
        assert_eq!(ids(&p), ["i0", "i1"]);
        assert_eq!(p["has_more"], true);
        let p = items_page(
            &items,
            &ItemsQuery {
                after: Some("i1".into()),
                ..q.clone()
            },
        )
        .unwrap();
        assert_eq!(ids(&p), ["i2", "i3"]);
        let p = items_page(
            &items,
            &ItemsQuery {
                after: Some("i3".into()),
                ..q.clone()
            },
        )
        .unwrap();
        assert_eq!(ids(&p), ["i4"]);
        assert_eq!(p["has_more"], false);
        // `before`: the items right before the cursor.
        let p = items_page(
            &items,
            &ItemsQuery {
                before: Some("i4".into()),
                ..q.clone()
            },
        )
        .unwrap();
        assert_eq!(ids(&p), ["i2", "i3"]);
        assert_eq!(p["has_more"], true);
        assert!(items_page(
            &items,
            &ItemsQuery {
                order: Some("up".into()),
                ..Default::default()
            }
        )
        .is_err());
        // Empty list.
        let p = items_page(&[], &ItemsQuery::default()).unwrap();
        assert_eq!(p["data"], json!([]));
        assert_eq!(p["first_id"], Value::Null);
    }

    #[test]
    fn context_management_is_parsed_strictly() {
        let ask = |v: Value| parse_context_management(&v);
        assert_eq!(ask(Value::Null), Ok(None));
        assert_eq!(ask(json!([])), Ok(None));
        assert_eq!(
            ask(json!([{"type": "compaction"}])),
            Ok(Some(CompactionAsk { threshold: None }))
        );
        assert_eq!(
            ask(json!([{"type": "compaction", "compact_threshold": 12000}])),
            Ok(Some(CompactionAsk {
                threshold: Some(12000)
            }))
        );
        assert!(ask(json!([{"type": "compaction", "compact_threshold": 10}])).is_err());
        assert!(ask(json!([{"type": "compaction", "compact_threshold": "big"}])).is_err());
        assert!(ask(json!([{"type": "other"}])).is_err());
        assert!(ask(json!({"type": "compaction"})).is_err());
    }

    #[test]
    fn transcript_entries_cover_messages_and_tools() {
        let items = [
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"Weather in Paris?"}]}),
            json!({"type":"reasoning","summary":[]}),
            json!({"type":"function_call","call_id":"c1","name":"get_weather","arguments":"{\"city\":\"Paris\"}"}),
            json!({"type":"function_call_output","call_id":"c1","output":"x".repeat(500)}),
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"Sunny."}]}),
            json!({"type":"message","role":"assistant","content":[]}),
        ];
        let resp = json!({"output": items[2..].to_vec()});
        let link = Link {
            input: &items[..2],
            response: &resp,
        };
        let entries = link_entries(&[link]);
        assert_eq!(entries.len(), 4, "{entries:?}");
        assert_eq!(entries[0], "User: Weather in Paris?");
        assert!(entries[1].starts_with("Assistant called tool `get_weather` with {"));
        assert!(entries[2].starts_with("Tool result: xxx"));
        assert!(entries[2].contains("rest of the tool result cleared, 200 characters"));
        assert_eq!(entries[3], "Assistant: Sunny.");
    }

    #[test]
    fn the_summary_joins_the_instructions() {
        let with = instructions_with_summary(&json!("Be terse."), "Ada likes tea.");
        let text = with.as_str().unwrap();
        assert!(text.starts_with("Be terse.\n\n<conversation_summary>"));
        assert!(text.contains("Ada likes tea."));
        let bare = instructions_with_summary(&Value::Null, "S");
        assert!(bare.as_str().unwrap().starts_with("<conversation_summary>"));
        let marker = json!({COMPACTION_KEY: {"through": "resp_2", "summary": "S", "tokens_before": 900,
            "tokens_after": 300, "created_at": 1}});
        let s = summary_of(&marker).unwrap();
        assert_eq!((s.through.as_str(), s.tokens_before), ("resp_2", 900));
        assert!(summary_of(&json!({"id": "resp_1"})).is_none());
    }

    #[test]
    fn settings_are_bounded() {
        let s = ResponsesSettings {
            store: true,
            retention_days: 0,
            max_stored: 0,
            max_mib: 999_999,
            compaction: CompactionSettings {
                auto: true,
                threshold_percent: 5,
                keep_links: 0,
            },
        }
        .validated();
        assert_eq!((s.retention_days, s.max_stored, s.max_mib), (1, 1, 10_240));
        assert_eq!(
            (s.compaction.threshold_percent, s.compaction.keep_links),
            (10, 1)
        );
        assert_eq!(CompactionSettings::default().threshold_percent, 80);
        let r = ResponsesSettings::default().retention();
        assert_eq!(r.max_age_ms, 30 * 86_400_000);
        assert_eq!(r.max_rows, 1000);
        assert_eq!(r.max_bytes, 100 * 1024 * 1024);
    }
}
