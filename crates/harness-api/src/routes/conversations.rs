//! Stored conversations (the Playground's chats): list, read, export, documents metadata and
//! context compaction. Everything needs the `conversations` scope (not covered by `read`:
//! chats are private) and works on `state.db` through the same core services as the GUI.

use std::collections::HashMap;

use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use harness_core::compaction::{self, CompactRequest, Endpoint, Marker, Strategy, TriggerKind};
use harness_core::error_key::err;
use harness_core::state::{
    ConversationExport, ConversationSummary, ConversationView, ListQuery, MarkdownLabels,
    RagDocument,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{blocking, ApiError, ApiResult, Body, ErrorBody, OptBody, P, Q};
use crate::AppState;

fn db() -> ApiResult<std::sync::Arc<harness_core::state::StateDb>> {
    harness_core::state::shared().map_err(ApiError::core)
}

#[derive(Deserialize, IntoParams, Default)]
#[into_params(parameter_in = Query)]
#[serde(default)]
pub struct ListParams {
    /// Only conversations in this folder.
    pub folder_id: Option<String>,
    /// Only conversations outside any folder.
    pub unfiled: bool,
    pub pinned_only: bool,
    pub favorites_only: bool,
    /// Full-text search over titles and message contents.
    pub query: Option<String>,
    /// Page size (default 50, at most 500).
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Conversations, newest first (title, folder, model, timestamps; with `query` also the
/// matching text fragment).
#[utoipa::path(get, operation_id = "list_conversations", path = "/conversations", tag = "conversations",
    params(ListParams),
    responses((status = 200, body = Vec<ConversationSummary>), (status = 403, body = ErrorBody)))]
pub async fn list(Q(q): Q<ListParams>) -> ApiResult<Json<Vec<ConversationSummary>>> {
    let db = db()?;
    let query = ListQuery {
        folder_id: q.folder_id,
        unfiled: q.unfiled,
        pinned_only: q.pinned_only,
        favorites_only: q.favorites_only,
        query: q.query,
        limit: Some(q.limit.unwrap_or(50).clamp(1, 500)),
        offset: q.offset,
        project_id: None,
    };
    let rows = blocking(move || db.list_conversations(&query))
        .await?
        .map_err(|e| ApiError::core(e.to_string()))?;
    Ok(Json(rows))
}

/// One conversation: its settings and the messages of the current path (root to leaf), each
/// with the shown variant (content, reasoning, tool calls, stats). A compaction marker is a
/// `system` message whose variant `meta.compaction` describes it.
#[utoipa::path(get, operation_id = "get_conversation", path = "/conversations/{id}", tag = "conversations",
    params(("id" = String, Path, description = "Conversation id")),
    responses(
        (status = 200, body = ConversationView),
        (status = 404, body = ErrorBody)))]
pub async fn get_one(P(id): P<String>) -> ApiResult<Json<ConversationView>> {
    let db = db()?;
    let view = blocking(move || db.get_conversation(&id))
        .await?
        .map_err(|e| ApiError::core(e.to_string()))?;
    Ok(Json(view))
}

#[derive(Deserialize, IntoParams, Default)]
#[into_params(parameter_in = Query)]
#[serde(default)]
pub struct ExportParams {
    /// `json` (default): the whole message tree with every variant (`harness.conversation.v1`,
    /// the file the GUI imports). `markdown`: the current path for reading.
    pub format: Option<String>,
}

/// Exports a conversation as JSON (`harness.conversation.v1`) or Markdown.
#[utoipa::path(get, operation_id = "export_conversation", path = "/conversations/{id}/export", tag = "conversations",
    params(("id" = String, Path, description = "Conversation id"), ExportParams),
    responses(
        (status = 200, description = "`harness.conversation.v1` JSON, or Markdown with `format=markdown`",
            content((ConversationExport = "application/json"), (String = "text/markdown"))),
        (status = 400, body = ErrorBody), (status = 404, body = ErrorBody)))]
pub async fn export(P(id): P<String>, Q(q): Q<ExportParams>) -> ApiResult<Response> {
    let db = db()?;
    match q.format.as_deref().unwrap_or("json") {
        "json" => {
            let doc: ConversationExport = blocking(move || db.export_json(&id))
                .await?
                .map_err(|e| ApiError::core(e.to_string()))?;
            Ok(Json(doc).into_response())
        }
        "markdown" | "md" => {
            let text = blocking(move || db.export_markdown(&id, &MarkdownLabels::default()))
                .await?
                .map_err(|e| ApiError::core(e.to_string()))?;
            Ok((
                [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
                text,
            )
                .into_response())
        }
        other => Err(ApiError::bad_request("api.invalid_format").with("format", other)),
    }
}

/// The documents attached to a conversation (name, kind, size, pages, chunks, index state,
/// mode). Metadata only: the text never leaves through the API.
#[utoipa::path(get, operation_id = "list_conversation_documents", path = "/conversations/{id}/documents", tag = "conversations",
    params(("id" = String, Path, description = "Conversation id")),
    responses((status = 200, body = Vec<RagDocument>), (status = 404, body = ErrorBody)))]
pub async fn documents(P(id): P<String>) -> ApiResult<Json<Vec<RagDocument>>> {
    let db = db()?;
    let docs = blocking(move || {
        db.get_conversation(&id)?;
        db.rag_documents(&id)
    })
    .await?
    .map_err(|e| ApiError::core(e.to_string()))?;
    Ok(Json(docs))
}

#[derive(Deserialize, ToSchema, Default)]
#[serde(default)]
pub struct CompactBody {
    /// Gateway alias that writes the summary and whose context window decides the plan
    /// (default: the conversation's own model).
    pub model: Option<String>,
    /// Context window of the chat model in tokens (default: the model's loaded server, else
    /// the definition's `context_size`).
    pub n_ctx: Option<u64>,
    /// Newest turns kept as they are (default 2; fewer when they exceed 40 % of the context).
    pub keep_turns: Option<usize>,
    /// Shorten old tool results before summarizing (default true).
    pub clear_tool_results: Option<bool>,
    /// Summary budget in tokens (default 1/8 of the context, 256 to 2048).
    pub summary_tokens: Option<u64>,
    /// Extra instruction for the summary ("keep the numbers").
    pub instruction: Option<String>,
}

/// Result of a compaction.
#[derive(Serialize, ToSchema)]
pub struct CompactResponse {
    pub conversation_id: String,
    /// Id of the marker message (undo it with `PUT .../compactions/{marker_id}`).
    pub marker_id: String,
    /// What was compacted: the token counts before and after, the turns kept.
    pub marker: Marker,
    /// Summarization requests made (chunks).
    pub requests: u32,
}

/// Compacts the conversation's current path: the older turns are replaced, in requests, by a
/// summary the model writes; the newest turns stay. Appends a marker message (nothing is
/// deleted; `PUT .../compactions/{message_id}` undoes it). The summary request goes through
/// the gateway (`harness serve --gateway` or the app's gateway), so it queues like any other
/// request. One compaction per conversation at a time (409 `compaction.busy`).
#[utoipa::path(post, operation_id = "compact_conversation", path = "/conversations/{id}/compact", tag = "conversations",
    params(("id" = String, Path, description = "Conversation id")),
    request_body(content = CompactBody, description = "All fields optional"),
    responses(
        (status = 200, body = CompactResponse),
        (status = 404, body = ErrorBody), (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn compact(
    axum::extract::State(state): axum::extract::State<AppState>,
    P(id): P<String>,
    OptBody(body): OptBody<CompactBody>,
) -> ApiResult<Json<CompactResponse>> {
    let body = body.unwrap_or_default();
    let db = db()?;
    let key = id.clone();
    let (conv, docs) = {
        let db = db.clone();
        blocking(move || {
            let view = db.get_conversation(&key)?;
            let docs = db.rag_documents(&key)?;
            Ok::<_, harness_core::state::StateError>((view.conversation, docs))
        })
        .await?
        .map_err(|e| ApiError::core(e.to_string()))?
    };
    let alias = body
        .model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or(conv.model_alias.clone())
        .ok_or_else(|| ApiError::core(String::from(err("compaction.no_model"))))?;

    // The summary is requested through the gateway like any client request.
    let status = harness_core::gateway::status();
    let (Some(endpoint), Some(scheduler)) = (
        status.endpoint.clone().filter(|_| status.running),
        harness_core::gateway::scheduler(),
    ) else {
        return Err(ApiError::core(String::from(err(
            "compaction.gateway_required",
        ))));
    };
    let n_ctx = match body.n_ctx.filter(|n| *n > 0) {
        Some(n) => n,
        None => scheduler
            .context_window(&alias)
            .await
            .map_err(|e| ApiError::core(e.message()))?
            .ok_or_else(|| {
                ApiError::core(String::from(
                    err("compaction.context_unknown").with("model", &alias),
                ))
            })?,
    };
    let mut headers = HashMap::new();
    let token_file = harness_core::gateway::client_token_path(&state.ctx.data_dir);
    if let Ok(token) = std::fs::read_to_string(token_file) {
        let token = token.trim();
        if !token.is_empty() {
            headers.insert("Authorization".to_string(), format!("Bearer {token}"));
        }
    }
    let request = CompactRequest {
        conversation_id: id.clone(),
        leaf_id: None,
        endpoint: Endpoint {
            base_url: endpoint.base_url,
            headers,
            model: Some(alias),
        },
        tokenize: None,
        n_ctx,
        summary_ctx: None,
        keep_turns: body.keep_turns.unwrap_or(2),
        clear_tool_results: body.clear_tool_results.unwrap_or(true),
        summary_tokens: body.summary_tokens,
        instruction: body.instruction,
        strategy: Strategy::default(),
        trigger: TriggerKind::Manual,
        mid_turn: false,
        documents: docs.iter().map(|d| d.name.clone()).collect(),
        system_prompt: conv.system_prompt,
    };

    let jobs = compaction::jobs();
    let flag = jobs
        .register(&id)
        .ok_or_else(|| ApiError::core(String::from(err("compaction.busy"))))?;
    let out = compaction::compact(&db, &request, &flag).await;
    jobs.finish(&id, &flag);
    let out = out.map_err(ApiError::core)?;
    Ok(Json(CompactResponse {
        conversation_id: id,
        marker_id: out.marker_id,
        marker: out.marker,
        requests: out.requests,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct CancelCompactResponse {
    /// `false` when no compaction was running.
    pub cancelled: bool,
}

/// Stops a running compaction of the conversation.
#[utoipa::path(post, operation_id = "cancel_compaction", path = "/conversations/{id}/compact/cancel", tag = "conversations",
    params(("id" = String, Path, description = "Conversation id")),
    responses((status = 200, body = CancelCompactResponse)))]
pub async fn cancel_compact(P(id): P<String>) -> Json<CancelCompactResponse> {
    Json(CancelCompactResponse {
        cancelled: compaction::jobs().cancel(&id),
    })
}

#[derive(Deserialize, ToSchema)]
pub struct UndoBody {
    /// `true` undoes the compaction (requests send the full history again), `false`
    /// re-applies it.
    pub undone: bool,
}

#[derive(Serialize, ToSchema)]
pub struct UndoResponse {
    pub conversation_id: String,
    pub message_id: String,
    pub undone: bool,
}

/// Undoes or re-applies a compaction marker of the conversation.
#[utoipa::path(put, operation_id = "set_compaction_undone", path = "/conversations/{id}/compactions/{message_id}", tag = "conversations",
    params(("id" = String, Path, description = "Conversation id"),
           ("message_id" = String, Path, description = "Marker message id (`marker_id`)")),
    request_body = UndoBody,
    responses(
        (status = 200, body = UndoResponse),
        (status = 404, body = ErrorBody)))]
pub async fn set_undone(
    P((id, message_id)): P<(String, String)>,
    Body(body): Body<UndoBody>,
) -> ApiResult<Json<UndoResponse>> {
    let db = db()?;
    let (conv, mid) = (id.clone(), message_id.clone());
    let undone = body.undone;
    blocking(move || {
        // The marker has to belong to this conversation.
        let view = db.get_conversation(&conv)?;
        if !view.messages.iter().any(|m| m.id == mid) {
            return Ok(Err(()));
        }
        compaction::set_undone(&db, &mid, undone).map(|_| Ok(()))
    })
    .await?
    .map_err(ApiError::core)?
    .map_err(|_| ApiError::not_found("compaction", &message_id))?;
    Ok(Json(UndoResponse {
        conversation_id: id,
        message_id,
        undone,
    }))
}
