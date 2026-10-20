//! Local RAG for Playground conversations: attach documents, index them, retrieve excerpts.
//!
//! Everything runs on this machine. Nothing leaves it except requests to Harness's own
//! gateway (loopback), which serves the embedding/rerank models the user picked.
//!
//! # Pipeline
//! 1. **Extract** ([`extract`]): PDF per page, DOCX, Markdown, code, text. Pure Rust.
//! 2. **Chunk** ([`chunk`]): paragraph-packed chunks of ~900 characters with ~150 characters
//!    of overlap, headings kept, pages tracked.
//! 3. **Store** (`state.db` v5, [`crate::state::RagDocument`]): text, chunks, a keyword index
//!    (FTS5) and, once embedded, one L2-normalized f32 vector per chunk.
//! 4. **Embed** ([`client`]): batches of [`EMBED_BATCH`] chunks to `/v1/embeddings` of the
//!    gateway by the embedding definition's alias (JIT load), with the model's document
//!    prefix ([`models::prefixes`]). Progress is reported per batch; a cancel flag is checked
//!    between batches. One document is indexed at a time (low memory).
//! 5. **Retrieve** ([`prepare`]): documents that fit the chat model's context comfortably
//!    (≤ half of it, [`FULL_TEXT_SHARE`]) go in whole ("full text", like LM Studio); the rest
//!    are searched: exact cosine over the conversation's vectors + FTS5 BM25, fused with RRF,
//!    optionally reranked by a rerank definition (`/v1/rerank`), top-k excerpts numbered
//!    `[1]..[k]` for citations.
//!
//! Without an embedding model documents still work: full text when they fit, else keyword
//! search. Changing the embedding model re-embeds (vectors of another model are never mixed).
//!
//! # Limits
//! Files up to [`extract::MAX_FILE_BYTES`], text up to [`extract::MAX_TEXT_CHARS`],
//! [`crate::state::MAX_DOCUMENTS_PER_CONVERSATION`] documents and
//! [`MAX_CHUNKS_PER_CONVERSATION`] chunks per conversation (brute-force search stays in the
//! millisecond range at that size).

pub mod chunk;
pub mod client;
pub mod extract;
pub mod models;
pub mod search;

use crate::error_key::err;
use crate::state::{ChunkHit, RagDocument, StateDb};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use client::Endpoint;

/// Chunks per conversation (all documents).
pub const MAX_CHUNKS_PER_CONVERSATION: usize = 6000;
/// Chunks per embedding request.
pub const EMBED_BATCH: usize = 16;
/// Documents in "auto" mode go in whole while all full-text documents need at most this
/// share of the chat model's context.
pub const FULL_TEXT_SHARE: f32 = 0.5;
/// Context assumed when the chat model's context is unknown.
pub const DEFAULT_CONTEXT_TOKENS: usize = 4096;
/// Candidates taken from each ranking before fusion.
const CANDIDATES: usize = 50;

// ---- jobs ----

/// Running indexing jobs by document id (cancel flags).
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

    /// Registers a job before it is spawned (so it counts as running while it waits).
    pub fn register(&self, document_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.lock().insert(document_id.to_string(), flag.clone());
        flag
    }

    /// Asks a job to stop; false when none runs for the document.
    pub fn cancel(&self, document_id: &str) -> bool {
        match self.lock().get(document_id) {
            Some(f) => {
                f.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn is_running(&self, document_id: &str) -> bool {
        self.lock().contains_key(document_id)
    }

    fn finish(&self, document_id: &str, flag: &Arc<AtomicBool>) {
        let mut map = self.lock();
        if map.get(document_id).is_some_and(|f| Arc::ptr_eq(f, flag)) {
            map.remove(document_id);
        }
    }
}

/// One indexing job at a time.
fn index_permit() -> &'static tokio::sync::Semaphore {
    static SEM: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
    &SEM
}

// ---- indexing ----

/// The embedding model of an index run or a query.
#[derive(Debug, Clone)]
pub struct EmbedModel {
    pub endpoint: Endpoint,
    pub alias: String,
    pub query_prefix: String,
    pub doc_prefix: String,
}

impl EmbedModel {
    /// For a definition: its alias and the prefixes of its model family. A cloud embedding
    /// model (`cloud:<provider>/<model>`, BYOK) has no local family: no prefixes, and the
    /// index is marked with that id.
    pub fn for_definition(endpoint: Endpoint, def: &crate::domain::ModelDefinition) -> Self {
        let (q, d) = if crate::cloud::parse_model(&def.id).is_some() {
            ("", "")
        } else {
            models::prefixes_for(def)
        };
        Self {
            endpoint,
            alias: def.alias.clone(),
            query_prefix: q.to_string(),
            doc_prefix: d.to_string(),
        }
    }
}

/// A progress report of an indexing job (Tauri event `rag-progress`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub conversation_id: String,
    pub document_id: String,
    /// Document status (`extracting`, `embedding`, `ready`, `failed`, `cancelled`).
    pub status: String,
    /// Embedded chunks / all chunks (embedding phase).
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
}

/// What an indexing job does.
#[derive(Debug, Clone)]
pub struct IndexJob {
    pub document_id: String,
    /// Extract (again) from this file; `None` = keep the stored chunks (re-embed / resume).
    pub source: Option<PathBuf>,
    /// Embed with this model; `None` = keyword search only.
    pub embed: Option<EmbedModel>,
}

/// Runs an indexing job to its end (the document's status records the outcome). The job
/// must have been registered with [`Jobs::register`]; `cancel` is its flag.
pub async fn run_job(
    db: Arc<StateDb>,
    job: IndexJob,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(Progress) + Send + Sync,
) -> Result<RagDocument, String> {
    let _permit = index_permit().acquire().await.map_err(|e| e.to_string());
    let conversation_id = db
        .rag_document(&job.document_id)
        .map(|d| d.conversation_id)
        .unwrap_or_default();
    let report = |status: &str, done: usize, total: usize, error: Option<String>| {
        on_progress(Progress {
            conversation_id: conversation_id.clone(),
            document_id: job.document_id.clone(),
            status: status.to_string(),
            done,
            total,
            error,
        })
    };
    let result = index_inner(&db, &job, &cancel, &report).await;
    jobs().finish(&job.document_id, &cancel);
    let (status, error) = match &result {
        Ok(_) => ("ready", None),
        Err(e) if e == CANCELLED => ("cancelled", None),
        Err(e) => ("failed", Some(e.clone())),
    };
    // The document may have been deleted meanwhile.
    let _ = db.rag_set_status(&job.document_id, status, error.as_deref());
    let doc = db.rag_document(&job.document_id);
    let (done, total) = doc
        .as_ref()
        .map(|d| (d.embedded as usize, d.chunks as usize))
        .unwrap_or((0, 0));
    report(status, done, total, error.clone());
    match (result, error) {
        (_, Some(e)) => Err(e),
        _ => doc.map_err(String::from),
    }
}

const CANCELLED: &str = "rag.cancelled";

/// How often a blocking step looks at the cancel flag.
const CANCEL_POLL: Duration = Duration::from_millis(100);

/// Runs `f` on a blocking thread and returns its result, or `rag.cancelled` as soon as the
/// cancel flag is set. A PDF parse cannot be interrupted, so a cancelled one keeps running
/// to its end in the background (its result is dropped); the job itself stops at once instead
/// of waiting for it.
async fn blocking_or_cancel<T: Send + 'static>(
    cancel: &AtomicBool,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let mut task = tokio::task::spawn_blocking(f);
    loop {
        tokio::select! {
            done = &mut task => {
                return done.map_err(|e| String::from(err("task.join_failed").with("error", e)))?;
            }
            _ = tokio::time::sleep(CANCEL_POLL) => {
                if cancel.load(Ordering::SeqCst) {
                    return Err(CANCELLED.into());
                }
            }
        }
    }
}

async fn index_inner(
    db: &Arc<StateDb>,
    job: &IndexJob,
    cancel: &AtomicBool,
    report: &(impl Fn(&str, usize, usize, Option<String>) + Sync),
) -> Result<(), String> {
    let stop = || cancel.load(Ordering::SeqCst);
    if stop() {
        return Err(CANCELLED.into());
    }
    let id = job.document_id.as_str();
    let source = match &job.source {
        Some(p) => Some(p.clone()),
        None => {
            // Resume a document that never got its text.
            let d = db.rag_document(id).map_err(String::from)?;
            (d.chunks == 0)
                .then(|| d.source_path.map(PathBuf::from))
                .flatten()
        }
    };
    if let Some(path) = source {
        db.rag_set_status(id, "extracting", None)
            .map_err(String::from)?;
        report("extracting", 0, 0, None);
        let extracted = blocking_or_cancel(cancel, move || -> Result<_, String> {
            let doc = extract::extract_file(&path)?;
            let bytes = std::fs::read(&path).map_err(|e| {
                String::from(
                    err("fs.read_failed")
                        .with("path", path.display())
                        .with("error", e),
                )
            })?;
            let sha = {
                use sha2::{Digest, Sha256};
                let h = Sha256::digest(&bytes);
                h.iter().map(|b| format!("{b:02x}")).collect::<String>()
            };
            let chunks = chunk::chunk_pages(&doc.pages);
            Ok((doc, bytes.len(), sha, chunks))
        })
        .await?;
        if stop() {
            return Err(CANCELLED.into());
        }
        let (doc, size, sha, chunks) = extracted;
        db.rag_store_extraction(
            id,
            doc.kind.as_str(),
            size as i64,
            &sha,
            doc.page_count().map(i64::from),
            &doc.full_text(),
            &chunks,
            MAX_CHUNKS_PER_CONVERSATION,
        )
        .map_err(String::from)?;
    }
    let Some(embed) = &job.embed else {
        return Ok(());
    };
    let doc = db
        .rag_begin_embedding(id, &embed.alias)
        .map_err(String::from)?;
    let total = doc.chunks as usize;
    let mut done = doc.embedded as usize;
    report("embedding", done, total, None);
    loop {
        if stop() {
            return Err(CANCELLED.into());
        }
        let batch = db
            .rag_pending_chunks(id, EMBED_BATCH)
            .map_err(String::from)?;
        if batch.is_empty() {
            return Ok(());
        }
        let input: Vec<String> = batch
            .iter()
            .map(|c| {
                format!(
                    "{}{}",
                    embed.doc_prefix,
                    chunk::embed_text(&c.text, c.heading.as_deref())
                )
            })
            .collect();
        let vectors = client::embed(&embed.endpoint, &embed.alias, &input).await?;
        let rows: Vec<(i64, Vec<f32>)> = batch.iter().map(|c| c.chunk_id).zip(vectors).collect();
        db.rag_store_embeddings(id, &rows).map_err(String::from)?;
        done += rows.len();
        report("embedding", done, total, None);
    }
}

// ---- retrieval ----

/// Options of [`prepare`].
#[derive(Debug, Clone, Default)]
pub struct RetrieveOptions {
    /// The user's message.
    pub query: String,
    /// Excerpts to return (1–20).
    pub top_k: usize,
    /// Chat model context (tokens); `None` = unknown ([`DEFAULT_CONTEXT_TOKENS`]).
    pub context_tokens: Option<usize>,
    pub embed: Option<EmbedModel>,
    /// Rerank definition alias (same endpoint as `embed`, or its own).
    pub rerank: Option<(Endpoint, String)>,
}

/// A source shown under the answer (an excerpt, or a whole document in full-text mode).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Citation number (`[n]`); 0 for full-text documents (cited by name).
    pub n: usize,
    pub document_id: String,
    pub document_name: String,
    pub chunk_id: Option<i64>,
    pub page_start: Option<i64>,
    pub page_end: Option<i64>,
    pub heading: Option<String>,
    pub text: String,
    /// Cosine similarity to the question (dense match).
    pub similarity: Option<f32>,
    /// Rerank relevance score.
    pub rerank_score: Option<f32>,
    /// The whole document was given (full-text mode).
    pub full: bool,
}

/// What to add to a chat request for the documents of a conversation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Prepared {
    /// Full-text documents, for a system message (stable across turns: prompt cache).
    pub system: Option<String>,
    /// The user's message with numbered excerpts (retrieval), replacing the plain message.
    pub user: Option<String>,
    pub sources: Vec<Source>,
    /// `hybrid` (vectors + keywords), `keyword` or `None` when nothing was searched.
    pub retrieval: Option<String>,
    /// Reranked with this alias.
    pub reranked: Option<String>,
    /// Info keys for the UI: `rag.notice_*`.
    pub notices: Vec<String>,
    /// Estimated tokens added to the request.
    pub tokens: usize,
}

/// Which ready documents go in whole and which are searched.
pub fn plan(
    docs: &[RagDocument],
    context_tokens: Option<usize>,
) -> (Vec<&RagDocument>, Vec<&RagDocument>) {
    let ctx = context_tokens.unwrap_or(DEFAULT_CONTEXT_TOKENS);
    let budget = (ctx as f32 * FULL_TEXT_SHARE) as i64;
    let ready: Vec<&RagDocument> = docs.iter().filter(|d| d.status == "ready").collect();
    let mut full: Vec<&RagDocument> = ready.iter().copied().filter(|d| d.mode == "full").collect();
    let auto: Vec<&RagDocument> = ready.iter().copied().filter(|d| d.mode == "auto").collect();
    let mut search: Vec<&RagDocument> = ready
        .iter()
        .copied()
        .filter(|d| d.mode == "retrieval")
        .collect();
    let with_auto: i64 = full.iter().chain(&auto).map(|d| d.tokens).sum();
    if with_auto <= budget {
        full.extend(auto);
    } else {
        search.extend(auto);
    }
    (full, search)
}

fn page_label(start: Option<i64>, end: Option<i64>) -> Option<String> {
    match (start, end) {
        (Some(a), Some(b)) if a != b => Some(format!("pp. {a}-{b}")),
        (Some(a), _) => Some(format!("p. {a}")),
        _ => None,
    }
}

/// The system message for full-text documents.
pub fn full_text_block(docs: &[(String, Option<i64>, String)]) -> String {
    let mut s = String::from(
        "The user attached the documents below to this conversation. Use them when they are \
         relevant. When you use a document, cite it by name (and page when known), e.g. \
         (report.pdf, p. 3).\n",
    );
    for (name, pages, content) in docs {
        let pages = pages.map(|p| format!(" pages=\"{p}\"")).unwrap_or_default();
        s.push_str(&format!(
            "\n<document name=\"{}\"{pages}>\n{}\n</document>\n",
            name.replace('"', "'"),
            content.trim()
        ));
    }
    s
}

/// The user message with numbered excerpts.
pub fn excerpts_prompt(query: &str, sources: &[Source]) -> String {
    let mut s = String::from(
        "Answer the question below using the numbered excerpts from the user's documents. Cite \
         the excerpts you use inline as [1], [2]. If the excerpts do not contain the answer, \
         say so; answer from general knowledge only if you can, and say that you did. Answer \
         in the language of the question.\n\n<excerpts>\n",
    );
    for src in sources {
        let mut label = src.document_name.clone();
        if let Some(p) = page_label(src.page_start, src.page_end) {
            label.push_str(&format!(", {p}"));
        }
        if let Some(h) = &src.heading {
            label.push_str(&format!(" — {h}"));
        }
        s.push_str(&format!("[{}] {}\n{}\n\n", src.n, label, src.text.trim()));
    }
    s.push_str("</excerpts>\n\nQuestion: ");
    s.push_str(query.trim());
    s
}

fn hit_source(n: usize, h: ChunkHit, similarity: Option<f32>, rerank_score: Option<f32>) -> Source {
    Source {
        n,
        document_id: h.document_id,
        document_name: h.document_name,
        chunk_id: Some(h.chunk_id),
        page_start: h.page_start,
        page_end: h.page_end,
        heading: h.heading,
        text: h.text,
        similarity,
        rerank_score,
        full: false,
    }
}

/// Builds the document context of one chat turn.
pub async fn prepare(
    db: &StateDb,
    conversation_id: &str,
    opts: &RetrieveOptions,
) -> Result<Prepared, String> {
    let docs = db.rag_documents(conversation_id).map_err(String::from)?;
    let (full, search_docs) = plan(&docs, opts.context_tokens);
    let mut out = Prepared::default();
    if full.is_empty() && search_docs.is_empty() {
        return Ok(out);
    }
    let ctx = opts.context_tokens.unwrap_or(DEFAULT_CONTEXT_TOKENS);

    if !full.is_empty() {
        let mut blocks = Vec::new();
        for d in &full {
            let content = db.rag_content(&d.id).map_err(String::from)?;
            out.sources.push(Source {
                n: 0,
                document_id: d.id.clone(),
                document_name: d.name.clone(),
                chunk_id: None,
                page_start: d.pages.map(|_| 1),
                page_end: d.pages,
                heading: None,
                text: String::new(),
                similarity: None,
                rerank_score: None,
                full: true,
            });
            out.tokens += d.tokens as usize;
            blocks.push((d.name.clone(), d.pages, content));
        }
        if out.tokens as f32 > ctx as f32 * 0.9 {
            out.notices.push(err("rag.notice_over_context").into());
        }
        out.system = Some(full_text_block(&blocks));
    }

    if search_docs.is_empty() || opts.query.trim().is_empty() {
        return Ok(out);
    }
    let top_k = opts.top_k.clamp(1, 20);
    let ids: Vec<String> = search_docs.iter().map(|d| d.id.clone()).collect();

    // Dense ranking over the documents embedded with the current model.
    let mut dense: Vec<(i64, f32)> = Vec::new();
    // Query vector and stored vectors, kept to score the candidates the dense list missed.
    let mut query_vector: Vec<f32> = Vec::new();
    let mut stored_vectors: Vec<(i64, Vec<f32>)> = Vec::new();
    if let Some(embed) = &opts.embed {
        let current: Vec<String> = search_docs
            .iter()
            .filter(|d| d.embed_model.as_deref() == Some(embed.alias.as_str()) && d.embedded > 0)
            .map(|d| d.id.clone())
            .collect();
        if current.len() < search_docs.len() {
            out.notices.push(err("rag.notice_not_embedded").into());
        }
        if !current.is_empty() {
            let q = format!("{}{}", embed.query_prefix, opts.query.trim());
            match client::embed(&embed.endpoint, &embed.alias, &[q]).await {
                Ok(v) => {
                    let vectors = db
                        .rag_vectors(&current, &embed.alias)
                        .map_err(String::from)?;
                    dense = search::nearest(&v[0], &vectors, CANDIDATES);
                    query_vector = v[0].clone();
                    stored_vectors = vectors;
                }
                Err(e) => out
                    .notices
                    .push(err("rag.notice_embed_failed").with("error", e).into()),
            }
        }
    } else {
        out.notices.push(err("rag.notice_keyword_only").into());
    }
    let keyword = match search::fts_query(&opts.query) {
        Some(q) => db.rag_keyword(&ids, &q, CANDIDATES).map_err(String::from)?,
        None => Vec::new(),
    };
    out.retrieval = Some(
        if dense.is_empty() {
            "keyword"
        } else {
            "hybrid"
        }
        .into(),
    );
    let mut similarity: HashMap<i64, f32> = dense.iter().copied().collect();
    let fused = search::rrf(&[dense.iter().map(|(id, _)| *id).collect(), keyword]);
    // The reranker is the precision step, so it gets a wider pool: with a few hundred chunks
    // (a handful of long documents) the right one can sit only in the keyword list, far down
    // the fused order (the e2e test's 135 chunks needed more than 20 candidates).
    let pool_size = if opts.rerank.is_some() {
        (top_k * 4).max(40)
    } else {
        (top_k * 4).max(20)
    };
    let pool: Vec<i64> = fused.iter().take(pool_size).map(|(id, _)| *id).collect();
    // A chunk that entered the pool only through the keyword ranking has no dense score yet.
    search::fill_similarity(&mut similarity, &query_vector, &stored_vectors, &pool);
    let hits = db.rag_chunks(&pool).map_err(String::from)?;

    let mut chosen: Vec<(ChunkHit, Option<f32>)> = Vec::new();
    if let Some((ep, alias)) = opts.rerank.as_ref().filter(|_| hits.len() > 1) {
        let texts: Vec<String> = hits
            .iter()
            .map(|h| chunk::embed_text(&h.text, h.heading.as_deref()))
            .collect();
        match client::rerank(ep, alias, opts.query.trim(), &texts).await {
            Ok(order) => {
                out.reranked = Some(alias.clone());
                for (i, score) in order.into_iter().take(top_k) {
                    chosen.push((hits[i].clone(), Some(score)));
                }
            }
            Err(e) => out
                .notices
                .push(err("rag.notice_rerank_failed").with("error", e).into()),
        }
    }
    if chosen.is_empty() {
        chosen = hits.into_iter().take(top_k).map(|h| (h, None)).collect();
    }
    for (i, (hit, score)) in chosen.into_iter().enumerate() {
        let sim = similarity.get(&hit.chunk_id).copied();
        out.tokens += search::estimate_tokens(hit.text.chars().count());
        out.sources.push(hit_source(i + 1, hit, sim, score));
    }
    let excerpts: Vec<Source> = out.sources.iter().filter(|s| !s.full).cloned().collect();
    if !excerpts.is_empty() {
        out.user = Some(excerpts_prompt(&opts.query, &excerpts));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
