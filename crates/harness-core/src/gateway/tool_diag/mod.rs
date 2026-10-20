//! Tool-call diagnosis: which of the parties in a tool round is at fault.
//!
//! An agent client (OpenCode, Claude Code, Codex...), llama-server with the model's chat
//! template, and the model itself all sit in one tool round, and a failed round looks the same
//! from outside: "the tool did not work". The gateway sees every request and response, so it can
//! tell them apart and say it next to the request in the inspector.
//!
//! * [`request::ToolRequest`] reads the tools, `tool_choice`, earlier calls and the tool results
//!   of a request (Chat Completions, Anthropic Messages and Responses shapes).
//! * [`response::ToolObserver`] collects the answer text and the calls from the response bytes
//!   as they stream through (JSON or SSE), bounded.
//! * [`classify`](classify::classify) is the pure verdict over both, with the finish reason,
//!   token counts, the slot's context size and what the chat template says about tools.
//! * [`ToolRound`] ties the three to a gateway request: it is started with the request body,
//!   fed the response and written into the [`RequestRecord`] when the request ends.
//! * [`level`] turns the measured rounds of a model into its tool level and the number of tools
//!   worth enabling.
//! * [`level`] turns the measured rounds of a model into its tool level and the number of tools
//!   worth enabling.
//! * [`CallIndex`] links a tool result in a later request to the request that made the call.
//!
//! Design notes and the rules in words: `private hs-internal tool-diagnosis notes`.

pub mod classify;
pub mod level;
pub mod request;
pub mod response;
pub mod schema;

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

pub use classify::{classify, Category, Code, Issue, Round, ToolDiag};
pub use level::{recommended_tools, ToolLevel, ToolRounds};
pub use request::ToolRequest;
pub use response::{ToolObserver, ToolResponse};

use super::record::RequestRecord;

/// Most tool calls the index remembers (oldest dropped first).
const INDEX_CAPACITY: usize = 4096;

/// Remembers which gateway request made which tool call, so that a tool result that comes back
/// in a *later* request can be linked to the request it answers.
///
/// The heuristic is the call id: llama-server writes an id into every call and a client echoes
/// it as `tool_call_id` / `tool_use_id` / `call_id` with the result. That is exact (no guessing
/// by client or conversation) and works across requests of one conversation. A client that
/// invents its own ids is simply not linked; the diagnosis still says the tool failed.
#[derive(Debug, Default)]
pub struct CallIndex {
    inner: Mutex<IndexInner>,
}

#[derive(Debug, Default)]
struct IndexInner {
    by_call: HashMap<String, String>,
    order: VecDeque<String>,
}

impl CallIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide index the gateway uses.
    pub fn global() -> &'static CallIndex {
        static INDEX: OnceLock<CallIndex> = OnceLock::new();
        INDEX.get_or_init(CallIndex::new)
    }

    pub fn record(&self, call_id: &str, request_id: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.by_call
            .insert(call_id.to_string(), request_id.to_string())
            .is_none()
        {
            g.order.push_back(call_id.to_string());
        }
        while g.order.len() > INDEX_CAPACITY {
            if let Some(old) = g.order.pop_front() {
                g.by_call.remove(&old);
            }
        }
    }

    /// The request that made the call.
    pub fn lookup(&self, call_id: &str) -> Option<String> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.by_call.get(call_id).cloned()
    }
}

/// One tool round of a gateway request, from the request body to the record.
#[derive(Debug)]
pub struct ToolRound {
    req: ToolRequest,
    obs: Option<ToolObserver>,
    template_tools: Option<bool>,
}

impl ToolRound {
    /// Reads the request body (the one sent upstream). `None` when the round involves no tools
    /// on the request side: neither tools nor tool results (the response cannot make a call
    /// then).
    pub fn start(body: &[u8], template_tools: Option<bool>) -> Option<Self> {
        let req = ToolRequest::parse(body)?;
        if req.tools.is_empty() && !req.has_tool_messages {
            return None;
        }
        Some(Self {
            req,
            obs: None,
            template_tools,
        })
    }

    /// The upstream answered: from now on response bytes are fed (`sse`: an event stream).
    pub fn begin_response(&mut self, sse: bool) {
        self.obs = Some(ToolObserver::new(sse));
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        if let Some(o) = self.obs.as_mut() {
            o.feed(chunk);
        }
    }

    /// Writes the verdict into the record (call when the request ended: status, usage and the
    /// slot's context size are read from the record) and files the calls in `index`.
    pub fn finalize(self, record: &mut RequestRecord, index: &CallIndex) {
        let resp = self.obs.map(ToolObserver::finish).unwrap_or_default();
        let round = Round {
            status: record.status,
            finish_reason: record.usage.finish_reason.as_deref(),
            prompt_tokens: record.usage.prompt_tokens,
            completion_tokens: record.usage.completion_tokens,
            ctx_size: record.ctx_size,
            template_tools: self.template_tools,
        };
        let mut diag = classify(&self.req, &resp, &round);
        for call in &resp.calls {
            if let Some(id) = &call.id {
                index.record(id, &record.id);
            }
        }
        if let Some(issue) = diag.issue.as_mut() {
            link_client_error(
                &mut issue.detail,
                index,
                issue.code == Code::ToolResultError,
            );
        }
        record.tool_involved = diag.involved;
        record.tools_offered = diag.tools_offered;
        record.tool_calls = diag.calls;
        if let Some(issue) = &diag.issue {
            record.tool_diag_category = Some(issue.category().as_str().to_string());
            record.tool_diag_code = Some(issue.code.as_str().to_string());
            record.tool_diag_detail = Some(issue.detail.to_string());
        }
    }
}

/// Adds `linked_request` to the client-error evidence (the detail itself for a client issue,
/// its `client` member when a model issue outranked it).
fn link_client_error(detail: &mut Value, index: &CallIndex, own: bool) {
    let target = if own {
        Some(detail)
    } else {
        detail.get_mut("client")
    };
    let Some(obj) = target.and_then(Value::as_object_mut) else {
        return;
    };
    let linked = obj
        .get("call_id")
        .and_then(Value::as_str)
        .and_then(|id| index.lookup(id));
    if let Some(r) = linked {
        obj.insert("linked_request".into(), Value::String(r));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(req: &[u8], resp: &[u8], sse: bool, mut record: RequestRecord) -> RequestRecord {
        let mut round = ToolRound::start(req, None).expect("tools involved");
        round.begin_response(sse);
        for piece in resp.chunks(211) {
            round.feed(piece);
        }
        round.finalize(&mut record, &CallIndex::new());
        record
    }

    #[test]
    fn a_request_without_tools_starts_no_round() {
        assert!(
            ToolRound::start(br#"{"messages":[{"role":"user","content":"hi"}]}"#, None).is_none()
        );
        assert!(ToolRound::start(b"not json", None).is_none());
    }

    #[test]
    fn the_record_gets_counts_and_the_verdict() {
        let req =
            include_bytes!("../../../tests/data/tooldiag/b11342-qwen2.5-coder-14b.chat.req.json");
        let resp =
            include_bytes!("../../../tests/data/tooldiag/b11342-qwen2.5-coder-14b.chat.resp.json");
        let mut r = RequestRecord {
            id: "req_1".into(),
            status: 200,
            ..Default::default()
        };
        r.usage.finish_reason = Some("stop".into());
        let r = run(req, resp, false, r);
        assert!(r.tool_involved);
        assert_eq!((r.tools_offered, r.tool_calls), (1, 0));
        assert_eq!(r.tool_diag_category.as_deref(), Some("runtime"));
        assert_eq!(r.tool_diag_code.as_deref(), Some("call_left_in_content"));
        let detail: Value = serde_json::from_str(r.tool_diag_detail.as_deref().unwrap()).unwrap();
        assert_eq!(detail["marker"], "<tools>");
    }

    #[test]
    fn a_tool_result_error_is_linked_to_the_request_that_made_the_call() {
        let index = CallIndex::new();
        // Round 1: the real answer of Qwen3 makes a call; the index files its id.
        let req1 = include_bytes!("../../../tests/data/tooldiag/b11342-qwen3-4b.chat.req.json");
        let resp1 = include_bytes!("../../../tests/data/tooldiag/b11342-qwen3-4b.chat.resp.json");
        let mut first = RequestRecord {
            id: "req_first".into(),
            status: 200,
            ..Default::default()
        };
        first.usage.finish_reason = Some("tool_calls".into());
        let mut round = ToolRound::start(req1, None).unwrap();
        round.begin_response(false);
        round.feed(resp1);
        round.finalize(&mut first, &index);
        assert_eq!(first.tool_calls, 1);
        assert_eq!(first.tool_diag_code, None);
        // Round 2: the client sends the (error) result with the echoed call id.
        let req2 = include_bytes!("../../../tests/data/tooldiag/b11342-qwen3-4b.chat2err.req.json");
        let resp2 =
            include_bytes!("../../../tests/data/tooldiag/b11342-qwen3-4b.chat2err.resp.json");
        let mut second = RequestRecord {
            id: "req_second".into(),
            status: 200,
            ..Default::default()
        };
        second.usage.finish_reason = Some("stop".into());
        let mut round = ToolRound::start(req2, None).unwrap();
        round.begin_response(false);
        round.feed(resp2);
        round.finalize(&mut second, &index);
        assert_eq!(second.tool_diag_category.as_deref(), Some("client"));
        let detail: Value =
            serde_json::from_str(second.tool_diag_detail.as_deref().unwrap()).unwrap();
        assert_eq!(detail["linked_request"], "req_first");
    }

    #[test]
    fn the_index_forgets_the_oldest() {
        let index = CallIndex::new();
        for i in 0..(INDEX_CAPACITY + 10) {
            index.record(&format!("c{i}"), &format!("r{i}"));
        }
        assert_eq!(index.lookup("c0"), None);
        assert_eq!(index.lookup("c9"), None);
        assert_eq!(index.lookup("c10").as_deref(), Some("r10"));
        assert_eq!(
            index.lookup(&format!("c{}", INDEX_CAPACITY + 9)).as_deref(),
            Some(format!("r{}", INDEX_CAPACITY + 9).as_str())
        );
    }
}
