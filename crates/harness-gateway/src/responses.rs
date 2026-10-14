//! Stateful Responses on top of llama-server's stateless `/v1/responses` (see
//! `harness_core::gateway::responses` for the semantics and retention).
//!
//! * Request side ([`prepare`]): `previous_response_id` is resolved in the caller's scope and
//!   the stored conversation is prepended to `input`; `previous_response_id` and `store` are
//!   removed before forwarding (llama-server rejects the former). A request without `model`
//!   continues with the model of the previous response.
//! * Response side ([`Pending::complete`]): the response object gets `previous_response_id`,
//!   `store` and `instructions` (as OpenAI echoes them) and, with `store`, is saved before the
//!   client sees it, so a `GET` right after the response (or after `response.completed` of a
//!   stream) finds it. Streams are scanned by [`SseScan`] for the final event.
//! * `GET /v1/responses/{id}`, `DELETE /v1/responses/{id}`,
//!   `GET /v1/responses/{id}/input_items` ([`get`], [`delete`], [`input_items`]): scoped to
//!   the caller; another token's id (or an expired one) is a 404 like an unknown id.

use axum::body::Body;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use harness_core::gateway::responses::{
    self as core, assign_item_ids, chain_context, instructions_with_summary, item_entry,
    link_entries, normalize_input, parse_context_management, ChainSummary, CompactionAsk,
    CompactionSettings, ItemsQuery, Link, Retention, COMPACTION_KEY,
};
use harness_core::gateway::GatewaySettings;
use harness_core::state::{NewStoredResponse, StateDb, StateResult};
use harness_core::token_policy::GatewayEndpoint;
use serde_json::{json, Value};

use crate::auth::Caller;
use crate::errors::GatewayError;
use crate::GwState;

/// What the request side decided.
#[derive(Default)]
pub(crate) struct Prepared {
    /// The rewritten body (history prepended, gateway-only fields removed).
    pub body: Option<Bytes>,
    /// The model of the previous response (for a request without `model`).
    pub model: Option<String>,
    pub pending: Option<Pending>,
    /// A chained request whose body is built once the model is known ([`build_chained`]):
    /// the history may have to be summarized first, which needs the model's context window.
    pub chained: Option<Chained>,
}

/// A request that continues a stored chain, before its body exists.
pub(crate) struct Chained {
    doc: Value,
    chain: harness_core::state::CompactedChain,
    own_input: Vec<Value>,
    ask: Option<CompactionAsk>,
    settings: CompactionSettings,
}

/// What [`build_chained`] made of a chained request.
pub(crate) struct ChainedBody {
    pub body: Bytes,
    /// `x-harness-compaction` header value (`None` = nothing to report).
    pub note: Option<String>,
}

/// A stateful request in flight: how to finish (and maybe store) its response.
pub(crate) struct Pending {
    pub store: bool,
    scope: String,
    previous_id: Option<String>,
    instructions: Value,
    input: Vec<Value>,
    retention: Retention,
    /// Alias the gateway served the request with (set once resolved).
    pub model: String,
    /// The id a background request answered with before it ran: the finished response
    /// carries it instead of llama-server's own.
    forced_id: Option<String>,
    /// The compaction this request made of its chain; stored with the response.
    compaction: Option<ChainSummary>,
}

fn db() -> Result<std::sync::Arc<StateDb>, GatewayError> {
    harness_core::state::shared()
        .map_err(|e| GatewayError::new(500, "state_unavailable", format!("state.db: {e}")))
}

async fn with_db<T: Send + 'static>(
    f: impl FnOnce(&StateDb) -> StateResult<T> + Send + 'static,
) -> Result<T, GatewayError> {
    let db = db()?;
    tokio::task::spawn_blocking(move || f(&db))
        .await
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?
        .map_err(|e| GatewayError::new(500, "state_error", e.to_string()))
}

fn scope(caller: &Caller) -> String {
    core::scope_of(caller.token_id.as_deref(), caller.token_name.as_deref())
}

fn mentions(body: &[u8], needle: &[u8]) -> bool {
    body.windows(needle.len()).any(|w| w == needle)
}

/// Request side of `/v1/responses`.
pub(crate) async fn prepare(
    caller: &Caller,
    settings: &GatewaySettings,
    body: &Bytes,
    forced_id: Option<&str>,
) -> Result<Prepared, GatewayError> {
    let rs = &settings.responses;
    let has_previous = mentions(body, b"\"previous_response_id\"");
    let has_store = mentions(body, b"\"store\"");
    let has_background = mentions(body, b"\"background\"");
    let has_context = mentions(body, b"\"context_management\"");
    let has_include = mentions(body, b"\"include\"");
    let Ok(mut doc) = serde_json::from_slice::<Value>(body) else {
        return Ok(Prepared::default());
    };
    let Some(obj) = doc.as_object_mut() else {
        return Ok(Prepared::default());
    };
    validate_include(obj.get("include"))?;
    // llama-server has no `background`; the gateway handles it (a `true` never gets here).
    obj.remove("background");
    // `context_management` is the gateway's too (llama-server would ignore or reject it).
    let ask = parse_context_management(&obj.remove("context_management").unwrap_or(Value::Null))
        .map_err(|m| GatewayError::new(400, "invalid_value", m).param("context_management"))?;
    let wants_store = match obj.remove("store") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => b,
        Some(_) => {
            return Err(
                GatewayError::new(400, "invalid_type", "'store' must be a boolean.").param("store"),
            )
        }
    };
    let previous = match obj.remove("previous_response_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s),
        Some(_) => {
            return Err(GatewayError::new(
                400,
                "invalid_type",
                "'previous_response_id' must be a response id string.",
            )
            .param("previous_response_id"))
        }
    };
    let scope = scope(caller);
    let retention = rs.retention();
    let own_input = normalize_input(obj.get("input"));
    let mut model = None;
    let mut chained = None;
    if let Some(prev) = &previous {
        let (sc, id) = (scope.clone(), prev.clone());
        let chain = with_db(move |db| db.response_chain_compacted(&sc, &id, &retention)).await?;
        let chain = chain.map_err(|missing| {
            let hint = if rs.store {
                ""
            } else {
                " Storing responses is turned off in Settings > Gateway."
            };
            GatewayError::new(
                404,
                "previous_response_not_found",
                format!("Previous response with id '{missing}' not found.{hint}"),
            )
            .param("previous_response_id")
        })?;
        model = chain
            .links
            .last()
            .map(|r| r.model.clone())
            .filter(|m| !m.is_empty());
        // The body is built after the model is resolved (it may need compaction).
        chained = Some(chain);
    }
    let instructions = obj.get("instructions").cloned().unwrap_or(Value::Null);
    let rebuilt = has_previous || has_store || has_background || has_context || has_include;
    let chained = chained.map(|chain| Chained {
        doc: Value::Null,
        chain,
        own_input: own_input.clone(),
        ask,
        settings: rs.compaction.clone(),
    });
    let body = (rebuilt && chained.is_none())
        .then(|| Bytes::from(serde_json::to_vec(&doc).unwrap_or_default()));
    let chained = chained.map(|mut c| {
        c.doc = std::mem::take(&mut doc);
        c
    });
    Ok(Prepared {
        body,
        chained,
        model,
        pending: Some(Pending {
            store: wants_store && rs.store,
            scope,
            previous_id: previous,
            instructions,
            input: own_input,
            retention,
            model: String::new(),
            forced_id: forced_id.map(str::to_string),
            compaction: None,
        }),
    })
}

const LOGPROBS_INCLUDE: &str = "message.output_text.logprobs";
/// Asked for by stateless clients (Codex sends it on every request) so they can replay the
/// model's reasoning. Local reasoning is not encrypted: it is already returned as plain text in
/// the reasoning items, which carry no `encrypted_content`. Accepted, not refused, so the client
/// works; it simply replays the plain reasoning (or none).
const ENCRYPTED_REASONING_INCLUDE: &str = "reasoning.encrypted_content";

fn validate_include(include: Option<&Value>) -> Result<(), GatewayError> {
    let Some(include) = include else {
        return Ok(());
    };
    let Some(values) = include.as_array() else {
        return Err(bad_request(
            "invalid_type",
            "'include' must be an array of strings.",
            "include",
        ));
    };
    for value in values {
        let Some(value) = value.as_str() else {
            return Err(bad_request(
                "invalid_type",
                "'include' must be an array of strings.",
                "include",
            ));
        };
        if value == ENCRYPTED_REASONING_INCLUDE {
            continue;
        }
        let detail = if value == LOGPROBS_INCLUDE {
            "llama-server's Responses adapter currently returns an empty logprobs array even when token probabilities are requested"
        } else {
            "the required hosted tool or encrypted reasoning data does not exist locally"
        };
        return Err(bad_request(
            "unsupported_value",
            &format!("The include value '{value}' is not available for local models: {detail}."),
            "include",
        ));
    }
    Ok(())
}

fn validate_include_query(include: Option<&str>) -> Result<(), GatewayError> {
    match include {
        None => Ok(()),
        Some(value) => validate_include(Some(&json!([value]))),
    }
}

/// Builds the body of a chained request: the stored links (after the newest summary) in front
/// of the input, the summary in the instructions. With compaction asked for (or turned on) and
/// the conversation over the threshold, the older links are summarized first by the model
/// itself, straight against its llama-server, and the summary is stored on the new response.
/// A summary that cannot be written never fails the request: it runs uncompacted (and the
/// header says why).
pub(crate) async fn build_chained(
    state: &GwState,
    def: &harness_core::domain::ModelDefinition,
    chained: Chained,
    pending: Option<&mut Pending>,
) -> Result<ChainedBody, GatewayError> {
    let Chained {
        mut doc,
        chain,
        own_input,
        ask,
        settings,
    } = chained;
    let instructions = doc.get("instructions").cloned().unwrap_or(Value::Null);
    let mut note = None;
    let mut summary = chain.summary.clone();
    let mut links = chain.links;
    let wanted = ask.is_some() || settings.auto;
    if wanted && links.len() > settings.keep_links as usize {
        match compact_links(
            state,
            def,
            &instructions,
            &links,
            &own_input,
            summary.as_ref(),
            ask,
            &settings,
        )
        .await
        {
            Ok(Compaction::Done(mark, kept)) => {
                note = Some(format!(
                    "{}->{} tokens",
                    mark.tokens_before, mark.tokens_after
                ));
                links.drain(..links.len() - kept);
                summary = Some(mark.clone());
                if let Some(p) = pending {
                    p.compaction = Some(mark);
                }
            }
            Ok(Compaction::Below) => {}
            Err(why) => {
                eprintln!(
                    "[Harness Gateway] chain compaction skipped for {}: {why}",
                    def.alias
                );
                note = Some(format!("skipped: {why}"));
            }
        }
    }
    let views: Vec<Link> = links
        .iter()
        .map(|r| Link {
            input: &r.input,
            response: &r.response,
        })
        .collect();
    let mut input = chain_context(&views);
    input.extend(own_input);
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("input".into(), Value::Array(input));
        if let Some(s) = &summary {
            obj.insert(
                "instructions".into(),
                instructions_with_summary(&instructions, &s.summary),
            );
        }
    }
    Ok(ChainedBody {
        body: Bytes::from(serde_json::to_vec(&doc).unwrap_or_default()),
        note,
    })
}

enum Compaction {
    /// Summarized; the number of newest links kept verbatim.
    Done(ChainSummary, usize),
    /// The conversation fits: nothing to do.
    Below,
}

#[allow(clippy::too_many_arguments)]
async fn compact_links(
    state: &GwState,
    def: &harness_core::domain::ModelDefinition,
    instructions: &Value,
    links: &[harness_core::state::StoredResponse],
    own_input: &[Value],
    earlier: Option<&ChainSummary>,
    ask: Option<CompactionAsk>,
    settings: &CompactionSettings,
) -> Result<Compaction, String> {
    use harness_core::compaction::{
        count_text, require_gain, summarize_transcript, Endpoint, Strategy, TokenizeEndpoint,
        TranscriptRequest,
    };
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;

    if crate::speech::is_audio_cpp_model(def) {
        return Err("audio.cpp only accepts POST /v1/audio/speech through the gateway.".into());
    }
    let cancel = AtomicBool::new(false);
    // The summary is written by the same model, on a slot of its own server.
    let lease = state
        .scheduler
        .acquire_tokens(def, None)
        .await
        .map_err(|e| GatewayError::from_sched(&e).message)?;
    let window = lease
        .ctx_size
        .filter(|n| *n > 0)
        .ok_or("the context window is unknown")?;
    let headers: HashMap<String, String> = lease
        .api_key
        .iter()
        .map(|k| ("Authorization".to_string(), format!("Bearer {k}")))
        .collect();
    let tokenize = TokenizeEndpoint {
        host_url: lease.upstream.clone(),
        headers: headers.clone(),
    };
    let endpoint = Endpoint {
        base_url: harness_core::engine::llama_cpp().openai_base(&lease.upstream),
        headers,
        model: None,
    };
    let views: Vec<Link> = links
        .iter()
        .map(|r| Link {
            input: &r.input,
            response: &r.response,
        })
        .collect();
    let own: Vec<String> = own_input.iter().filter_map(item_entry).collect();
    let tokens = |text: String| {
        let (endpoint, tokenize, cancel) = (&endpoint, &tokenize, &cancel);
        async move {
            count_text(endpoint, Some(tokenize), &text, cancel)
                .await
                .map(|(n, _)| n)
        }
    };
    let mut full = String::new();
    if let Some(i) = instructions.as_str() {
        full.push_str(i);
        full.push('\n');
    }
    if let Some(s) = earlier {
        full.push_str(&s.summary);
        full.push('\n');
    }
    full.push_str(&link_entries(&views).join("\n"));
    full.push('\n');
    full.push_str(&own.join("\n"));
    let used = tokens(full).await?;
    let limit = ask
        .and_then(|a| a.threshold)
        .unwrap_or(window * u64::from(settings.threshold_percent) / 100);
    if used <= limit {
        return Ok(Compaction::Below);
    }
    let keep = (settings.keep_links as usize).min(links.len() - 1);
    let split = links.len() - keep;
    let older: Vec<Link> = views[..split].to_vec();
    let kept_views = &views[split..];
    let summarized = summarize_transcript(
        &TranscriptRequest {
            endpoint: endpoint.clone(),
            tokenize: Some(tokenize.clone()),
            n_ctx: window,
            summary_tokens: None,
            instruction: None,
            strategy: Strategy::default(),
            entries: link_entries(&older),
            previous: earlier.map(|s| s.summary.clone()),
        },
        &cancel,
    )
    .await?;
    let mut after = crate::responses::summary_block_text(&summarized.summary);
    after.push('\n');
    after.push_str(&link_entries(kept_views).join("\n"));
    after.push('\n');
    after.push_str(&own.join("\n"));
    let tokens_after = tokens(after).await?;
    // The request before had the earlier summary too: compare like with like.
    let tokens_before = used;
    require_gain(tokens_before, tokens_after).map_err(|e| e.to_string())?;
    Ok(Compaction::Done(
        ChainSummary {
            through: links[split - 1].id.clone(),
            summary: summarized.summary,
            tokens_before,
            tokens_after,
            model: Some(def.alias.clone()),
            links: older.len() as u32,
            created_at: crate::proxy::unix_ms() as i64,
        },
        keep,
    ))
}

fn summary_block_text(summary: &str) -> String {
    harness_core::compaction::summary_block(summary)
}

impl Pending {
    /// Makes every streamed lifecycle response carry the same gateway-owned fields as the
    /// non-streaming response object.
    pub(crate) fn decorate(&self, response: &mut Value) {
        let Some(obj) = response.as_object_mut() else {
            return;
        };
        if let Some(id) = &self.forced_id {
            obj.insert("id".into(), json!(id));
        }
        obj.insert("background".into(), json!(self.forced_id.is_some()));
        obj.insert("previous_response_id".into(), json!(self.previous_id));
        obj.insert("store".into(), json!(self.store));
        if let Some(c) = &self.compaction {
            if let Ok(v) = serde_json::to_value(c) {
                obj.insert(COMPACTION_KEY.into(), v);
            }
        }
        if !obj.contains_key("instructions") {
            obj.insert("instructions".into(), self.instructions.clone());
        }
    }

    /// Adds OpenAI's echoed fields to a response object; with `store` returns the row to save.
    fn finish(&self, response: &mut Value) -> Option<NewStoredResponse> {
        self.decorate(response);
        let obj = response.as_object_mut()?;
        if !self.store {
            return None;
        }
        let id = obj.get("id").and_then(Value::as_str)?.to_string();
        let created_at = obj
            .get("created_at")
            .and_then(Value::as_i64)
            .map(|s| s * 1000)
            .unwrap_or_else(|| crate::proxy::unix_ms() as i64);
        let mut input = self.input.clone();
        assign_item_ids(&mut input, &id);
        Some(NewStoredResponse {
            id,
            scope: self.scope.clone(),
            previous_id: self.previous_id.clone(),
            model: self.model.clone(),
            created_at,
            input,
            response: response.clone(),
        })
    }

    /// Rewrites complete SSE events, decorating any embedded Response object. Partial events
    /// stay in `buffer`; emitted bytes are canonical SSE and preserve non-JSON events.
    pub(crate) fn decorate_stream(&self, buffer: &mut Vec<u8>, chunk: &[u8]) -> Vec<u8> {
        buffer.extend(chunk.iter().copied().filter(|&b| b != b'\r'));
        let mut out = Vec::new();
        while let Some(pos) = buffer.windows(2).position(|w| w == b"\n\n") {
            let raw: Vec<u8> = buffer.drain(..pos + 2).collect();
            let data: Vec<&[u8]> = raw
                .split(|&b| b == b'\n')
                .filter_map(|line| line.strip_prefix(b"data:"))
                .map(|line| line.strip_prefix(b" ").unwrap_or(line))
                .collect();
            let joined = data.join(&b'\n');
            let Ok(mut event) = serde_json::from_slice::<Value>(&joined) else {
                out.extend_from_slice(&raw);
                continue;
            };
            if let Some(response) = event.get_mut("response") {
                self.decorate(response);
            }
            if let Some(kind) = event.get("type").and_then(Value::as_str) {
                out.extend_from_slice(b"event: ");
                out.extend_from_slice(kind.as_bytes());
                out.push(b'\n');
            }
            out.extend_from_slice(b"data: ");
            out.extend_from_slice(&serde_json::to_vec(&event).unwrap_or(joined));
            out.extend_from_slice(b"\n\n");
        }
        out
    }

    /// The stored form of a background response in `status` (before it has output).
    fn snapshot(&self, id: &str, model: &str, status: &str) -> NewStoredResponse {
        let mut input = self.input.clone();
        assign_item_ids(&mut input, id);
        let created_at = crate::proxy::unix_ms() as i64;
        NewStoredResponse {
            id: id.to_string(),
            scope: self.scope.clone(),
            previous_id: self.previous_id.clone(),
            model: model.to_string(),
            created_at,
            input,
            response: json!({
                "id": id,
                "object": "response",
                "created_at": created_at / 1000,
                "status": status,
                "background": true,
                "error": null,
                "incomplete_details": null,
                "instructions": self.instructions,
                "model": model,
                "output": [],
                "previous_response_id": self.previous_id,
                "store": true,
                "usage": null,
            }),
        }
    }

    /// Saves a finished response (blocking; call on a blocking thread). Failures are logged:
    /// the client still gets its answer.
    fn save(row: NewStoredResponse, retention: Retention) {
        let result = harness_core::state::shared()
            .and_then(|db| db.put_response(&row, &retention).map_err(|e| e.to_string()));
        if let Err(e) = result {
            eprintln!(
                "[Harness Gateway] response {} could not be stored: {}",
                row.id, e
            );
        }
    }

    /// Non-streaming: finishes the JSON body (and stores it) before it goes to the client.
    pub(crate) async fn complete(&self, body: &[u8]) -> Option<Vec<u8>> {
        let mut v: Value = serde_json::from_slice(body).ok()?;
        if let Some(row) = self.finish(&mut v) {
            let retention = self.retention;
            let _ = tokio::task::spawn_blocking(move || Self::save(row, retention)).await;
        }
        serde_json::to_vec(&v).ok()
    }

    /// Streaming: the final response object of the stream was seen; store it on a blocking
    /// thread (the relay holds the event until this finishes).
    pub(crate) fn complete_stream(
        &self,
        mut response: Value,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let row = self.finish(&mut response)?;
        let retention = self.retention;
        Some(tokio::task::spawn_blocking(move || {
            Self::save(row, retention)
        }))
    }
}

/// Finds the final response object in a Responses SSE stream. Keeps only the event being read.
#[derive(Default)]
pub(crate) struct SseScan {
    buf: Vec<u8>,
    /// Bytes of `buf` already searched for an event end.
    scanned: usize,
    done: bool,
}

impl SseScan {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Option<Value> {
        if self.done {
            return None;
        }
        self.buf
            .extend(chunk.iter().copied().filter(|&b| b != b'\r'));
        let mut found = None;
        loop {
            let from = self.scanned.saturating_sub(1);
            let Some(pos) = self.buf[from..]
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|p| p + from)
            else {
                self.scanned = self.buf.len();
                break;
            };
            let event: Vec<u8> = self.buf.drain(..pos + 2).collect();
            self.scanned = 0;
            if found.is_some()
                || !(mentions(&event, b"response.completed")
                    || mentions(&event, b"response.incomplete")
                    || mentions(&event, b"response.failed")
                    || mentions(&event, b"response.cancelled"))
            {
                continue;
            }
            let data: Vec<&[u8]> = event
                .split(|&b| b == b'\n')
                .filter_map(|l| l.strip_prefix(b"data:"))
                .map(|l| l.strip_prefix(b" ").unwrap_or(l))
                .collect();
            let data = data.join(&b'\n');
            let Ok(mut v) = serde_json::from_slice::<Value>(&data) else {
                continue;
            };
            let kind = v.get("type").and_then(Value::as_str);
            if matches!(
                kind,
                Some(
                    "response.completed"
                        | "response.incomplete"
                        | "response.failed"
                        | "response.cancelled"
                )
            ) {
                if let Some(r) = v.get_mut("response").map(Value::take) {
                    found = Some(r);
                }
            }
        }
        if found.is_some() {
            self.done = true;
            self.buf = Vec::new();
        }
        found
    }
}

fn not_found(id: &str) -> GatewayError {
    GatewayError::new(
        404,
        "not_found",
        format!("Response with id '{id}' not found."),
    )
}

async fn lookup(
    state: &GwState,
    headers: &HeaderMap,
    id: &str,
) -> Result<(Caller, harness_core::state::StoredResponse), GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Chat)?;
    let retention = state.scheduler.settings().responses.retention();
    let (sc, rid) = (scope(&caller), id.to_string());
    match with_db(move |db| db.get_response(&sc, &rid, &retention)).await? {
        Some(r) => {
            // A background response nobody is working on any more (the gateway restarted):
            // it will never finish.
            let key = task_key(&scope(&caller), id);
            if is_background(&r.response)
                && !is_terminal(&r.response)
                && !tasks().contains_key(&key)
            {
                let failed = settle(
                    &scope(&caller),
                    id,
                    retention,
                    "failed",
                    Some((
                        "interrupted",
                        "The gateway restarted before the response finished.",
                    )),
                )
                .await;
                let failed = failed.unwrap_or(r);
                let sc = scope(&caller);
                let replayable = with_db({
                    let sc = sc.clone();
                    let id = id.to_string();
                    move |db| db.has_response_events(&sc, &id)
                })
                .await
                .unwrap_or(false);
                if replayable {
                    let _ = append_event(
                        &sc,
                        id,
                        lifecycle_event("response.failed", failed.response.clone()),
                        retention,
                    )
                    .await;
                }
                return Ok((caller, failed));
            }
            Ok((caller, r))
        }
        None => Err(not_found(id)),
    }
}

/// Query parameters for retrieval and resumable background streaming.
#[derive(Debug, Default, serde::Deserialize)]
pub struct RetrieveQuery {
    #[serde(default)]
    stream: bool,
    starting_after: Option<i64>,
    #[serde(alias = "include[]")]
    include: Option<String>,
}

/// `GET /v1/responses/{id}` (or its persisted SSE stream).
pub async fn get(state: GwState, headers: HeaderMap, id: String, query: RetrieveQuery) -> Response {
    let (caller, stored) = match lookup(&state, &headers, &id).await {
        Ok(found) => found,
        Err(e) => return e.into_response(),
    };
    if let Err(error) = validate_include_query(query.include.as_deref()) {
        return error.into_response();
    }
    if !query.stream {
        return axum::Json(stored.response).into_response();
    }
    if !is_background(&stored.response) {
        return bad_request(
            "invalid_request_error",
            "Only a response created with `background: true` and `stream: true` can be streamed with GET.",
            "stream",
        )
        .into_response();
    }
    let sc = scope(&caller);
    let (check_scope, check_id) = (sc.clone(), id.clone());
    let replayable = with_db(move |db| db.has_response_events(&check_scope, &check_id)).await;
    match replayable {
        Ok(true) => replay_response(sc, id, query.starting_after.unwrap_or(-1)),
        Ok(false) => bad_request(
            "invalid_request_error",
            "This background response was not created with `stream: true` and cannot be streamed.",
            "stream",
        )
        .into_response(),
        Err(e) => e.into_response(),
    }
}

/// `DELETE /v1/responses/{id}`.
pub async fn delete(state: GwState, headers: HeaderMap, id: String) -> Response {
    let caller = match crate::auth::check(&state, &headers, false, GatewayEndpoint::Chat) {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };
    let (sc, rid) = (scope(&caller), id.clone());
    // Deleting a response that is still running stops it.
    if let Some(handle) = tasks().remove(&task_key(&sc, &id)) {
        handle.abort();
    }
    match with_db(move |db| db.delete_response(&sc, &rid)).await {
        Ok(true) => {
            axum::Json(json!({"id": id, "object": "response", "deleted": true})).into_response()
        }
        Ok(false) => not_found(&id).into_response(),
        Err(e) => e.into_response(),
    }
}

/// `GET /v1/responses/{id}/input_items`: the request's own input items, paged like OpenAI.
pub async fn input_items(
    state: GwState,
    headers: HeaderMap,
    id: String,
    query: ItemsQuery,
) -> Response {
    match lookup(&state, &headers, &id).await {
        Ok((_, r)) => {
            if let Err(error) = validate_include_query(query.include.as_deref()) {
                return error.into_response();
            }
            match core::items_page(&r.input, &query) {
                Ok(page) => axum::Json(page).into_response(),
                Err(msg) => GatewayError::new(400, "invalid_value", msg)
                    .param("order")
                    .into_response(),
            }
        }
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------------------
// Background responses (`background: true`) and `POST /v1/responses/{id}/cancel`
// ---------------------------------------------------------------------------------------
//
// The request is validated and stored as a `queued` response, answered at once, and then run
// in a task through the normal request path (queue, JIT load, token budget, analytics): the
// finished response replaces the stored one under the same id. A client polls
// `GET /v1/responses/{id}` until the status is final. With `stream: true`, the task itself
// consumes and persists SSE events, while POST/GET clients replay that log independently.
// `cancel` aborts the task, which drops the upstream request (llama-server stops generating).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Running background tasks by `scope` + id.
struct Tasks(Mutex<HashMap<String, tokio::task::AbortHandle>>);

impl Tasks {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, tokio::task::AbortHandle>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn contains_key(&self, key: &str) -> bool {
        self.lock().contains_key(key)
    }

    fn remove(&self, key: &str) -> Option<tokio::task::AbortHandle> {
        self.lock().remove(key)
    }
}

fn tasks() -> &'static Tasks {
    static TASKS: OnceLock<Tasks> = OnceLock::new();
    TASKS.get_or_init(|| Tasks(Mutex::new(HashMap::new())))
}

fn task_key(scope: &str, id: &str) -> String {
    format!("{scope}\u{1f}{id}")
}

fn is_background(response: &Value) -> bool {
    response.get("background").and_then(Value::as_bool) == Some(true)
}

fn is_terminal(response: &Value) -> bool {
    response
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(core::is_final_status)
}

/// Moves a stored, still unfinished response to the final `status` (with an `error` for
/// `failed`) and returns it. A response that already finished is returned unchanged (the
/// finished one wins a race). `None` when it does not exist (any more).
async fn settle(
    scope: &str,
    id: &str,
    retention: Retention,
    status: &'static str,
    error: Option<(&str, &str)>,
) -> Option<harness_core::state::StoredResponse> {
    let (sc, rid) = (scope.to_string(), id.to_string());
    let error = error.map(|(c, m)| (c.to_string(), m.to_string()));
    with_db(move |db| {
        let Some(mut row) = db.get_response(&sc, &rid, &retention)? else {
            return Ok(None);
        };
        if is_terminal(&row.response) {
            return Ok(Some(row));
        }
        if let Some(obj) = row.response.as_object_mut() {
            obj.insert("status".into(), json!(status));
            if let Some((code, message)) = &error {
                obj.insert("error".into(), json!({"code": code, "message": message}));
            }
        }
        db.put_response(
            &NewStoredResponse {
                id: row.id.clone(),
                scope: sc.clone(),
                previous_id: row.previous_id.clone(),
                model: row.model.clone(),
                created_at: row.created_at,
                input: row.input.clone(),
                response: row.response.clone(),
            },
            &retention,
        )?;
        Ok(Some(row))
    })
    .await
    .ok()
    .flatten()
}

async fn append_event(scope: &str, id: &str, event: Value, retention: Retention) -> Option<Value> {
    let (sc, rid) = (scope.to_string(), id.to_string());
    with_db(move |db| db.append_response_event(&sc, &rid, event, &retention))
        .await
        .ok()
        .flatten()
        .map(|stored| stored.event)
}

fn lifecycle_event(kind: &str, response: Value) -> Value {
    json!({"type": kind, "response": response})
}

fn event_bytes(event: &Value) -> Bytes {
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    let data = serde_json::to_vec(event).unwrap_or_else(|_| b"null".to_vec());
    let mut out = Vec::with_capacity(kind.len() + data.len() + 17);
    out.extend_from_slice(b"event: ");
    out.extend_from_slice(kind.as_bytes());
    out.extend_from_slice(b"\ndata: ");
    out.extend_from_slice(&data);
    out.extend_from_slice(b"\n\n");
    Bytes::from(out)
}

fn replay_response(scope: String, id: String, starting_after: i64) -> Response {
    struct Replay {
        scope: String,
        id: String,
        cursor: i64,
    }
    let stream = futures_util::stream::unfold(
        Replay {
            scope,
            id,
            cursor: starting_after,
        },
        |mut state| async move {
            loop {
                let (sc, rid, after) = (state.scope.clone(), state.id.clone(), state.cursor);
                let events = with_db(move |db| db.response_events_after(&sc, &rid, after))
                    .await
                    .unwrap_or_default();
                if let Some(last) = events.last() {
                    state.cursor = last.sequence_number;
                    let mut bytes = Vec::new();
                    for event in events {
                        bytes.extend_from_slice(&event_bytes(&event.event));
                    }
                    return Some((Ok::<_, std::io::Error>(Bytes::from(bytes)), state));
                }
                if !tasks().contains_key(&task_key(&state.scope, &state.id)) {
                    return None;
                }
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            }
        },
    );
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| {
            GatewayError::new(500, "gateway_error", "stream unavailable").into_response()
        })
}

#[derive(Default)]
struct EventCollector {
    buffer: Vec<u8>,
}

impl EventCollector {
    fn push(&mut self, chunk: &[u8]) -> Vec<Value> {
        self.buffer
            .extend(chunk.iter().copied().filter(|&b| b != b'\r'));
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            let raw: Vec<u8> = self.buffer.drain(..pos + 2).collect();
            let data: Vec<&[u8]> = raw
                .split(|&b| b == b'\n')
                .filter_map(|line| line.strip_prefix(b"data:"))
                .map(|line| line.strip_prefix(b" ").unwrap_or(line))
                .collect();
            if let Ok(event) = serde_json::from_slice::<Value>(&data.join(&b'\n')) {
                if event.get("type").and_then(Value::as_str).is_some() {
                    events.push(event);
                }
            }
        }
        events
    }
}

fn bad_request(code: &str, message: &str, param: &str) -> GatewayError {
    GatewayError::new(400, code, message).param(param)
}

/// `background: true` on `POST /v1/responses`: `None` when the request is not one (it goes
/// the normal way), else the immediate answer (the `queued` response, or the error).
pub(crate) async fn start_background(
    state: &GwState,
    headers: &HeaderMap,
    body: &Bytes,
) -> Option<Response> {
    if !mentions(body, b"\"background\"") {
        return None;
    }
    let doc: Value = serde_json::from_slice(body).ok()?;
    match doc.get("background") {
        Some(Value::Bool(true)) => {}
        Some(Value::Bool(false) | Value::Null) | None => return None,
        Some(_) => {
            return Some(
                bad_request(
                    "invalid_type",
                    "'background' must be a boolean.",
                    "background",
                )
                .into_response(),
            )
        }
    }
    Some(match start(state, headers, doc).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    })
}

async fn start(
    state: &GwState,
    headers: &HeaderMap,
    mut doc: Value,
) -> Result<Response, GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Chat)?;
    let settings = state.scheduler.settings();
    let obj = doc.as_object_mut().ok_or_else(|| {
        GatewayError::new(
            400,
            "invalid_json",
            "The request body must be a JSON object.",
        )
    })?;
    if !settings.responses.store || obj.get("store") == Some(&Value::Bool(false)) {
        return Err(bad_request(
            "invalid_request_error",
            "A background response has to be stored (`store` true, and storing responses \
             turned on in Settings > Gateway): its result is read with GET /v1/responses/{id}.",
            "background",
        ));
    }
    let streaming = match obj.get("stream") {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(_) => {
            return Err(bad_request(
                "invalid_type",
                "'stream' must be a boolean.",
                "stream",
            ))
        }
    };
    obj.remove("background");
    obj.insert("stream".into(), Value::Bool(streaming));
    let requested_model = obj.get("model").and_then(Value::as_str).map(str::to_string);
    let inner = Bytes::from(serde_json::to_vec(&doc).unwrap_or_default());

    let id = core::new_response_id();
    // Validates what the run would (the previous response, the types) before answering.
    let prep = prepare(&caller, &settings, &inner, Some(&id)).await?;
    let pending = prep
        .pending
        .ok_or_else(|| GatewayError::new(500, "gateway_error", "response state unavailable"))?;
    let model = requested_model.or(prep.model).unwrap_or_default();
    let queued = pending.snapshot(&id, &model, "queued");
    let retention = settings.responses.retention();
    let sc = scope(&caller);
    let row = queued.clone();
    with_db(move |db| db.put_response(&row, &retention)).await?;
    let answer = queued.response.clone();
    if streaming {
        append_event(
            &sc,
            &id,
            lifecycle_event("response.queued", answer.clone()),
            retention,
        )
        .await
        .ok_or_else(|| {
            GatewayError::new(500, "state_error", "The queued stream could not be stored.")
        })?;
    }

    let (st, hd) = (state.clone(), headers.clone());
    let (task_scope, task_id) = (sc.clone(), id.clone());
    let key = task_key(&sc, &id);
    let mut registry = tasks().lock();
    let join = tokio::spawn(async move {
        // In progress as soon as the task runs (it may still wait in the queue).
        let running = pending.snapshot(&task_id, &model, "in_progress");
        let _ = with_db(move |db| {
            // Never over a final status (cancelled before the task got going).
            match db.get_response(&running.scope, &running.id, &retention)? {
                Some(r) if is_terminal(&r.response) => Ok(()),
                _ => db.put_response(&running, &retention),
            }
        })
        .await;
        let resp =
            crate::proxy::handle_with(st, crate::Api::Responses, hd, inner, Some(task_id.clone()))
                .await;
        let status = resp.status();
        if status.is_success() && streaming {
            use futures_util::StreamExt;
            let mut body = resp.into_body().into_data_stream();
            let mut collector = EventCollector::default();
            while let Some(chunk) = body.next().await {
                match chunk {
                    Ok(chunk) => {
                        for event in collector.push(&chunk) {
                            let _ = append_event(&task_scope, &task_id, event, retention).await;
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let failed = settle(
                            &task_scope,
                            &task_id,
                            retention,
                            "failed",
                            Some(("upstream_error", &message)),
                        )
                        .await;
                        if let Some(row) = failed {
                            let _ = append_event(
                                &task_scope,
                                &task_id,
                                lifecycle_event("response.failed", row.response),
                                retention,
                            )
                            .await;
                        }
                        break;
                    }
                }
            }
            // `Pending::complete_stream` normally wrote the terminal response. If upstream
            // ended without one, make the stored state and replay stream terminal.
            if let Some(row) = settle(
                &task_scope,
                &task_id,
                retention,
                "failed",
                Some((
                    "server_error",
                    "The model stream ended without a final response.",
                )),
            )
            .await
            {
                if row.response["status"] == "failed" {
                    let already_failed = with_db({
                        let sc = task_scope.clone();
                        let rid = task_id.clone();
                        move |db| db.response_events_after(&sc, &rid, -1)
                    })
                    .await
                    .unwrap_or_default()
                    .iter()
                    .any(|event| event.event["type"] == "response.failed");
                    if !already_failed {
                        let _ = append_event(
                            &task_scope,
                            &task_id,
                            lifecycle_event("response.failed", row.response),
                            retention,
                        )
                        .await;
                    }
                }
            }
        } else if status.is_success() {
            // `Pending::complete` stored the finished response; if that did not happen
            // (an unreadable answer), it must not stay "in progress" for ever.
            let _ = settle(
                &task_scope,
                &task_id,
                retention,
                "failed",
                Some(("server_error", "The model's answer could not be stored.")),
            )
            .await;
        } else {
            let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
                .await
                .unwrap_or_default();
            let err: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            let code = err["error"]["code"].as_str().unwrap_or("server_error");
            let message = err["error"]["message"]
                .as_str()
                .unwrap_or("The request failed.");
            let failed = settle(
                &task_scope,
                &task_id,
                retention,
                "failed",
                Some((code, message)),
            )
            .await;
            if streaming {
                if let Some(row) = failed {
                    let _ = append_event(
                        &task_scope,
                        &task_id,
                        lifecycle_event("response.failed", row.response),
                        retention,
                    )
                    .await;
                }
            }
        }
        tasks().remove(&task_key(&task_scope, &task_id));
    });
    registry.insert(key, join.abort_handle());
    drop(registry);
    if streaming {
        Ok(replay_response(sc, id, -1))
    } else {
        Ok(axum::Json(answer).into_response())
    }
}

/// `POST /v1/responses/{id}/cancel`: stops a background response. Only responses created
/// with `background: true` can be cancelled; one that already finished is returned as it is.
pub async fn cancel(state: GwState, headers: HeaderMap, id: String) -> Response {
    let (caller, stored) = match lookup(&state, &headers, &id).await {
        Ok(x) => x,
        Err(e) => return e.into_response(),
    };
    if !is_background(&stored.response) {
        return bad_request(
            "invalid_request_error",
            "Cannot cancel a synchronous response: only responses created with `background: true` can be cancelled.",
            "response_id",
        )
        .into_response();
    }
    if is_terminal(&stored.response) {
        return axum::Json(stored.response).into_response();
    }
    let sc = scope(&caller);
    if let Some(handle) = tasks().remove(&task_key(&sc, &id)) {
        handle.abort();
    }
    let retention = state.scheduler.settings().responses.retention();
    match settle(&sc, &id, retention, "cancelled", None).await {
        Some(row) => {
            if with_db({
                let sc = sc.clone();
                let id = id.clone();
                move |db| db.has_response_events(&sc, &id)
            })
            .await
            .unwrap_or(false)
            {
                let _ = append_event(
                    &sc,
                    &id,
                    lifecycle_event("response.cancelled", row.response.clone()),
                    retention,
                )
                .await;
            }
            axum::Json(row.response).into_response()
        }
        None => not_found(&id).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_scan_finds_the_final_response_across_chunks() {
        // The tail of a real llama-server (b11306) stream, split mid-event and with CRLF.
        let stream = concat!(
            "event: response.output_text.delta\r\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"Hello\"}\r\n\r\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_X\",\"object\":\"response\",",
            "\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",",
            "\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\"}]}]}}\n\n"
        );
        let bytes = stream.as_bytes();
        let mut scan = SseScan::default();
        let mut found = None;
        for chunk in bytes.chunks(17) {
            if let Some(v) = scan.push(chunk) {
                assert!(found.is_none());
                found = Some(v);
            }
        }
        let r = found.expect("response.completed");
        assert_eq!(r["id"], "resp_X");
        assert_eq!(r["output"][0]["content"][0]["text"], "Hello");
        assert!(scan.push(b"event: x\n\n").is_none());
    }

    #[test]
    fn pending_echoes_fields_and_assigns_input_ids() {
        let p = Pending {
            store: true,
            scope: "anonymous".into(),
            previous_id: Some("resp_prev".into()),
            instructions: json!("Be brief."),
            input: normalize_input(Some(&json!("hi"))),
            retention: harness_core::gateway::responses::ResponsesSettings::default().retention(),
            model: "alias".into(),
            forced_id: None,
            compaction: None,
        };
        let mut v = json!({"id": "resp_new", "created_at": 1790812443, "output": []});
        let row = p.finish(&mut v).unwrap();
        assert_eq!(v["previous_response_id"], "resp_prev");
        assert_eq!(v["store"], true);
        assert_eq!(v["instructions"], "Be brief.");
        assert_eq!(row.created_at, 1_790_812_443_000);
        assert_eq!(row.input[0]["id"], "msg_new_0");
        assert_eq!(row.model, "alias");
        let off = Pending { store: false, ..p };
        let mut v = json!({"id": "resp_n2", "output": []});
        assert!(off.finish(&mut v).is_none());
        assert_eq!(v["store"], false);
    }

    #[test]
    fn every_streamed_response_object_gets_state_fields() {
        let p = Pending {
            store: true,
            scope: "token:one".into(),
            previous_id: Some("resp_prev".into()),
            instructions: json!("Be brief."),
            input: vec![],
            retention: harness_core::gateway::responses::ResponsesSettings::default().retention(),
            model: "alias".into(),
            forced_id: Some("resp_forced".into()),
            compaction: None,
        };
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"upstream\",\"status\":\"in_progress\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"upstream\",\"status\":\"completed\"}}\n\n"
        );
        let mut buffer = Vec::new();
        let mut output = Vec::new();
        for chunk in input.as_bytes().chunks(13) {
            output.extend_from_slice(&p.decorate_stream(&mut buffer, chunk));
        }
        assert!(buffer.is_empty());
        let text = String::from_utf8(output).unwrap();
        for event in text.split("\n\n").filter(|event| !event.is_empty()) {
            let data = event
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap();
            let parsed: Value = serde_json::from_str(data).unwrap();
            let response = &parsed["response"];
            assert_eq!(response["id"], "resp_forced");
            assert_eq!(response["background"], true);
            assert_eq!(response["previous_response_id"], "resp_prev");
            assert_eq!(response["store"], true);
            assert_eq!(response["instructions"], "Be brief.");
        }
    }

    #[test]
    fn include_rejects_values_that_local_responses_cannot_populate() {
        assert!(validate_include(Some(&json!([]))).is_ok());
        let err = validate_include(Some(&json!([LOGPROBS_INCLUDE]))).unwrap_err();
        assert_eq!(err.status, 400);
        assert!(err.message.contains("empty logprobs"));
        // Codex sends it on every request: accepted (local reasoning is plain text).
        assert!(validate_include(Some(&json!([ENCRYPTED_REASONING_INCLUDE]))).is_ok());
        let err = validate_include(Some(&json!([
            ENCRYPTED_REASONING_INCLUDE,
            "file_search_call.results"
        ])))
        .unwrap_err();
        assert_eq!(err.status, 400);
        assert_eq!(err.param.as_deref(), Some("include"));
        assert!(err.message.contains("file_search_call.results"));
        let err = validate_include(Some(&json!(LOGPROBS_INCLUDE))).unwrap_err();
        assert_eq!(err.status, 400);
    }
}
