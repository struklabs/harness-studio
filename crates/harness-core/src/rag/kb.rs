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
use crate::state::{DiskUsage, KnowledgeBaseRow, RagDocument, RunStats, StateDb};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct KnowledgeBase {
    #[serde(flatten)]
    pub row: KnowledgeBaseRow,
    pub documents: usize,
    pub chunks: i64,
    pub embedded: i64,
    pub failed: usize,
    /// Pages of its documents whose text came from the OCR model.
    pub ocr_pages: usize,
    pub state: KbState,
    /// Why the index is stale: `model` (another embedding model) or `dims` (vector size).
    pub stale_reason: Option<String>,
    /// Documents that are audio, video or images.
    pub media_documents: usize,
    /// What the index takes in the database.
    pub disk: DiskUsage,
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
        ocr_pages: docs.iter().map(|d| d.ocr_pages.len()).sum(),
        media_documents: docs.iter().filter(|d| d.meta.is_some()).count(),
        disk: DiskUsage::default(),
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
        let disk = db.kb_disk_usage(&row.id).unwrap_or_default();
        let mut kb = summarize(row, &docs, current_model);
        kb.disk = disk;
        out.push(kb);
    }
    Ok(out)
}

/// One base with its figures.
pub fn get(db: &StateDb, id: &str, current_model: Option<&str>) -> Result<KnowledgeBase, String> {
    let row = db.kb_get(id).map_err(String::from)?;
    let docs = db.rag_documents(id).map_err(String::from)?;
    let mut kb = summarize(row, &docs, current_model);
    kb.disk = db.kb_disk_usage(id).unwrap_or_default();
    Ok(kb)
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
    scan_folder_media(root, MediaScan::default())
}

/// Which media files a scan takes: audio and video need a speech-recognition model, images an
/// OCR model, so a base without them leaves those files out (counted as skipped).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MediaScan {
    pub audio_video: bool,
    pub images: bool,
}

impl MediaScan {
    fn takes(self, kind: super::media::MediaKind) -> bool {
        match kind {
            super::media::MediaKind::Image => self.images,
            _ => self.audio_video,
        }
    }
}

/// [`scan_folder`] that also takes the media files `media` allows.
pub fn scan_folder_media(root: &Path, media: MediaScan) -> Result<FolderScan, String> {
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
    walk(root, 0, media, &mut scan);
    scan.files.sort();
    if scan.files.len() > KB_MAX_DOCUMENTS as usize {
        scan.files.truncate(KB_MAX_DOCUMENTS as usize);
        scan.truncated = true;
    }
    Ok(scan)
}

fn walk(dir: &Path, depth: usize, media: MediaScan, scan: &mut FolderScan) {
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
                walk(&path, depth + 1, media, scan);
            }
        } else if kind.is_file()
            && !name.starts_with('.')
            && super::media::media_kind(&path).is_some()
        {
            let k = super::media::media_kind(&path).expect("checked above");
            let ok = media.takes(k)
                && entry
                    .metadata()
                    .map(|m| m.len() > 0 && m.len() <= k.max_bytes())
                    .unwrap_or(false);
            if ok {
                scan.files.push(path);
            } else if media.takes(k) {
                // Only a file the base could read but cannot (empty or too large) is skipped.
                scan.skipped += 1;
            }
        } else if kind.is_file()
            && !name.starts_with('.')
            && super::extract::kind_of(&path).is_some()
        {
            let too_large = entry
                .metadata()
                .map(|m| m.len() > super::extract::MAX_FILE_BYTES || m.len() == 0)
                .unwrap_or(true);
            // PDF and DOCX are binary by nature (a scan holds its images, a DOCX is a zip).
            let text_kind = !matches!(
                super::extract::kind_of(&path),
                Some(super::extract::DocKind::Pdf | super::extract::DocKind::Docx)
            );
            if too_large || generated(&name) || (text_kind && looks_binary(&path)) {
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct KbProgress {
    pub kb_id: String,
    /// `indexing`, `done` or `cancelled`.
    pub status: String,
    /// `scan` (what changed), `extract` (text, transcripts, OCR) or `embed`.
    #[serde(default)]
    pub stage: String,
    pub docs_done: usize,
    pub docs_total: usize,
    /// Document being indexed.
    pub current: Option<String>,
    /// Chunks embedded / chunks of the current document.
    pub chunk_done: usize,
    pub chunk_total: usize,
    pub elapsed_ms: u64,
}

const STAGES: [&str; 4] = ["scan", "extract", "embed", ""];

/// The stage of a run, readable from the progress callbacks of several tasks.
struct Stage(std::sync::atomic::AtomicUsize);

impl Stage {
    fn set(&self, s: &str) {
        let i = STAGES.iter().position(|x| *x == s).unwrap_or(3);
        self.0.store(i, Ordering::SeqCst);
    }

    fn get(&self) -> &'static str {
        STAGES[self.0.load(Ordering::SeqCst).min(3)]
    }
}

/// The newest progress report of each base (for callers that poll instead of listening).
fn last_progress() -> &'static Mutex<HashMap<String, KbProgress>> {
    static MAP: std::sync::OnceLock<Mutex<HashMap<String, KbProgress>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// The latest progress report of a base's run (the last run's final report once it ended).
pub fn progress_of(kb_id: &str) -> Option<KbProgress> {
    last_progress()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(kb_id)
        .cloned()
}

/// What a run does.
#[derive(Debug, Clone, Default)]
pub struct KbIndexRequest {
    pub kb_id: String,
    /// Files of the sources; documents of the base that are not among them are compared with
    /// the disk (see [`sync_sources`]).
    pub files: Vec<PathBuf>,
    /// Folder the document names are relative to.
    pub root: Option<PathBuf>,
    /// The folder was read completely: documents of it whose file is gone are dropped.
    pub prune: bool,
    /// Embed with this model; `None` = keyword search only.
    pub embed: Option<EmbedModel>,
    /// Recognize scanned PDF pages and images with this model (the base's OCR model).
    pub ocr: Option<super::ocr::OcrModel>,
    /// Transcribe audio and video with this model (the base's speech-recognition model).
    pub asr: Option<super::media::AsrModel>,
    /// Extract every document again and re-embed it (otherwise only new, changed, unfinished
    /// and outdated documents are worked on).
    pub rebuild: bool,
}

/// Result of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// New files, files whose content changed, files that were as before, documents dropped
    /// because their file is gone.
    pub added: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub removed: usize,
    /// Time of the extraction (text, transcripts, OCR) and of the embedding stage.
    pub extract_ms: u64,
    pub embed_ms: u64,
    pub cpu_avg_pct: f32,
    pub cpu_peak_pct: f32,
    pub rss_peak_bytes: u64,
}

/// Modification time of a file in milliseconds since the epoch.
pub fn mtime_ms(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

/// Path of `file` below `root` (`/` separators); `None` when it is not below it.
fn rel_of(root: Option<&Path>, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root?).ok()?;
    let s = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/");
    (!s.is_empty()).then_some(s)
}

/// Where the source file of a document is now: below the base's folder when the document has
/// a relative path (so a moved folder keeps working), else its stored path.
pub fn resolve_source(folder: Option<&str>, d: &RagDocument) -> Option<PathBuf> {
    match (folder.filter(|f| !f.is_empty()), d.rel_path.as_deref()) {
        (Some(f), Some(rel)) => Some(Path::new(f).join(rel)),
        _ => d.source_path.as_ref().map(PathBuf::from),
    }
}

/// What the sources look like compared with the index.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncResult {
    pub added: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub not_added: usize,
    /// Documents whose file changed (or all of them on a rebuild): they are read again.
    pub reextract: HashSet<String>,
}

enum FileState {
    Unchanged,
    /// Same content, other modification time: remember the new time.
    Touched(i64),
    Changed,
}

/// Compares a document with its file: size and modification time first (no reading), the
/// SHA-256 only when the time differs but the size does not (a file that was copied or
/// touched keeps its document).
fn file_state(d: &RagDocument, file: &Path) -> FileState {
    let Ok(meta) = std::fs::metadata(file) else {
        return FileState::Changed;
    };
    if d.sha256.is_empty() {
        // Never extracted: it is worked on anyway.
        return FileState::Unchanged;
    }
    let mtime = mtime_ms(file);
    if d.size != meta.len() as i64 {
        return FileState::Changed;
    }
    if d.mtime_ms.is_some() && d.mtime_ms == mtime {
        return FileState::Unchanged;
    }
    match super::media::sha256_file(file) {
        Ok(h) if h == d.sha256 => mtime.map_or(FileState::Unchanged, FileState::Touched),
        _ => FileState::Changed,
    }
}

/// Brings the documents of a base in line with its sources: new files become pending
/// documents, files that changed (size, modification time, then hash) are marked to be read
/// again, documents whose file is gone are dropped. Nothing is read or embedded here except
/// the hash of a file whose time changed.
pub fn sync_sources(db: &StateDb, req: &KbIndexRequest) -> Result<SyncResult, String> {
    let docs = db.rag_documents(&req.kb_id).map_err(String::from)?;
    let mut by_rel: HashMap<&str, &RagDocument> = HashMap::new();
    let mut by_path: HashMap<&str, &RagDocument> = HashMap::new();
    for d in &docs {
        if let Some(r) = d.rel_path.as_deref() {
            by_rel.insert(r, d);
        }
        if let Some(p) = d.source_path.as_deref() {
            by_path.insert(p, d);
        }
    }
    let mut out = SyncResult::default();
    let mut seen: HashSet<String> = HashSet::new();
    for file in &req.files {
        let abs = file.to_string_lossy().to_string();
        let rel = rel_of(req.root.as_deref(), file);
        let existing = rel
            .as_deref()
            .and_then(|r| by_rel.get(r).copied())
            .or_else(|| by_path.get(abs.as_str()).copied());
        let Some(d) = existing else {
            let name = doc_name(req.root.as_deref(), file);
            match db.rag_create_document_limited(&req.kb_id, &name, Some(&abs), KB_MAX_DOCUMENTS) {
                Ok(created) => {
                    out.added += 1;
                    seen.insert(created.id.clone());
                    if rel.is_some() {
                        let _ = db.rag_set_source_stat(&created.id, None, rel.as_deref(), None);
                    }
                }
                Err(_) => out.not_added += 1,
            }
            continue;
        };
        seen.insert(d.id.clone());
        if d.source_path.as_deref() != Some(abs.as_str()) {
            let _ = db.rag_set_source_path(&d.id, &abs);
        }
        if rel.is_some() && d.rel_path != rel {
            let _ = db.rag_set_source_stat(&d.id, None, rel.as_deref(), None);
        }
        if req.rebuild {
            out.changed += 1;
            out.reextract.insert(d.id.clone());
            continue;
        }
        match file_state(d, file) {
            FileState::Unchanged => out.unchanged += 1,
            FileState::Touched(m) => {
                out.unchanged += 1;
                let _ = db.rag_set_source_stat(&d.id, Some(m), None, None);
            }
            FileState::Changed => {
                out.changed += 1;
                out.reextract.insert(d.id.clone());
                let _ = db.rag_set_status(&d.id, "pending", None);
            }
        }
    }
    // Documents whose file is gone. Those of the scanned folder are only dropped when the
    // scan was complete; a file added on its own is dropped when it does not exist any more.
    let in_folder = |d: &RagDocument| {
        d.rel_path.is_some()
            || req
                .root
                .as_deref()
                .zip(d.source_path.as_deref())
                .is_some_and(|(r, p)| Path::new(p).starts_with(r))
    };
    for d in &docs {
        if seen.contains(&d.id) || jobs().is_running(&d.id) {
            continue;
        }
        let missing = d
            .source_path
            .as_deref()
            .is_some_and(|p| !Path::new(p).exists());
        let gone = if in_folder(d) {
            req.prune && (d.rel_path.is_some() || missing)
        } else {
            missing
        };
        if gone && db.rag_delete_document(&d.id).is_ok() {
            out.removed += 1;
        }
    }
    Ok(out)
}

/// The files and settings of a run for a base: its folder (or `folder`) is scanned, `paths` are
/// added as they are; media files are taken when the base has the model that reads them.
pub fn build_request(
    db: &StateDb,
    kb_id: &str,
    folder: Option<&str>,
    paths: Vec<String>,
) -> Result<KbIndexRequest, String> {
    let kb = db.kb_get(kb_id).map_err(String::from)?;
    let folder = folder
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .or(kb.folder.clone())
        .map(PathBuf::from);
    let media = MediaScan {
        audio_video: kb.asr_model.is_some(),
        images: kb.ocr_model.is_some(),
    };
    let mut req = KbIndexRequest {
        kb_id: kb_id.to_string(),
        ..Default::default()
    };
    if let Some(f) = folder.filter(|f| f.is_dir()) {
        let scan = scan_folder_media(&f, media)?;
        req.prune = !scan.truncated;
        req.files = scan.files;
        req.root = Some(f);
    }
    req.files.extend(paths.into_iter().map(PathBuf::from));
    Ok(req)
}

/// Adds sources to a base: the first folder becomes the base's folder (its files are indexed
/// with relative paths); another folder, and single files, are added file by file. The files
/// are indexed by the next run.
pub fn add_sources(
    db: &StateDb,
    kb_id: &str,
    folder: Option<&str>,
    paths: &[String],
) -> Result<Vec<String>, String> {
    let kb = db.kb_get(kb_id).map_err(String::from)?;
    let mut extra: Vec<String> = paths.to_vec();
    if let Some(f) = folder.filter(|f| !f.is_empty()) {
        if !Path::new(f).is_dir() {
            return Err(err("rag.kb_not_a_folder").with("path", f).into());
        }
        if kb.folder.is_none() {
            db.kb_update(
                kb_id,
                &crate::state::KbUpdate {
                    folder: Some(f.to_string()),
                    ..Default::default()
                },
            )
            .map_err(String::from)?;
        } else if kb.folder.as_deref() != Some(f) {
            let media = MediaScan {
                audio_video: kb.asr_model.is_some(),
                images: kb.ocr_model.is_some(),
            };
            let scan = scan_folder_media(Path::new(f), media)?;
            extra.extend(scan.files.iter().map(|p| p.to_string_lossy().to_string()));
        }
    }
    Ok(extra)
}

/// Removes documents from a base by id, name (path below the folder) or source path. Returns
/// how many were removed.
pub fn remove_sources(db: &StateDb, kb_id: &str, which: &[String]) -> Result<usize, String> {
    db.kb_get(kb_id).map_err(String::from)?;
    let docs = db.rag_documents(kb_id).map_err(String::from)?;
    let mut removed = 0;
    for d in docs {
        let hit = which.iter().any(|w| {
            *w == d.id
                || *w == d.name
                || d.rel_path.as_deref() == Some(w.as_str())
                || d.source_path.as_deref() == Some(w.as_str())
        });
        if hit && !jobs().is_running(&d.id) && db.rag_delete_document(&d.id).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
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

/// Whether a stored document still has to be embedded with `embed`.
fn needs_embedding(d: &RagDocument, embed: &EmbedModel) -> bool {
    d.chunks > 0
        && d.status != "failed"
        && (d.embed_model.as_deref() != Some(embed.alias.as_str()) || d.embedded < d.chunks)
}

/// Runs the indexing of a knowledge base to its end. The run must have been registered with
/// `kb_jobs().register(kb_id)`; `cancel` is its flag (all documents stop with it). One broken
/// document does not stop the run (its status says `failed`). The stages run apart: first the
/// sources are compared with the index, then every file that needs it is read (text, OCR,
/// transcript), then all chunks are embedded, so the models are never swapped back and forth.
/// Measures the wall time and the resources, stores them in the base together with the
/// embedding model and vector size, and logs one line.
pub async fn index_kb(
    db: Arc<StateDb>,
    req: KbIndexRequest,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(KbProgress) + Send + Sync,
) -> Result<KbRun, String> {
    let started = Instant::now();
    let report = |p: KbProgress| {
        last_progress()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(p.kb_id.clone(), p.clone());
        on_progress(p);
    };
    let result = index_kb_inner(&db, &req, &cancel, &report, started).await;
    kb_jobs().finish(&req.kb_id, &cancel);
    if let Err(e) = &result {
        report(KbProgress {
            kb_id: req.kb_id.clone(),
            status: "failed".into(),
            stage: String::new(),
            docs_done: 0,
            docs_total: 0,
            current: Some(e.clone()),
            chunk_done: 0,
            chunk_total: 0,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }
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
    let started_at = crate::analytics::now_ms() as i64;
    let meter = super::usage::Meter::start();
    let stage = Stage(std::sync::atomic::AtomicUsize::new(0));
    let emit =
        |status: &str, done: usize, total: usize, cur: Option<&str>, cd: usize, ct: usize| {
            on_progress(KbProgress {
                kb_id: req.kb_id.clone(),
                status: status.to_string(),
                stage: stage.get().to_string(),
                docs_done: done,
                docs_total: total,
                current: cur.map(String::from),
                chunk_done: cd,
                chunk_total: ct,
                elapsed_ms: started.elapsed().as_millis() as u64,
            })
        };
    emit("indexing", 0, 0, None, 0, 0);

    // 1. What changed: new files become pending documents, changed files are marked, documents
    //    of vanished files are dropped.
    let synced = {
        let (db, req) = (db.clone(), req.clone());
        tokio::task::spawn_blocking(move || sync_sources(&db, &req))
            .await
            .map_err(|e| e.to_string())??
    };

    // 2. The documents that need work.
    let docs = db.rag_documents(&req.kb_id).map_err(String::from)?;
    let work: Vec<RagDocument> = docs
        .into_iter()
        .filter(|d| !jobs().is_running(&d.id) && needs_work(d, req.embed.as_ref(), req.rebuild))
        .collect();
    // Stage A reads what has no (or outdated) text; without an embedding model it also
    // finishes documents that only wait for their status.
    let read_list: Vec<RagDocument> = work
        .iter()
        .filter(|d| {
            req.embed.is_none() || req.rebuild || d.chunks == 0 || synced.reextract.contains(&d.id)
        })
        .cloned()
        .collect();
    let mut total = read_list.len();
    emit("indexing", 0, total, None, 0, 0);

    let mut failed = 0;
    let mut done = 0;
    let mut cancelled = false;
    let mut touched: HashSet<String> = HashSet::new();
    // Documents that went through every stage they need (the progress counts these).
    let mut finished: HashSet<String> = HashSet::new();

    // 3a. Read: one document after the other (text, OCR, transcripts; low memory).
    stage.set("extract");
    let extract_started = Instant::now();
    for d in &read_list {
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        // A rebuild or a changed file reads the file again; a resume keeps the stored chunks.
        let source = (req.rebuild || d.chunks == 0 || synced.reextract.contains(&d.id))
            .then(|| resolve_source(kb.folder.as_deref(), d))
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
                // Embedding is the next stage.
                embed: None,
                ocr: req.ocr.clone(),
                asr: req.asr.clone(),
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
        touched.insert(d.id.clone());
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        if outcome.is_err() {
            failed += 1;
        }
        // With an embedding model the document is not done before its vectors are.
        if outcome.is_err() || req.embed.is_none() {
            finished.insert(d.id.clone());
        }
        done = finished.len();
    }
    let extract_ms = extract_started.elapsed().as_millis() as u64;

    // 3b. Embed: every document that still lacks vectors of the model in use.
    let mut embed_ms = 0;
    if let (Some(embed), false) = (req.embed.as_ref(), cancelled) {
        stage.set("embed");
        let embed_started = Instant::now();
        let embed_list: Vec<RagDocument> = db
            .rag_documents(&req.kb_id)
            .map_err(String::from)?
            .into_iter()
            .filter(|d| !jobs().is_running(&d.id) && needs_embedding(d, embed))
            .collect();
        total = read_list
            .iter()
            .map(|d| &d.id)
            .chain(embed_list.iter().map(|d| &d.id))
            .collect::<HashSet<_>>()
            .len();
        emit("indexing", done, total, None, 0, 0);
        for d in &embed_list {
            if cancel.load(Ordering::SeqCst) {
                cancelled = true;
                break;
            }
            jobs().register_shared(&d.id, cancel.clone());
            let state = Mutex::new((0usize, 0usize));
            let name = d.name.clone();
            let outcome = run_job(
                db.clone(),
                IndexJob {
                    document_id: d.id.clone(),
                    source: None,
                    embed: req.embed.clone(),
                    ocr: None,
                    asr: None,
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
            touched.insert(d.id.clone());
            if cancel.load(Ordering::SeqCst) {
                cancelled = true;
                break;
            }
            if outcome.is_err() {
                failed += 1;
            }
            finished.insert(d.id.clone());
            done = finished.len();
        }
        embed_ms = embed_started.elapsed().as_millis() as u64;
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

    // 4. Record the model, the size, the time and the resources.
    let usage = meter.stop();
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
    let _ = db.kb_record_run(
        &req.kb_id,
        RunStats {
            started_at,
            elapsed_ms,
            extract_ms,
            embed_ms,
            added: synced.added,
            changed: synced.changed,
            removed: synced.removed,
            unchanged: synced.unchanged,
            processed: touched.len(),
            failed,
            cpu_avg_pct: usage.cpu_avg_pct,
            cpu_peak_pct: usage.cpu_peak_pct,
            rss_peak_bytes: usage.rss_peak_bytes,
            model: model.clone(),
        },
    );
    eprintln!(
        "[rag] knowledge base \"{}\": {} added, {} changed, {} unchanged, {} removed; {} documents worked on ({} failed{}), {} chunks, {} embedded with {} ({} dims) in {} ms (read {} ms, embed {} ms)",
        kb.name,
        synced.added,
        synced.changed,
        synced.unchanged,
        synced.removed,
        touched.len(),
        failed,
        if cancelled { ", cancelled" } else { "" },
        chunks,
        embedded,
        model.as_deref().unwrap_or("keywords only"),
        dims.unwrap_or(0),
        elapsed_ms,
        extract_ms,
        embed_ms
    );
    stage.set("");
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
        processed: touched.len(),
        failed,
        cancelled,
        documents: docs.len(),
        chunks,
        embedded,
        elapsed_ms,
        model,
        dims,
        not_added: synced.not_added,
        added: synced.added,
        changed: synced.changed,
        unchanged: synced.unchanged,
        removed: synced.removed,
        extract_ms,
        embed_ms,
        cpu_avg_pct: usage.cpu_avg_pct,
        cpu_peak_pct: usage.cpu_peak_pct,
        rss_peak_bytes: usage.rss_peak_bytes,
    })
}

/// The answer to a question about one base: the excerpts with their citations and what the
/// question cost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct KbAnswer {
    pub kb_id: String,
    pub query: String,
    pub sources: Vec<super::Source>,
    /// `hybrid` (vectors and keywords) or `keyword`.
    pub retrieval: Option<String>,
    /// Rerank model that reordered the excerpts.
    pub reranked: Option<String>,
    pub notices: Vec<String>,
    /// Time of the whole question (embedding the question, searching, reranking).
    pub latency_ms: u64,
}

/// Asks a base: the best excerpts of its documents for `opts.query`, numbered for citation
/// (`[1]`...), with the page or the time span of each. Records the latency in the base's
/// figures. The base's rerank model is used unless `opts` already has one or `no_rerank` is set.
pub async fn ask(
    db: &StateDb,
    kb_id: &str,
    opts: &super::RetrieveOptions,
) -> Result<KbAnswer, String> {
    let started = Instant::now();
    let p = super::query_kb(db, kb_id, opts).await?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let _ = db.kb_record_query(kb_id, latency_ms);
    Ok(KbAnswer {
        kb_id: kb_id.to_string(),
        query: opts.query.clone(),
        sources: p.sources,
        retrieval: p.retrieval,
        reranked: p.reranked,
        notices: p.notices,
        latency_ms,
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
