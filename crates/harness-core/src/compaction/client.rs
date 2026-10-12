//! Running a compaction: token counting, summarization requests and the marker write.

use super::{
    active_compaction, chunk_budget, chunks, is_chat, message_marker, message_text, plan,
    plan_mid_turn, shorten, strip_think, summary_block, summary_budget, summary_messages,
    transcript_entry, Marker, PlanOptions, Strategy, TriggerKind, AUTO_MIN_PERCENT, MARKER_VERSION,
    MESSAGE_OVERHEAD, META_KEY,
};
use crate::error_key::err;
use crate::state::{AppendMessage, ConversationView, StateDb, StateError, VariantInput};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The OpenAI-compatible endpoint the summary is requested from (the Playground's chat
/// target: the gateway's `.../v1` with its client token, or a server's `/v1`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Endpoint {
    pub base_url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Model alias (`model` field); required by the gateway.
    #[serde(default)]
    pub model: Option<String>,
}

/// A llama-server root (`http://host:port`) whose `/tokenize` counts tokens exactly.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TokenizeEndpoint {
    pub host_url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// Added to the summary instruction when the answer is still running: the user's request
/// follows verbatim, the summary has to carry the work done so far.
const MID_TURN_INSTRUCTION: &str = "The assistant is in the middle of answering the user's latest request (quoted first in the transcript; it is kept verbatim next to your summary, so do not restate it). Under 'Answers and decisions' list EVERY tool call already made (tool name and arguments) with the key result, so that no call is repeated; under 'Open items' list only what the request still needs and has not been done yet. The assistant continues the work from your summary.";

fn default_keep() -> usize {
    2
}

fn yes() -> bool {
    true
}

/// One compaction of a conversation's current path.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompactRequest {
    pub conversation_id: String,
    /// The last message the client shows; the request fails when the stored path moved on.
    #[serde(default)]
    pub leaf_id: Option<String>,
    pub endpoint: Endpoint,
    #[serde(default)]
    pub tokenize: Option<TokenizeEndpoint>,
    /// Context of the chat model (per slot), in tokens.
    pub n_ctx: u64,
    /// Context of the summary model when it is another model (default: `n_ctx`).
    #[serde(default)]
    pub summary_ctx: Option<u64>,
    #[serde(default = "default_keep")]
    pub keep_turns: usize,
    #[serde(default = "yes")]
    pub clear_tool_results: bool,
    /// Summary output budget in tokens (default: 1/8 of the context, 256–2048).
    #[serde(default)]
    pub summary_tokens: Option<u64>,
    /// Extra instruction for the summary ("focus on the numbers").
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub strategy: Strategy,
    #[serde(default)]
    pub trigger: TriggerKind,
    /// Between two tool rounds of a running answer: the path ends with the running turn, whose
    /// user message stays verbatim ([`plan_mid_turn`]).
    #[serde(default)]
    pub mid_turn: bool,
    /// Names of the conversation's documents (kept and re-retrieved, not summarized).
    #[serde(default)]
    pub documents: Vec<String>,
    /// The conversation's system prompt (counted in the before/after numbers).
    #[serde(default)]
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactOutcome {
    pub view: ConversationView,
    pub marker_id: String,
    pub marker: Marker,
    /// Summarization requests made (chunks).
    pub requests: u32,
}

// ---- jobs ----

/// Running compactions by conversation id (cancel flags).
#[derive(Default)]
pub struct Jobs {
    map: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

pub fn jobs() -> &'static Jobs {
    static JOBS: std::sync::OnceLock<Jobs> = std::sync::OnceLock::new();
    JOBS.get_or_init(Jobs::default)
}

impl Jobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers a compaction; `None` while another one runs for the conversation.
    pub fn register(&self, conversation_id: &str) -> Option<Arc<AtomicBool>> {
        let mut map = self.lock();
        if map.contains_key(conversation_id) {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        map.insert(conversation_id.to_string(), flag.clone());
        Some(flag)
    }

    /// Asks a running compaction to stop; false when none runs.
    pub fn cancel(&self, conversation_id: &str) -> bool {
        match self.lock().get(conversation_id) {
            Some(f) => {
                f.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn finish(&self, conversation_id: &str, flag: &Arc<AtomicBool>) {
        let mut map = self.lock();
        if map
            .get(conversation_id)
            .is_some_and(|f| Arc::ptr_eq(f, flag))
        {
            map.remove(conversation_id);
        }
    }
}

// ---- HTTP ----

/// Prompt processing of a long transcript plus a JIT model load can take minutes.
const TIMEOUT: Duration = Duration::from_secs(600);

fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(TIMEOUT)
                .no_proxy()
                .build()
                .unwrap_or_default()
        })
        .clone()
}

async fn wait_cancel(flag: &AtomicBool) {
    while !flag.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn cancelled() -> String {
    err("compaction.cancelled").into()
}

async fn post_json(
    url: &str,
    headers: &HashMap<String, String>,
    body: &Value,
    cancel: &AtomicBool,
) -> Result<Value, String> {
    let mut req = http().post(url).json(body);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let send = async {
        let res = req
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, url))?;
        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|e| crate::http_error::transport(&e, url))?;
        if !status.is_success() {
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v["error"]["message"]
                        .as_str()
                        .or(v["error"].as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| text.chars().take(300).collect());
            return Err(String::from(
                err("compaction.request_failed")
                    .with("url", url)
                    .with("error", format!("HTTP {}: {}", status.as_u16(), detail)),
            ));
        }
        serde_json::from_str::<Value>(&text).map_err(|e| {
            String::from(
                err("compaction.request_failed")
                    .with("url", url)
                    .with("error", e),
            )
        })
    };
    tokio::select! {
        r = send => r,
        _ = wait_cancel(cancel) => Err(cancelled()),
    }
}

/// A finished summarization request.
#[derive(Debug, Clone)]
pub(crate) struct ChatResult {
    pub content: String,
    pub completion_tokens: Option<u64>,
}

pub(crate) async fn chat(
    ep: &Endpoint,
    messages: Vec<Value>,
    max_tokens: u64,
    cancel: &AtomicBool,
) -> Result<ChatResult, String> {
    let mut body = json!({
        "messages": messages,
        "stream": false,
        "max_tokens": max_tokens,
        "temperature": 0.2,
        // Reasoning models would spend the budget on thinking.
        "chat_template_kwargs": { "enable_thinking": false },
    });
    if let Some(m) = ep.model.as_deref().filter(|m| !m.is_empty()) {
        body["model"] = json!(m);
    }
    let url = format!("{}/chat/completions", ep.base_url.trim_end_matches('/'));
    let v = post_json(&url, &ep.headers, &body, cancel).await?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    Ok(ChatResult {
        content: strip_think(content),
        completion_tokens: v["usage"]["completion_tokens"].as_u64(),
    })
}

// ---- token counting ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CountMode {
    Tokenize,
    CountTokens,
    Estimate,
}

/// Counts tokens of message texts: llama-server `/tokenize`, else the gateway's
/// `/v1/messages/count_tokens`, else an estimate. The first failure of a method switches to
/// the next one for the rest of the run.
pub(crate) struct Counter<'a> {
    tokenize: Option<&'a TokenizeEndpoint>,
    ep: &'a Endpoint,
    mode: CountMode,
    cache: HashMap<String, u64>,
}

impl<'a> Counter<'a> {
    pub fn new(tokenize: Option<&'a TokenizeEndpoint>, ep: &'a Endpoint) -> Self {
        Counter {
            mode: if tokenize.is_some() {
                CountMode::Tokenize
            } else if ep.base_url.is_empty() {
                CountMode::Estimate
            } else {
                CountMode::CountTokens
            },
            tokenize,
            ep,
            cache: HashMap::new(),
        }
    }

    /// The counts so far came from a tokenizer.
    pub fn exact(&self) -> bool {
        self.mode != CountMode::Estimate
    }

    /// Tokens of `text` as one chat message (template overhead included).
    pub async fn count(&mut self, text: &str, cancel: &AtomicBool) -> Result<u64, String> {
        if text.is_empty() {
            return Ok(MESSAGE_OVERHEAD);
        }
        if let Some(&n) = self.cache.get(text) {
            return Ok(n);
        }
        let n = loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(cancelled());
            }
            match self.mode {
                CountMode::Tokenize => {
                    let t = self.tokenize.expect("tokenize mode has an endpoint");
                    let url = format!("{}/tokenize", t.host_url.trim_end_matches('/'));
                    match post_json(&url, &t.headers, &json!({ "content": text }), cancel).await {
                        Ok(v) if v["tokens"].is_array() => {
                            break v["tokens"].as_array().map_or(0, Vec::len) as u64
                                + MESSAGE_OVERHEAD
                        }
                        Err(e) if cancel.load(Ordering::SeqCst) => return Err(e),
                        _ => {
                            self.mode = if self.ep.base_url.is_empty() {
                                CountMode::Estimate
                            } else {
                                CountMode::CountTokens
                            }
                        }
                    }
                }
                CountMode::CountTokens => {
                    let url = format!(
                        "{}/messages/count_tokens",
                        self.ep.base_url.trim_end_matches('/')
                    );
                    let body = json!({
                        "model": self.ep.model.clone().unwrap_or_default(),
                        "messages": [{ "role": "user", "content": text }],
                    });
                    match post_json(&url, &self.ep.headers, &body, cancel).await {
                        Ok(v) if v["input_tokens"].is_u64() => {
                            break v["input_tokens"].as_u64().unwrap_or(0)
                        }
                        Err(e) if cancel.load(Ordering::SeqCst) => return Err(e),
                        _ => self.mode = CountMode::Estimate,
                    }
                }
                CountMode::Estimate => break super::estimate_tokens(text) + MESSAGE_OVERHEAD,
            }
        };
        self.cache.insert(text.to_string(), n);
        Ok(n)
    }
}

/// How a summary is written (shared by conversation compaction and Responses chains).
pub(crate) struct SummarizeParams<'a> {
    pub strategy: Strategy,
    pub instruction: Option<&'a str>,
    pub documents: &'a [String],
    /// Context of the model that writes the summary.
    pub sum_ctx: u64,
    /// Output budget of the summary.
    pub summary_max: u64,
}

/// Summarizes transcript `entries` (text and tokens each), merging `previous` (an earlier
/// summary); chunks the transcript when it does not fit one request. Returns the summary, the
/// completion tokens of the last request when the server reported them, and the number of
/// requests made.
pub(crate) async fn summarize_entries(
    counter: &mut Counter<'_>,
    ep: &Endpoint,
    p: &SummarizeParams<'_>,
    entries: &[(String, u64)],
    previous: Option<String>,
    cancel: &AtomicBool,
) -> Result<(String, Option<u64>, u32), String> {
    let mut summary = previous;
    let previous_tokens = match &summary {
        Some(s) => counter.count(s, cancel).await?,
        None => 0,
    };
    let fixed: String = summary_messages(
        p.strategy,
        "",
        None,
        p.instruction,
        p.documents,
        p.summary_max,
    )
    .iter()
    .filter_map(|m| m["content"].as_str())
    .collect::<Vec<_>>()
    .join(
        "
",
    );
    let overhead = counter.count(&fixed, cancel).await? + 3 * MESSAGE_OVERHEAD;

    let entry_tokens: Vec<u64> = entries.iter().map(|e| e.1).collect();
    let total: u64 = entry_tokens.iter().sum();
    let single = chunk_budget(
        p.sum_ctx,
        p.summary_max,
        overhead,
        previous_tokens,
        p.strategy,
    )
    .map_err(String::from)?;
    let (ranges, budget) = if total <= single && p.strategy != Strategy::Incremental {
        (
            std::iter::once(0..entries.len()).collect::<Vec<_>>(),
            single,
        )
    } else {
        let b = chunk_budget(
            p.sum_ctx,
            p.summary_max,
            overhead,
            previous_tokens.max(p.summary_max),
            p.strategy,
        )
        .map_err(String::from)?;
        (chunks(&entry_tokens, b), b)
    };

    let mut summary_tokens = None;
    let mut requests = 0u32;
    for r in ranges {
        let transcript = entries[r]
            .iter()
            .map(|(text, t)| shorten(text, *t, budget))
            .collect::<Vec<_>>()
            .join(
                "

",
            );
        let messages = summary_messages(
            p.strategy,
            &transcript,
            summary.as_deref(),
            p.instruction,
            p.documents,
            p.summary_max,
        );
        let res = chat(ep, messages, p.summary_max, cancel).await?;
        requests += 1;
        if res.content.trim().is_empty() {
            return Err(err("compaction.empty_summary").into());
        }
        summary_tokens = res.completion_tokens;
        summary = Some(res.content);
    }
    let summary = summary.ok_or_else(|| String::from(err("compaction.empty_summary")))?;
    Ok((summary, summary_tokens, requests))
}

// ---- compaction ----

/// Summarizes the older turns of the conversation's current path and appends the marker.
pub async fn compact(
    db: &StateDb,
    req: &CompactRequest,
    cancel: &AtomicBool,
) -> Result<CompactOutcome, String> {
    let view = db.get_conversation(&req.conversation_id)?;
    let path = &view.messages;
    let last = path
        .last()
        .ok_or_else(|| String::from(err("compaction.nothing_to_compact")))?;
    if let Some(leaf) = req.leaf_id.as_deref() {
        if last.id != leaf {
            return Err(err("compaction.stale_path").into());
        }
    }
    let mut counter = Counter::new(req.tokenize.as_ref(), &req.endpoint);

    // Token counts of the messages the next request would send.
    let active = active_compaction(path);
    let start = active.as_ref().map_or(0, |a| a.boundary_index + 1);
    let mut tokens = vec![0u64; path.len()];
    for i in start..path.len() {
        if is_chat(&path[i]) {
            tokens[i] = counter.count(&message_text(&path[i]), cancel).await?;
        }
    }
    // The previous marker's pinned question is part of the request too (before the boundary).
    let previous_pinned_tokens = match active.as_ref().and_then(|a| a.pinned_index) {
        Some(p) => {
            tokens[p] = counter.count(&message_text(&path[p]), cancel).await?;
            tokens[p]
        }
        None => 0,
    };
    let auto = req.trigger == TriggerKind::Auto;
    let opts = PlanOptions {
        n_ctx: req.n_ctx,
        keep_turns: req.keep_turns,
        min_tokens: if auto {
            req.n_ctx * AUTO_MIN_PERCENT / 100
        } else {
            0
        },
    };
    let plan = if req.mid_turn {
        plan_mid_turn(path, &tokens, &opts)
    } else {
        plan(path, &tokens, &opts)
    }
    .map_err(String::from)?;

    let sum_ctx = req.summary_ctx.filter(|&c| c > 0).unwrap_or(req.n_ctx);
    let summary_max = summary_budget(sum_ctx, req.summary_tokens);
    let instruction = if req.mid_turn {
        Some(
            match req.instruction.as_deref().filter(|i| !i.trim().is_empty()) {
                Some(i) => format!("{} {}", MID_TURN_INSTRUCTION, i.trim()),
                None => MID_TURN_INSTRUCTION.to_string(),
            },
        )
    } else {
        req.instruction
            .as_deref()
            .filter(|i| !i.trim().is_empty())
            .map(str::to_string)
    };
    let instruction = instruction.as_deref();

    // Transcript entries of the summarized messages (tool results shortened).
    let mut entries: Vec<(String, u64)> = Vec::with_capacity(plan.summarize.len());
    let mut cleared = 0u32;
    // Between tool rounds the question being answered is context for the summarizer (it stays
    // verbatim in the request, so it is not part of what the summary replaces).
    if let Some(p) = plan.pinned {
        let text = format!(
            "User (the request being answered now): {}",
            path[p].variant.content.trim()
        );
        entries.push((text, tokens[p] + 2));
    }
    for &i in &plan.summarize {
        let (text, c) = transcript_entry(&path[i], req.clear_tool_results);
        cleared += c;
        let t = if c > 0 {
            counter.count(&text, cancel).await?
        } else {
            tokens[i] + 2
        };
        entries.push((text, t));
    }

    let previous = plan.previous.as_ref().map(|a| a.summary.clone());
    let (summary, summary_tokens, requests) = summarize_entries(
        &mut counter,
        &req.endpoint,
        &SummarizeParams {
            strategy: req.strategy,
            instruction,
            documents: &req.documents,
            sum_ctx,
            summary_max,
        },
        &entries,
        previous,
        cancel,
    )
    .await?;

    let system_tokens = if req.system_prompt.trim().is_empty() {
        0
    } else {
        counter.count(req.system_prompt.trim(), cancel).await?
    };
    let previous_block = match &plan.previous {
        Some(a) => counter.count(&summary_block(&a.summary), cancel).await?,
        None => 0,
    };
    let block = counter.count(&summary_block(&summary), cancel).await?;
    let tokens_before = system_tokens
        + previous_block
        + previous_pinned_tokens
        + tokens[start..].iter().sum::<u64>();
    let tokens_after = system_tokens + block + plan.pinned_tokens + plan.kept_tokens;
    // A summary at least as long as what it replaces only loses detail: keep the originals.
    super::require_gain(tokens_before, tokens_after)?;
    let summary_tokens = match summary_tokens {
        Some(n) => n,
        None => counter.count(&summary, cancel).await?,
    };

    let covered =
        plan.previous.as_ref().map_or(0, |a| a.marker.covered) + plan.summarize.len() as u32;
    let marker = Marker {
        version: MARKER_VERSION,
        boundary_id: path[plan.boundary].id.clone(),
        covered,
        turns: plan.turns as u32,
        kept_turns: plan.kept_turns as u32,
        trigger: req.trigger,
        strategy: req.strategy,
        instruction: instruction.map(|i| i.trim().to_string()),
        model: req.endpoint.model.clone().filter(|m| !m.is_empty()),
        tokens_before,
        tokens_after,
        summary_tokens,
        tool_results_cleared: cleared,
        exact_tokens: counter.exact(),
        previous_id: plan
            .previous
            .as_ref()
            .map(|a| path[a.marker_index].id.clone()),
        undone: false,
        mid_turn: req.mid_turn,
        pinned_id: plan.pinned.map(|p| path[p].id.clone()),
        created_at: crate::state::now_ms(),
    };
    if cancel.load(Ordering::SeqCst) {
        return Err(cancelled());
    }
    let appended = db.append_message(AppendMessage {
        conversation_id: req.conversation_id.clone(),
        parent_id: Some(last.id.clone()),
        role: "system".into(),
        variant: VariantInput {
            content: summary,
            model_alias: marker.model.clone(),
            meta: Some(json!({ META_KEY: marker })),
            ..Default::default()
        },
        id: None,
        variant_id: None,
        any_parent_variant: false,
    })?;
    let view = db.get_conversation(&req.conversation_id)?;
    Ok(CompactOutcome {
        view,
        marker_id: appended.id,
        marker,
        requests,
    })
}

// ---- transcripts outside a stored conversation ----

/// A summary of plain transcript entries (a Responses chain, see `gateway::responses`).
#[derive(Debug, Clone, Default)]
pub struct TranscriptRequest {
    /// Where the summary is requested (the gateway's own `/v1`).
    pub endpoint: Endpoint,
    pub tokenize: Option<TokenizeEndpoint>,
    /// Context of the model that writes the summary.
    pub n_ctx: u64,
    /// Output budget (default: 1/8 of the context, 256-2048).
    pub summary_tokens: Option<u64>,
    pub instruction: Option<String>,
    pub strategy: Strategy,
    /// Transcript entries, oldest first.
    pub entries: Vec<String>,
    /// An earlier summary to merge.
    pub previous: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranscriptSummary {
    pub summary: String,
    pub summary_tokens: u64,
    /// Tokens of the entries and the earlier summary block, before summarizing.
    pub tokens_before: u64,
    pub requests: u32,
    /// Token counts came from a tokenizer (else estimated).
    pub exact_tokens: bool,
}

/// Writes the summary of `req.entries` (merging `req.previous`) with the model behind
/// `req.endpoint`; chunks the transcript when it does not fit one request.
pub async fn summarize_transcript(
    req: &TranscriptRequest,
    cancel: &AtomicBool,
) -> Result<TranscriptSummary, String> {
    let mut counter = Counter::new(req.tokenize.as_ref(), &req.endpoint);
    let mut entries: Vec<(String, u64)> = Vec::with_capacity(req.entries.len());
    for text in &req.entries {
        let t = counter.count(text, cancel).await?;
        entries.push((text.clone(), t));
    }
    let previous_block = match &req.previous {
        Some(p) => counter.count(&summary_block(p), cancel).await?,
        None => 0,
    };
    let tokens_before = previous_block + entries.iter().map(|e| e.1).sum::<u64>();
    let instruction = req.instruction.as_deref().filter(|i| !i.trim().is_empty());
    let (summary, summary_tokens, requests) = summarize_entries(
        &mut counter,
        &req.endpoint,
        &SummarizeParams {
            strategy: req.strategy,
            instruction,
            documents: &[],
            sum_ctx: req.n_ctx,
            summary_max: summary_budget(req.n_ctx, req.summary_tokens),
        },
        &entries,
        req.previous.clone(),
        cancel,
    )
    .await?;
    let summary_tokens = match summary_tokens {
        Some(n) => n,
        None => counter.count(&summary, cancel).await?,
    };
    Ok(TranscriptSummary {
        summary,
        summary_tokens,
        tokens_before,
        requests,
        exact_tokens: counter.exact(),
    })
}

/// Tokens of `text` as one chat message (template overhead included) and whether a tokenizer
/// counted them (else an estimate).
pub async fn count_text(
    ep: &Endpoint,
    tokenize: Option<&TokenizeEndpoint>,
    text: &str,
    cancel: &AtomicBool,
) -> Result<(u64, bool), String> {
    let mut counter = Counter::new(tokenize, ep);
    let n = counter.count(text, cancel).await?;
    Ok((n, counter.exact()))
}

/// Undoes (`undone = true`) or re-applies a compaction marker; returns the conversation.
pub fn set_undone(
    db: &StateDb,
    message_id: &str,
    undone: bool,
) -> Result<ConversationView, String> {
    let conv = db.write(|tx| {
        let row: Option<(String, String, Option<String>, String)> = tx
            .query_row(
                "SELECT v.id, m.conversation_id, v.meta_json, m.role FROM messages m \
                 JOIN message_variants v ON v.message_id = m.id WHERE m.id = ?1 \
                 ORDER BY v.created_at DESC, v.rid DESC LIMIT 1",
                [message_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((vid, conv, meta, role)) = row else {
            return Err(StateError::not_found(
                err("conversations.message_not_found").with("id", message_id),
            ));
        };
        let mut meta: Value = meta
            .and_then(|m| serde_json::from_str(&m).ok())
            .unwrap_or(Value::Null);
        let has_marker = role == "system" && super::marker_of(Some(&meta)).is_some();
        if !has_marker {
            return Err(StateError::invalid(
                err("compaction.not_a_marker").with("id", message_id),
            ));
        }
        meta[META_KEY]["undone"] = json!(undone);
        tx.execute(
            "UPDATE message_variants SET meta_json = ?2 WHERE id = ?1",
            rusqlite::params![vid, meta.to_string()],
        )?;
        tx.execute(
            "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
            rusqlite::params![conv, crate::state::now_ms()],
        )?;
        Ok(conv)
    })?;
    Ok(db.get_conversation(&conv)?)
}

/// The markers on a path, oldest first (for tests and diagnostics).
pub fn markers(view: &ConversationView) -> Vec<Marker> {
    view.messages.iter().filter_map(message_marker).collect()
}

/// Counts the same chat request that will be sent. Tokenizing a concatenation of contents
/// cannot be exact: the model's chat template adds role markers, tool schemas and generation prefix.
pub async fn count_request(
    endpoint: Option<&TokenizeEndpoint>,
    body: &Value,
) -> Result<(u64, bool), String> {
    let fallback = || (super::estimate_tokens(&body.to_string()), false);
    let Some(ep) = endpoint else {
        return Ok(fallback());
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| crate::http_error::transport(&e, &ep.host_url))?;
    let root = ep.host_url.trim_end_matches('/');
    let mut template = body.clone();
    template["add_generation_prompt"] = json!(true);
    let mut request = client
        .post(format!("{root}/apply-template"))
        .json(&template);
    for (k, v) in &ep.headers {
        request = request.header(k, v);
    }
    let exact = async {
        let rendered: Value = request
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        let prompt = rendered["prompt"].as_str()?;
        let mut request = client
            .post(format!("{root}/tokenize"))
            .json(&json!({"content":prompt,"add_special":false}));
        for (k, v) in &ep.headers {
            request = request.header(k, v);
        }
        let result: Value = request
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        Some((result["tokens"].as_array()?.len() as u64, true))
    }
    .await;
    Ok(exact.unwrap_or_else(fallback))
}
