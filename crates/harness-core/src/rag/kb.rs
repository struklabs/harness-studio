//! Knowledge bases: named, reusable document sets built from a folder or files.
//!
//! A knowledge base is stored like conversation documents (see `state::rag_kb`: its documents
//! are `rag_documents` of a hidden conversation row), so extraction, chunking with overlap,
//! embedding, the vector and keyword indexes and the retrieval of [`super::prepare`] are shared.
//! This module adds what a reusable base needs on top:
//!
//! * [`scan_folder`]: which files of a folder to index (supported kinds, no build/VCS folders).
//! * [`index_kb`]: one run over all documents of a base: new files are added, unfinished or
//!   outdated documents are resumed or re-embedded, with one progress stream, one cancel flag
//!   and the measured wall time (stored in the base and logged).
//! * [`kb_state`]: whether the index is empty, running, complete, incomplete or **stale** (it
//!   was built with another embedding model than the one in use, so its vectors are not
//!   comparable and the base should be rebuilt).
//! * [`fit_budget`]: keeps the excerpts of attached bases within a small share of the context.

use super::{
    jobs, run_job, EmbedModel, IndexJob, Jobs, Progress, KB_MAX_DOCUMENTS, KB_MAX_TOKENS,
    KB_TOKEN_SHARE,
};
use crate::error_key::err;
use crate::state::{KnowledgeBaseRow, RagDocument, StateDb};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Running knowledge base runs by base id (cancel flags).
pub fn kb_jobs() -> &'static Jobs {
    static JOBS: std::sync::OnceLock<Jobs> = std::sync::OnceLock::new();
    JOBS.get_or_init(Jobs::default)
}

/// Where the index of a knowledge base stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KbState {
    /// No documents yet.
    Empty,
    /// A run is working on it.
    Indexing,
    /// All documents are indexed with the model in use.
    Ready,
    /// Some documents failed, were cancelled or lack vectors of the model in use.
    Incomplete,
    /// Built with another embedding model (or vector size) than the one in use: rebuild.
    Stale,
}

/// A knowledge base with its index figures, for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeBase {
    #[serde(flatten)]
    pub row: KnowledgeBaseRow,
    pub documents: usize,
    pub chunks: i64,
    pub embedded: i64,
    pub failed: usize,
    pub state: KbState,
    /// Why the index is stale: `model` (another embedding model) or `dims` (vector size).
    pub stale_reason: Option<String>,
}

/// State of a base given its documents and the embedding model in use (`None` = none).
/// Returns the state and, for [`KbState::Stale`], the reason.
pub fn kb_state(
    row: &KnowledgeBaseRow,
    docs: &[RagDocument],
    current_model: Option<&str>,
    running: bool,
) -> (KbState, Option<&'static str>) {
    if docs.is_empty() {
        return (KbState::Empty, None);
    }
    if running || docs.iter().any(|d| super::state_is_active(&d.status)) {
        return (KbState::Indexing, None);
    }
    // Vectors of two models, or of two sizes, can never be compared: the index is stale.
    if let (Some(built), Some(now)) = (row.embed_model.as_deref(), current_model) {
        if built != now {
            return (KbState::Stale, Some("model"));
        }
    }
    if row.embed_model.is_none() && current_model.is_some() && row.indexed_at.is_some() {
        // Built with keyword search only; an embedding model is available now.
        return (KbState::Stale, Some("model"));
    }
    if let Some(dims) = row.embed_dims {
        if docs
            .iter()
            .any(|d| d.embed_dims.is_some_and(|dd| dd != dims))
        {
            return (KbState::Stale, Some("dims"));
        }
    }
    let wants_vectors = row.embed_model.is_some();
    let incomplete = docs.iter().any(|d| {
        d.status != "ready"
            || (wants_vectors
                && (d.embed_model.as_deref() != row.embed_model.as_deref()
                    || d.embedded < d.chunks))
    });
    if incomplete {
        (KbState::Incomplete, None)
    } else {
        (KbState::Ready, None)
    }
}

/// A base with its figures.
pub fn summarize(
    row: KnowledgeBaseRow,
    docs: &[RagDocument],
    current_model: Option<&str>,
) -> KnowledgeBase {
    let running = kb_jobs().is_running(&row.id);
    let (state, reason) = kb_state(&row, docs, current_model, running);
    KnowledgeBase {
        documents: docs.len(),
        chunks: docs.iter().map(|d| d.chunks).sum(),
        embedded: docs.iter().map(|d| d.embedded).sum(),
        failed: docs.iter().filter(|d| d.status == "failed").count(),
        stale_reason: reason.map(String::from),
        state,
        row,
    }
}

/// All bases (documents left "indexing" by a closed app become cancelled first).
pub fn list(db: &StateDb, current_model: Option<&str>) -> Result<Vec<KnowledgeBase>, String> {
    let mut out = Vec::new();
    for row in db.kb_list().map_err(String::from)? {
        let running = kb_jobs().is_running(&row.id);
        db.rag_reconcile(&row.id, |id| running || jobs().is_running(id))
            .map_err(String::from)?;
        let docs = db.rag_documents(&row.id).map_err(String::from)?;
        out.push(summarize(row, &docs, current_model));
    }
    Ok(out)
}

/// One base with its figures.
pub fn get(db: &StateDb, id: &str, current_model: Option<&str>) -> Result<KnowledgeBase, String> {
    let row = db.kb_get(id).map_err(String::from)?;
    let docs = db.rag_documents(id).map_err(String::from)?;
    Ok(summarize(row, &docs, current_model))
}

// ---- folders ----

/// Folders that never hold documents worth indexing.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "vendor",
    "__pycache__",
    "venv",
    "site-packages",
];
/// Folders are followed this deep.
const MAX_DEPTH: usize = 12;

/// Files of a folder worth indexing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderScan {
    /// Files to index, sorted by path.
    pub files: Vec<PathBuf>,
    /// Supported files left out (too large, generated or lock files).
    pub skipped: usize,
    /// The limit of documents per base was reached.
    pub truncated: bool,
}

/// Whether the start of a file holds NUL bytes (a binary file with a text extension would
/// only ever fail its document and keep the base "incomplete").
fn looks_binary(path: &Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 4096];
    std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut buf))
        .map(|n| buf[..n].contains(&0))
        .unwrap_or(false)
}

fn generated(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".lock")
        || n.ends_with("-lock.json")
        || n.ends_with(".min.js")
        || n.ends_with(".min.css")
        || n.ends_with(".map")
}

/// Walks `root` for files of a supported kind ([`super::extract::kind_of`]). Hidden folders,
/// build and dependency folders and symbolic links are not entered; generated files and files
/// above [`super::extract::MAX_FILE_BYTES`] are counted as skipped.
pub fn scan_folder(root: &Path) -> Result<FolderScan, String> {
    let meta = std::fs::metadata(root).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", root.display())
                .with("error", e),
        )
    })?;
    if !meta.is_dir() {
        return Err(err("rag.kb_not_a_folder")
            .with("path", root.display())
            .into());
    }
    let mut scan = FolderScan::default();
    walk(root, 0, &mut scan);
    scan.files.sort();
    if scan.files.len() > KB_MAX_DOCUMENTS as usize {
        scan.files.truncate(KB_MAX_DOCUMENTS as usize);
        scan.truncated = true;
    }
    Ok(scan)
}

fn walk(dir: &Path, depth: usize, scan: &mut FolderScan) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if depth < MAX_DEPTH && !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                walk(&path, depth + 1, scan);
            }
        } else if kind.is_file()
            && !name.starts_with('.')
            && super::extract::kind_of(&path).is_some()
        {
            let too_large = entry
                .metadata()
                .map(|m| m.len() > super::extract::MAX_FILE_BYTES || m.len() == 0)
                .unwrap_or(true);
            if too_large || generated(&name) || looks_binary(&path) {
                scan.skipped += 1;
            } else {
                scan.files.push(path);
            }
        }
    }
}

/// Document name of `file`: its path below `root` with `/` separators, else the file name.
pub fn doc_name(root: Option<&Path>, file: &Path) -> String {
    root.and_then(|r| file.strip_prefix(r).ok())
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| super::extract::file_name(file))
}

// ---- indexing ----

/// A progress report of a run (Tauri event `kb-progress`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KbProgress {
    pub kb_id: String,
    /// `indexing`, `done` or `cancelled`.
    pub status: String,
    pub docs_done: usize,
    pub docs_total: usize,
    /// Document being indexed.
    pub current: Option<String>,
    /// Chunks embedded / chunks of the current document.
    pub chunk_done: usize,
    pub chunk_total: usize,
    pub elapsed_ms: u64,
}

/// What a run does.
#[derive(Debug, Clone)]
pub struct KbIndexRequest {
    pub kb_id: String,
    /// Files to add (those the base already has are skipped).
    pub files: Vec<PathBuf>,
    /// Folder the document names are relative to.
    pub root: Option<PathBuf>,
    /// Embed with this model; `None` = keyword search only.
    pub embed: Option<EmbedModel>,
    /// Extract every document again and re-embed it (otherwise only new, unfinished and
    /// outdated documents are worked on).
    pub rebuild: bool,
}

/// Result of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KbRun {
    pub kb_id: String,
    /// Documents worked on in this run.
    pub processed: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub documents: usize,
    pub chunks: i64,
    pub embedded: i64,
    pub elapsed_ms: u64,
    pub model: Option<String>,
    pub dims: Option<i64>,
    /// Files that could not be added (limit reached).
    pub not_added: usize,
}

/// Whether a document needs work in a run.
fn needs_work(d: &RagDocument, embed: Option<&EmbedModel>, rebuild: bool) -> bool {
    if rebuild || d.status != "ready" {
        return true;
    }
    match embed {
        Some(e) => d.embed_model.as_deref() != Some(e.alias.as_str()) || d.embedded < d.chunks,
        None => false,
    }
}

/// Runs the indexing of a knowledge base to its end. The run must have been registered with
/// `kb_jobs().register(kb_id)`; `cancel` is its flag (all documents stop with it). One broken
/// document does not stop the run (its status says `failed`). Measures the wall time, stores it
/// in the base together with the embedding model and vector size, and logs one line.
pub async fn index_kb(
    db: Arc<StateDb>,
    req: KbIndexRequest,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(KbProgress) + Send + Sync,
) -> Result<KbRun, String> {
    let started = Instant::now();
    let result = index_kb_inner(&db, &req, &cancel, &on_progress, started).await;
    kb_jobs().finish(&req.kb_id, &cancel);
    result
}

async fn index_kb_inner(
    db: &Arc<StateDb>,
    req: &KbIndexRequest,
    cancel: &Arc<AtomicBool>,
    on_progress: &(impl Fn(KbProgress) + Send + Sync),
    started: Instant,
) -> Result<KbRun, String> {
    let kb = db.kb_get(&req.kb_id).map_err(String::from)?;
    let emit =
        |status: &str, done: usize, total: usize, cur: Option<&str>, cd: usize, ct: usize| {
            on_progress(KbProgress {
                kb_id: req.kb_id.clone(),
                status: status.to_string(),
                docs_done: done,
                docs_total: total,
                current: cur.map(String::from),
                chunk_done: cd,
                chunk_total: ct,
                elapsed_ms: started.elapsed().as_millis() as u64,
            })
        };
    emit("indexing", 0, 0, None, 0, 0);

    // 1. New files become pending documents.
    let known = db.kb_source_paths(&req.kb_id).map_err(String::from)?;
    let mut not_added = 0;
    for file in &req.files {
        let path = file.to_string_lossy().to_string();
        if known.contains(&path) {
            continue;
        }
        let name = doc_name(req.root.as_deref(), file);
        if db
            .rag_create_document_limited(&req.kb_id, &name, Some(&path), KB_MAX_DOCUMENTS)
            .is_err()
        {
            not_added += 1;
        }
    }

    // 2. The documents that need work.
    let docs = db.rag_documents(&req.kb_id).map_err(String::from)?;
    let work: Vec<RagDocument> = docs
        .into_iter()
        .filter(|d| !jobs().is_running(&d.id) && needs_work(d, req.embed.as_ref(), req.rebuild))
        .collect();
    let total = work.len();
    emit("indexing", 0, total, None, 0, 0);

    // 3. One document after the other (one embedding request stream, low memory).
    let mut failed = 0;
    let mut done = 0;
    let mut cancelled = false;
    for d in &work {
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        // A rebuild reads the file again; a resume keeps the stored chunks.
        let source = (req.rebuild || d.chunks == 0)
            .then(|| d.source_path.clone().map(PathBuf::from))
            .flatten()
            .filter(|p| p.is_file());
        jobs().register_shared(&d.id, cancel.clone());
        let state = Mutex::new((0usize, 0usize));
        let name = d.name.clone();
        let outcome = run_job(
            db.clone(),
            IndexJob {
                document_id: d.id.clone(),
                source,
                embed: req.embed.clone(),
            },
            cancel.clone(),
            |p: Progress| {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                if p.total > 0 {
                    *s = (p.done, p.total);
                }
                emit("indexing", done, total, Some(&name), s.0, s.1);
            },
        )
        .await;
        done += 1;
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        if outcome.is_err() {
            failed += 1;
        }
    }
    if cancelled {
        // Documents that were never reached stay resumable.
        for d in &work {
            if let Ok(cur) = db.rag_document(&d.id) {
                if cur.status == "pending" {
                    let _ = db.rag_set_status(&d.id, "cancelled", None);
                }
            }
        }
    }

    // 4. Record the model, the size and the time.
    let docs = db.rag_documents(&req.kb_id).map_err(String::from)?;
    // The model of the run is the model of the index even when the run stopped early (the
    // documents without vectors then make the base incomplete, not stale).
    let model = req.embed.as_ref().map(|e| e.alias.clone());
    let dims = model.as_ref().and_then(|_| {
        docs.iter()
            .filter(|d| d.embedded > 0)
            .find_map(|d| d.embed_dims)
    });
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let chunks: i64 = docs.iter().map(|d| d.chunks).sum();
    let embedded: i64 = docs.iter().map(|d| d.embedded).sum();
    // A run that added nothing and worked on nothing keeps the recorded model.
    let (rec_model, rec_dims) = if work.is_empty() {
        (kb.embed_model.clone(), kb.embed_dims)
    } else {
        (model.clone(), dims)
    };
    db.kb_record_index(
        &req.kb_id,
        rec_model.as_deref(),
        rec_dims,
        elapsed_ms as i64,
    )
    .map_err(String::from)?;
    eprintln!(
        "[rag] knowledge base \"{}\": {} of {} documents worked on ({} failed{}), {} chunks, {} embedded with {} ({} dims) in {} ms",
        kb.name,
        done,
        total,
        failed,
        if cancelled { ", cancelled" } else { "" },
        chunks,
        embedded,
        model.as_deref().unwrap_or("keywords only"),
        dims.unwrap_or(0),
        elapsed_ms
    );
    emit(
        if cancelled { "cancelled" } else { "done" },
        done,
        total,
        None,
        0,
        0,
    );
    Ok(KbRun {
        kb_id: req.kb_id.clone(),
        processed: done,
        failed,
        cancelled,
        documents: docs.len(),
        chunks,
        embedded,
        elapsed_ms,
        model,
        dims,
        not_added,
    })
}

// ---- token budget ----

/// Tokens the excerpts of attached knowledge bases may add to a request: a quarter of the
/// chat model's context, at most [`KB_MAX_TOKENS`], at least 300.
pub fn token_budget(context_tokens: usize) -> usize {
    ((context_tokens as f32 * KB_TOKEN_SHARE) as usize).clamp(300, KB_MAX_TOKENS)
}

/// Keeps the leading items whose token costs fit `budget` (the best-ranked come first); the
/// first item is always kept so a question never goes out without any excerpt.
pub fn fit_budget<T>(items: Vec<T>, tokens_of: impl Fn(&T) -> usize, budget: usize) -> Vec<T> {
    let mut used = 0;
    let mut out = Vec::new();
    for item in items {
        let cost = tokens_of(&item);
        if !out.is_empty() && used + cost > budget {
            break;
        }
        used += cost;
        out.push(item);
    }
    out
}
