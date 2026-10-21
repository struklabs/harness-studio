//! Knowledge bases (local RAG): create, list, delete, add and remove sources, index (only what
//! changed), rebuild, and ask with cited excerpts. Everything needs the `conversations` scope
//! (the documents are private) and works on `state.db` through the same core services as the
//! app. Indexing and questions need the gateway (`harness serve --gateway` or the app) because
//! the embedding, rerank, OCR and speech models are served by it.

use axum::Json;
use harness_core::error_key::err;
use harness_core::rag::kb::{self, KbAnswer, KbProgress, KnowledgeBase};
use harness_core::rag::{self, RetrieveOptions};
use harness_core::state::{KbUpdate, RagDocument};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::error::{blocking, ApiError, ApiResult, Body, ErrorBody, OptBody, P, Q};
use crate::AppState;

fn db() -> ApiResult<Arc<harness_core::state::StateDb>> {
    harness_core::state::shared().map_err(ApiError::core)
}

fn core<T, E: std::fmt::Display>(r: Result<T, E>) -> ApiResult<T> {
    r.map_err(|e| ApiError::core(e.to_string()))
}

#[derive(Deserialize, IntoParams, Default)]
#[into_params(parameter_in = Query)]
#[serde(default)]
pub struct StateParams {
    /// The embedding model (alias) in use now: a base built with another one is `stale`.
    /// Default: no check.
    pub embed_model: Option<String>,
}

fn current_model(p: &StateParams) -> Option<String> {
    p.embed_model.clone().filter(|m| !m.trim().is_empty())
}

/// Knowledge bases with their index state, document and chunk counts, disk use and measured
/// figures, newest first.
#[utoipa::path(get, operation_id = "list_knowledge_bases", path = "/knowledge-bases", tag = "knowledge",
    params(StateParams),
    responses((status = 200, body = Vec<KnowledgeBase>), (status = 403, body = ErrorBody)))]
pub async fn list(Q(q): Q<StateParams>) -> ApiResult<Json<Vec<KnowledgeBase>>> {
    let db = db()?;
    let current = current_model(&q);
    let out = blocking(move || kb::list(&db, current.as_deref())).await?;
    Ok(Json(core(out)?))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateBody {
    pub name: String,
    /// Folder whose files are indexed (and scanned again on every run).
    pub folder: Option<String>,
    /// OCR model (alias) for scanned PDF pages and images.
    pub ocr_model: Option<String>,
    /// Speech-recognition model (alias) for audio and video.
    pub asr_model: Option<String>,
    /// Rerank model (alias) that reorders the best excerpts of a question.
    pub rerank_model: Option<String>,
}

/// Creates an empty knowledge base. Index it with `POST /knowledge-bases/{id}/index`.
#[utoipa::path(post, operation_id = "create_knowledge_base", path = "/knowledge-bases", tag = "knowledge",
    request_body = CreateBody,
    responses((status = 200, body = KnowledgeBase), (status = 422, body = ErrorBody)))]
pub async fn create(Body(b): Body<CreateBody>) -> ApiResult<Json<KnowledgeBase>> {
    let db = db()?;
    let out = blocking(move || -> Result<KnowledgeBase, String> {
        if let Some(f) = b.folder.as_deref().filter(|f| !f.is_empty()) {
            if !std::path::Path::new(f).is_dir() {
                return Err(err("rag.kb_not_a_folder").with("path", f).into());
            }
        }
        let row = db
            .kb_create(&b.name, b.folder.as_deref().filter(|f| !f.is_empty()))
            .map_err(String::from)?;
        db.kb_update(
            &row.id,
            &KbUpdate {
                ocr_model: b.ocr_model,
                asr_model: b.asr_model,
                rerank_model: b.rerank_model,
                ..Default::default()
            },
        )
        .map_err(String::from)?;
        kb::get(&db, &row.id, None)
    })
    .await?;
    Ok(Json(core(out)?))
}

/// One knowledge base: state (`empty`, `indexing`, `ready`, `incomplete`, `stale`), counts, what
/// the last run cost (time per stage, CPU, memory), question latency and disk use.
#[utoipa::path(get, operation_id = "get_knowledge_base", path = "/knowledge-bases/{id}", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id"), StateParams),
    responses((status = 200, body = KnowledgeBase), (status = 404, body = ErrorBody)))]
pub async fn get_one(P(id): P<String>, Q(q): Q<StateParams>) -> ApiResult<Json<KnowledgeBase>> {
    let db = db()?;
    let current = current_model(&q);
    let out = blocking(move || kb::get(&db, &id, current.as_deref())).await?;
    Ok(Json(core(out)?))
}

#[derive(Deserialize, ToSchema, Default)]
#[serde(default)]
pub struct PatchBody {
    pub name: Option<String>,
    /// New folder (also how a moved folder is pointed at: documents keep their relative paths).
    /// An empty string clears it.
    pub folder: Option<String>,
    pub ocr_model: Option<String>,
    pub asr_model: Option<String>,
    /// `""` turns the rerank step off.
    pub rerank_model: Option<String>,
}

/// Changes the name, folder or models of a knowledge base (an empty string clears a model).
/// Models take effect on the next run; the rerank model on the next question.
#[utoipa::path(patch, operation_id = "update_knowledge_base", path = "/knowledge-bases/{id}", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    request_body = PatchBody,
    responses((status = 200, body = KnowledgeBase), (status = 404, body = ErrorBody)))]
pub async fn update(P(id): P<String>, Body(b): Body<PatchBody>) -> ApiResult<Json<KnowledgeBase>> {
    let db = db()?;
    let out = blocking(move || -> Result<KnowledgeBase, String> {
        if let Some(f) = b.folder.as_deref().filter(|f| !f.is_empty()) {
            if !std::path::Path::new(f).is_dir() {
                return Err(err("rag.kb_not_a_folder").with("path", f).into());
            }
        }
        db.kb_update(
            &id,
            &KbUpdate {
                name: b.name,
                folder: b.folder,
                ocr_model: b.ocr_model,
                asr_model: b.asr_model,
                rerank_model: b.rerank_model,
            },
        )
        .map_err(String::from)?;
        kb::get(&db, &id, None)
    })
    .await?;
    Ok(Json(core(out)?))
}

#[derive(Serialize, ToSchema)]
pub struct Deleted {
    pub deleted: bool,
}

/// Deletes the knowledge base with its documents and chunks (the source files stay). A running
/// index run is stopped first.
#[utoipa::path(delete, operation_id = "delete_knowledge_base", path = "/knowledge-bases/{id}", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    responses((status = 200, body = Deleted), (status = 404, body = ErrorBody)))]
pub async fn delete(P(id): P<String>) -> ApiResult<Json<Deleted>> {
    let db = db()?;
    kb::kb_jobs().cancel(&id);
    let out = blocking(move || db.kb_delete(&id)).await?;
    core(out)?;
    Ok(Json(Deleted { deleted: true }))
}

/// The documents of a knowledge base (metadata only): name (path below the folder), kind, size,
/// pages or duration, chunks, index state, the modification time and hash used for change
/// detection.
#[utoipa::path(get, operation_id = "list_knowledge_base_documents", path = "/knowledge-bases/{id}/documents", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    responses((status = 200, body = Vec<RagDocument>), (status = 404, body = ErrorBody)))]
pub async fn documents(P(id): P<String>) -> ApiResult<Json<Vec<RagDocument>>> {
    let db = db()?;
    let out = blocking(move || -> Result<_, String> {
        db.kb_get(&id).map_err(String::from)?;
        db.rag_documents(&id).map_err(String::from)
    })
    .await?;
    Ok(Json(core(out)?))
}

#[derive(Deserialize, ToSchema, Default)]
#[serde(default)]
pub struct IndexBody {
    /// Embedding model (alias). Default: the model the base was built with, else the
    /// recommended multilingual model when it is installed, else keyword search only.
    pub embed_model: Option<String>,
    /// Read and embed every document again.
    pub rebuild: bool,
}

#[derive(Serialize, ToSchema)]
pub struct IndexStarted {
    pub kb_id: String,
    pub started: bool,
    /// Embedding model of the run (`null` = keyword search only).
    pub embed_model: Option<String>,
}

async fn start_run(
    state: &AppState,
    id: &str,
    embed_model: Option<String>,
    rebuild: bool,
    folder: Option<String>,
    paths: Vec<String>,
) -> ApiResult<IndexStarted> {
    let db = db()?;
    let endpoint = rag::setup::gateway_endpoint(&state.ctx.data_dir)
        .ok_or_else(|| ApiError::core(String::from(err("rag.gateway_required"))))?;
    let (kb_row, mut req, default_model) = {
        let (db, id) = (db.clone(), id.to_string());
        let out = blocking(move || -> Result<_, String> {
            let kb_row = db.kb_get(&id).map_err(String::from)?;
            let req = kb::build_request(&db, &id, folder.as_deref(), paths)?;
            let default_model = kb_row
                .embed_model
                .clone()
                .or_else(|| rag::embedder::sync_definition().ok().flatten());
            Ok((kb_row, req, default_model))
        })
        .await?;
        core(out)?
    };
    let alias = embed_model
        .filter(|m| !m.trim().is_empty())
        .or(default_model);
    if let Some(alias) = &alias {
        req.embed = Some(core(
            rag::setup::embed_model(endpoint.clone(), alias).await,
        )?);
    }
    let (ocr, asr) = rag::setup::run_models(&endpoint, &kb_row);
    req.ocr = ocr;
    req.asr = asr;
    req.rebuild = rebuild;
    core(rag::setup::spawn_run(db, req, |_| {}))?;
    Ok(IndexStarted {
        kb_id: id.to_string(),
        started: true,
        embed_model: alias,
    })
}

/// Starts an index run in the background (poll `GET .../progress`). New files are added, files
/// whose size, modification time and hash changed are read and embedded again, documents of
/// removed files are dropped, and an unchanged file costs nothing. A base built with another
/// embedding model is re-embedded (its text is kept). Audio and video are transcribed with the
/// base's speech model, images read with its OCR model. 409 `rag.kb_busy` while it runs.
#[utoipa::path(post, operation_id = "index_knowledge_base", path = "/knowledge-bases/{id}/index", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    request_body(content = IndexBody, description = "All fields optional"),
    responses((status = 200, body = IndexStarted), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn index(
    axum::extract::State(state): axum::extract::State<AppState>,
    P(id): P<String>,
    OptBody(b): OptBody<IndexBody>,
) -> ApiResult<Json<IndexStarted>> {
    let b = b.unwrap_or_default();
    Ok(Json(
        start_run(&state, &id, b.embed_model, b.rebuild, None, Vec::new()).await?,
    ))
}

#[derive(Deserialize, ToSchema, Default)]
#[serde(default)]
pub struct AddSourcesBody {
    /// A folder. The first one becomes the base's folder (documents keep relative paths);
    /// another one is added file by file.
    pub folder: Option<String>,
    /// Single files.
    pub paths: Vec<String>,
    /// Index right away (default true).
    pub index: Option<bool>,
    /// Embedding model of that run (see `IndexBody`).
    pub embed_model: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SourcesAdded {
    /// Files taken from the folder or the list (the run decides which are new).
    pub files: usize,
    pub index: Option<IndexStarted>,
}

/// Adds a folder or files to a knowledge base and (by default) indexes what is new.
#[utoipa::path(post, operation_id = "add_knowledge_base_sources", path = "/knowledge-bases/{id}/sources", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    request_body = AddSourcesBody,
    responses((status = 200, body = SourcesAdded), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn add_sources(
    axum::extract::State(state): axum::extract::State<AppState>,
    P(id): P<String>,
    Body(b): Body<AddSourcesBody>,
) -> ApiResult<Json<SourcesAdded>> {
    let db = db()?;
    for p in &b.paths {
        if !std::path::Path::new(p).is_file() {
            return Err(ApiError::core(String::from(
                err("fs.read_failed")
                    .with("path", p)
                    .with("error", "not a file"),
            )));
        }
    }
    let extra = {
        let (id, folder, paths) = (id.clone(), b.folder.clone(), b.paths.clone());
        let out = blocking(move || kb::add_sources(&db, &id, folder.as_deref(), &paths)).await?;
        core(out)?
    };
    let files = extra.len();
    let index = if b.index.unwrap_or(true) {
        Some(start_run(&state, &id, b.embed_model, false, None, extra).await?)
    } else {
        None
    };
    Ok(Json(SourcesAdded { files, index }))
}

#[derive(Deserialize, ToSchema)]
pub struct RemoveSourcesBody {
    /// Document ids, names (paths below the folder) or source paths.
    pub sources: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SourcesRemoved {
    pub removed: usize,
}

/// Removes documents (and their chunks) from a knowledge base. A file that is still in the base's
/// folder comes back with the next run; take the folder out of the base to stop that.
#[utoipa::path(delete, operation_id = "remove_knowledge_base_sources", path = "/knowledge-bases/{id}/sources", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    request_body = RemoveSourcesBody,
    responses((status = 200, body = SourcesRemoved), (status = 404, body = ErrorBody)))]
pub async fn remove_sources(
    P(id): P<String>,
    Body(b): Body<RemoveSourcesBody>,
) -> ApiResult<Json<SourcesRemoved>> {
    let db = db()?;
    let out = blocking(move || kb::remove_sources(&db, &id, &b.sources)).await?;
    Ok(Json(SourcesRemoved {
        removed: core(out)?,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct ProgressView {
    pub running: bool,
    /// The newest report of the current or last run (`null` before the first run of this
    /// process).
    pub progress: Option<KbProgress>,
}

/// Progress of the base's index run: stage (`scan`, `extract`, `embed`), documents done and
/// total, the document in work, elapsed time.
#[utoipa::path(get, operation_id = "knowledge_base_progress", path = "/knowledge-bases/{id}/progress", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    responses((status = 200, body = ProgressView), (status = 404, body = ErrorBody)))]
pub async fn progress(P(id): P<String>) -> ApiResult<Json<ProgressView>> {
    let db = db()?;
    let key = id.clone();
    let out = blocking(move || db.kb_get(&key)).await?;
    core(out)?;
    Ok(Json(ProgressView {
        running: kb::kb_jobs().is_running(&id),
        progress: kb::progress_of(&id),
    }))
}

#[derive(Serialize, ToSchema)]
pub struct Cancelled {
    /// `false` when no run was active.
    pub cancelled: bool,
}

/// Stops the base's index run (finished documents stay; the rest resume on the next run).
#[utoipa::path(post, operation_id = "cancel_knowledge_base_index", path = "/knowledge-bases/{id}/index/cancel", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    responses((status = 200, body = Cancelled)))]
pub async fn cancel(P(id): P<String>) -> Json<Cancelled> {
    Json(Cancelled {
        cancelled: kb::kb_jobs().cancel(&id),
    })
}

#[derive(Deserialize, ToSchema)]
pub struct QueryBody {
    pub query: String,
    /// Excerpts to return (1-20, default 5).
    pub top_k: Option<usize>,
    /// Embedding model (alias) of the question. Default: the model the base was built with
    /// (a base without vectors is searched by keywords).
    pub embed_model: Option<String>,
    /// Rerank model (alias) for this question; default: the base's own. `false` for no rerank.
    pub rerank: Option<RerankChoice>,
}

/// A rerank model alias, or `false` to skip the base's rerank step for one question.
#[derive(Deserialize, ToSchema)]
#[serde(untagged)]
pub enum RerankChoice {
    Alias(String),
    Off(bool),
}

/// The best excerpts of the base for a question, numbered for citation (`[1]`...). Each source has
/// the document name, the page (PDF) or the time span (audio, video) of the excerpt, its text,
/// the similarity and the rerank score, and the answer carries the latency of the question.
#[utoipa::path(post, operation_id = "query_knowledge_base", path = "/knowledge-bases/{id}/query", tag = "knowledge",
    params(("id" = String, Path, description = "Knowledge base id")),
    request_body = QueryBody,
    responses((status = 200, body = KbAnswer), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
pub async fn query(
    axum::extract::State(state): axum::extract::State<AppState>,
    P(id): P<String>,
    Body(b): Body<QueryBody>,
) -> ApiResult<Json<KbAnswer>> {
    if b.query.trim().is_empty() {
        return Err(ApiError::bad_request("rag.query_empty"));
    }
    let db = db()?;
    let kb_row = {
        let (db, id) = (db.clone(), id.clone());
        core(blocking(move || db.kb_get(&id)).await?)?
    };
    let endpoint = rag::setup::gateway_endpoint(&state.ctx.data_dir);
    let alias = b
        .embed_model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or(kb_row.embed_model.clone());
    let mut opts = RetrieveOptions {
        query: b.query.clone(),
        top_k: b.top_k.unwrap_or(5),
        ..Default::default()
    };
    if let (Some(alias), Some(ep)) = (&alias, &endpoint) {
        opts.embed = Some(core(rag::setup::embed_model(ep.clone(), alias).await)?);
    }
    match (&b.rerank, &endpoint) {
        (Some(RerankChoice::Off(false)), _) => opts.no_rerank = true,
        (Some(RerankChoice::Alias(a)), Some(ep)) if !a.trim().is_empty() => {
            opts.rerank = Some((ep.clone(), a.clone()))
        }
        _ => {}
    }
    if alias.is_some() && endpoint.is_none() {
        // Vectors need the model the gateway serves; keywords still work.
        opts.embed = None;
    }
    let answer = core(kb::ask(&db, &id, &opts).await)?;
    Ok(Json(answer))
}
