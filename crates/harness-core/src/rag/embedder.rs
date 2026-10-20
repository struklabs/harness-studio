//! The recommended embedding model for knowledge bases, installed on demand.
//!
//! One small multilingual model (multilingual-e5-small, Q8_0 GGUF, 126 MB, 384 dimensions, 100
//! languages, MIT) is pinned by repository revision, size and SHA-256, like the CrispASR helper
//! models: it downloads through the normal download queue into the primary library folder,
//! then gets a default model definition that keeps it on the CPU (`gpu_layers = 0`) so
//! indexing never competes with a chat model for the GPU. The gateway serves it by alias
//! (`/v1/embeddings`, llama.cpp `--embeddings`), the same way as any other embedding model.

use crate::domain::store::Domain;
use crate::domain::{ModelDefinition, ModelKind};
use crate::downloader::{DownloadManager, PinnedFile};
use crate::params::ParamValue;
use serde::Serialize;
use std::path::PathBuf;

pub const ID: &str = "multilingual-e5-small";
pub const REPO: &str = "TwinSunsLLC/multilingual-e5-small-gguf";
pub const REV: &str = "b6cac9615d4ecce28d7f22539b7322d695fc2886";
pub const FILE: &str = "multilingual-e5-small-q8_0.gguf";
pub const SIZE: u64 = 132_439_008;
pub const SHA256: &str = "e011debc1208e31bf7b6aebee2d9fc8bd2ca11694a77ed66ac9d0c9d0a877c93";
pub const DIMS: u64 = 384;
/// Label of its download job.
pub const VARIANT: &str = "multilingual-e5-small Q8_0 (Embedding)";

/// What the Knowledge Base screen shows about the recommended embedding model.
#[derive(Debug, Clone, Serialize)]
pub struct EmbedderStatus {
    pub id: &'static str,
    pub repo: &'static str,
    pub file: &'static str,
    pub size_bytes: u64,
    pub dims: u64,
    /// The file is in a library folder at its full size.
    pub installed: bool,
    /// Definition alias the gateway serves it under (after install).
    pub alias: Option<String>,
    /// Download job id while it downloads or waits.
    pub download_id: Option<u64>,
    pub downloaded_bytes: u64,
    /// `queued | downloading | verifying | paused | failed | ...` of the job, if any.
    pub download_status: Option<String>,
}

fn pinned() -> PinnedFile<'static> {
    PinnedFile {
        repo: REPO,
        rev: REV,
        filename: FILE,
        size: SIZE,
        sha256: SHA256,
    }
}

/// Path of the file in a library folder.
pub fn path_in(library: &std::path::Path) -> PathBuf {
    library.join(REPO).join(FILE)
}

/// The installed file (full size) in any library folder.
pub fn installed_path() -> Option<PathBuf> {
    crate::config::AppSettings::load()
        .all_models_dirs()
        .iter()
        .map(|d| path_in(d))
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() == SIZE))
}

/// Queues the download (pinned revision, SHA-256 checked). An already queued job is reused.
pub fn start_download(manager: &DownloadManager) -> Result<u64, String> {
    if let Some(job) = manager
        .list()
        .into_iter()
        .find(|j| j.repo_id == REPO && j.variant_name == VARIANT && j.status.is_pending())
    {
        return Ok(job.id);
    }
    manager.start_pinned_download(
        &pinned(),
        VARIANT.to_string(),
        DownloadManager::models_dir().join(REPO),
    )
}

/// Keeps the model on the CPU unless the definition already says otherwise. Returns whether it
/// changed the definition.
pub fn apply_cpu_default(def: &mut ModelDefinition) -> bool {
    if def.load.contains_key("gpu_layers") {
        return false;
    }
    def.load.insert("gpu_layers".into(), ParamValue::Int(0));
    true
}

/// Makes (or finds) the definition of the installed file and pins it to the CPU. Returns its
/// alias, `None` when the file is not installed.
pub fn sync_definition() -> Result<Option<String>, String> {
    let Some(path) = installed_path() else {
        return Ok(None);
    };
    let domain = Domain::global();
    let find = |domain: &Domain| -> Result<Option<ModelDefinition>, String> {
        Ok(domain
            .definitions()?
            .into_iter()
            .find(|d| d.base.local_path() == Some(path.as_path())))
    };
    if find(&domain)?.is_none() {
        let models = crate::models::ModelManager.scan_models();
        crate::library_definitions::ensure(&models, &[ModelKind::Embedding])?;
    }
    let Some(mut def) = find(&domain)? else {
        return Ok(None);
    };
    if apply_cpu_default(&mut def) {
        def = domain.save_definition(def)?;
    }
    Ok(Some(def.alias))
}

/// Status for the UI (reads the queue and the library; cheap).
pub fn status(manager: &DownloadManager) -> EmbedderStatus {
    let installed = installed_path();
    let alias = installed.as_ref().and_then(|p| {
        Domain::global()
            .definitions()
            .ok()?
            .into_iter()
            .find(|d| d.base.local_path() == Some(p.as_path()))
            .map(|d| d.alias)
    });
    let job = manager
        .list()
        .into_iter()
        .filter(|j| j.repo_id == REPO && j.variant_name == VARIANT)
        .max_by_key(|j| j.id);
    EmbedderStatus {
        id: ID,
        repo: REPO,
        file: FILE,
        size_bytes: SIZE,
        dims: DIMS,
        installed: installed.is_some(),
        alias,
        download_id: job.as_ref().map(|j| j.id),
        downloaded_bytes: job.as_ref().map_or(0, |j| j.downloaded_bytes),
        download_status: job
            .as_ref()
            .map(|j| format!("{:?}", j.status).to_lowercase()),
    }
}

/// Deletes the model file (its definition stays and shows as missing until removed).
pub fn remove() -> Result<(), String> {
    for dir in crate::config::AppSettings::load().all_models_dirs() {
        let path = path_in(&dir);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ModelRef;

    #[test]
    fn pin_is_well_formed() {
        assert_eq!(REV.len(), 40);
        assert!(REV.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(SHA256.len(), 64);
        // At most 300 MB, as the component budget says.
        const { assert!(SIZE <= 300 * 1024 * 1024) };
        assert_eq!(
            path_in(std::path::Path::new("lib")),
            std::path::Path::new("lib").join(REPO).join(FILE)
        );
    }

    #[test]
    fn definition_stays_on_the_cpu_unless_chosen_otherwise() {
        let mut def = ModelDefinition::new(
            "e5",
            ModelRef::Local {
                path: PathBuf::from("x.gguf"),
            },
        );
        assert!(apply_cpu_default(&mut def));
        assert_eq!(def.load.get("gpu_layers"), Some(&ParamValue::Int(0)));
        // A second call and a user's own choice are kept.
        assert!(!apply_cpu_default(&mut def));
        def.load.insert("gpu_layers".into(), ParamValue::Int(99));
        assert!(!apply_cpu_default(&mut def));
        assert_eq!(def.load.get("gpu_layers"), Some(&ParamValue::Int(99)));
    }
}
