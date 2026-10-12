//! Library file operations: reveal in the file manager, delete (with a preview of what is
//! affected), repair missing parts of a split model, the in-use check shared by delete and
//! move, and recording the last use when a server starts.

use super::{norm, RepairInfo};
use crate::domain::{ModelDefinition, ModelOrigin, ServerProfile};
use crate::error_key::err;
use crate::models::LocalModel;
use crate::params::ParamValue;
use crate::process::{LaunchConfig, ServerStatus};
use crate::servers::ServerRegistry;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// --- reveal ---

/// Opens the system file manager at `path`: Windows `explorer /select,<path>`, macOS
/// `open -R <path>`, Linux `xdg-open <folder>` (no portable "select" there).
pub fn reveal_in_folder(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(err("fs.not_found").with("path", path.display()).into());
    }
    let mut child = spawn_file_manager(path)
        .map_err(|e| String::from(err("library.file_manager_failed").with("error", e)))?;
    // Reap the launcher so it does not linger as a zombie process.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(windows)]
fn spawn_file_manager(path: &Path) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    // Explorer wants `/select,"<path>"` verbatim (Windows paths cannot contain `"`); it
    // exits with 1 even on success, so only the spawn result counts.
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
}

#[cfg(target_os = "macos")]
fn spawn_file_manager(path: &Path) -> std::io::Result<std::process::Child> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_file_manager(path: &Path) -> std::io::Result<std::process::Child> {
    std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn()
}

// --- in use ---

/// Every file a launch reads: model, draft, projector, LoRA and path-like catalog values.
fn launch_files(c: &LaunchConfig) -> Vec<PathBuf> {
    let mut out = vec![c.model_path.clone()];
    out.extend(c.draft_model_path.clone());
    out.extend(c.mmproj_path.clone());
    out.extend(c.lora_path.clone());
    for v in c.params.values() {
        match v {
            ParamValue::Text(s) => out.push(PathBuf::from(s)),
            ParamValue::List(l) => out.extend(l.iter().map(PathBuf::from)),
            _ => {}
        }
    }
    out
}

/// Servers (starting or running) that use any of `files`: as model, draft, projector,
/// adapter, or through a router serving their folder.
pub fn servers_using(registry: &ServerRegistry, files: &[PathBuf]) -> Vec<String> {
    let keys: HashSet<String> = files.iter().map(|p| norm(p)).collect();
    registry
        .all()
        .into_iter()
        .filter(|(_, pm)| {
            matches!(
                pm.get_status(),
                ServerStatus::Starting | ServerStatus::Running
            )
        })
        .filter_map(|(id, pm)| {
            let c = pm.launch_config()?;
            let direct = launch_files(&c).iter().any(|f| keys.contains(&norm(f)));
            let routed = c.router_mode
                && c.models_path.as_ref().is_some_and(|dir| {
                    let d = format!("{}/", norm(dir).trim_end_matches('/'));
                    keys.iter().any(|k| k.starts_with(&d))
                });
            (direct || routed).then_some(id)
        })
        .collect()
}

// --- delete ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UseRole {
    Base,
    Draft,
    Mmproj,
    Adapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionUse {
    pub id: String,
    pub alias: String,
    pub role: UseRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// What deleting a model touches; shown before the user confirms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeletePreview {
    pub model_id: String,
    pub name: String,
    /// Linked from Ollama / LM Studio: only the link can be removed, never the file.
    pub linked: bool,
    pub files: Vec<FileEntry>,
    pub definitions: Vec<DefinitionUse>,
    /// Profiles serving one of those definitions.
    pub profiles: Vec<crate::domain::store::ProfileRef>,
    /// Servers using the files right now (deletion is refused while non-empty).
    pub running_on: Vec<String>,
}

/// Definitions that reference any of the model's files, and how.
pub fn definitions_using(model: &LocalModel, defs: &[ModelDefinition]) -> Vec<DefinitionUse> {
    let files: HashSet<String> = model.shard_paths.iter().map(|p| norm(p)).collect();
    let hit = |p: &Path| files.contains(&norm(p));
    let mut out = Vec::new();
    for d in defs {
        let role = if d.base.local_path().is_some_and(hit) {
            Some(UseRole::Base)
        } else if d.draft.as_ref().is_some_and(|x| hit(&x.model)) {
            Some(UseRole::Draft)
        } else if d.mmproj.as_deref().is_some_and(hit) {
            Some(UseRole::Mmproj)
        } else if d.adapters.iter().any(|a| hit(&a.path)) {
            Some(UseRole::Adapter)
        } else {
            None
        };
        if let Some(role) = role {
            out.push(DefinitionUse {
                id: d.id.clone(),
                alias: d.alias.clone(),
                role,
            });
        }
    }
    out
}

pub fn delete_preview(
    model: &LocalModel,
    defs: &[ModelDefinition],
    profiles: &[ServerProfile],
    running_on: Vec<String>,
) -> DeletePreview {
    let definitions = definitions_using(model, defs);
    let ids: HashSet<&str> = definitions.iter().map(|d| d.id.as_str()).collect();
    DeletePreview {
        model_id: model.id.clone(),
        name: model.name.clone(),
        linked: model.origin == ModelOrigin::Linked || model.source.is_some(),
        files: model
            .shard_paths
            .iter()
            .map(|p| FileEntry {
                size_bytes: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
                path: p.clone(),
            })
            .collect(),
        profiles: profiles
            .iter()
            .filter(|p| p.id != crate::servers::DEFAULT_SERVER_ID)
            .filter(|p| p.definition_id.as_deref().is_some_and(|d| ids.contains(d)))
            .map(|p| crate::domain::store::ProfileRef {
                id: p.id.clone(),
                name: p.name.clone(),
            })
            .collect(),
        definitions,
        running_on,
    }
}

/// Deletes a library model's files (every part). Refuses linked models and any file outside
/// `roots` (the library directories); checks everything before removing anything.
pub fn delete_files(model: &LocalModel, roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    if model.source.is_some() || model.origin == ModelOrigin::Linked {
        return Err(err("library.linked_not_deletable").into());
    }
    for shard in &model.shard_paths {
        if !roots.iter().any(|r| crate::paths::is_within(shard, r)) {
            return Err(err("library.outside_library")
                .with("path", shard.display())
                .into());
        }
    }
    // A text-to-speech model is a folder (its phoneme data included): the folder goes.
    if model.kind == crate::domain::ModelKind::Tts {
        crate::tts::delete_model_dir(&model.path, roots)?;
        return Ok(model.shard_paths.clone());
    }
    // So is a sherpa-onnx speech-recognition model.
    if crate::model_registry::is_sherpa_asr(model) {
        crate::asr::delete_model_dir(&model.path, roots)?;
        return Ok(model.shard_paths.clone());
    }
    // And a safetensors checkpoint (config, tokenizer and weights of one repo).
    if crate::engine::is_mistralrs_library_model(model) {
        crate::engine::mistralrs_delete_model_dir(&model.path, roots)?;
        return Ok(vec![model.path.clone()]);
    }
    let mut removed = Vec::new();
    for shard in &model.shard_paths {
        if shard.exists() {
            std::fs::remove_file(shard).map_err(|e| {
                String::from(
                    err("fs.remove_failed")
                        .with("path", shard.display())
                        .with("error", e),
                )
            })?;
            removed.push(shard.clone());
        }
    }
    Ok(removed)
}

/// Outcome of [`delete_model`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DeleteReport {
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<String>))]
    pub files: Vec<PathBuf>,
    /// Definitions based on the file that were deleted with it (on request).
    pub definitions_deleted: Vec<String>,
    /// Definitions that could not be deleted (e.g. a profile serves them).
    pub definition_errors: Vec<String>,
}

/// Deletes the files (see [`delete_files`]), optionally the definitions based on them, and
/// forgets the model's library state. The caller checks [`servers_using`] first and holds
/// the model's write lease.
pub fn delete_model(
    model: &LocalModel,
    roots: &[PathBuf],
    domain: &crate::domain::store::Domain,
    delete_definitions: bool,
    db: Option<&crate::state::StateDb>,
) -> Result<DeleteReport, String> {
    let files = delete_files(model, roots)?;
    let mut report = DeleteReport {
        files,
        ..Default::default()
    };
    if delete_definitions {
        let defs = domain.definitions()?;
        for u in definitions_using(model, &defs)
            .into_iter()
            .filter(|u| u.role == UseRole::Base)
        {
            match domain.delete_definition(&u.id) {
                Ok(()) => report.definitions_deleted.push(u.alias),
                Err(e) => report.definition_errors.push(
                    err("library.definition_delete_failed")
                        .with("alias", &u.alias)
                        .with("error", e)
                        .into(),
                ),
            }
        }
    }
    if let Some(db) = db {
        if let Err(e) = db.forget_model_state(&super::state_key(&model.path)) {
            eprintln!("[Harness Studio] Kütüphane durumu silinemedi: {}", e);
        }
    }
    Ok(report)
}

// --- repair ---

/// The library directory that contains `path`.
fn root_of<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a PathBuf> {
    roots
        .iter()
        .filter(|r| crate::paths::is_within(path, r))
        .max_by_key(|r| r.as_os_str().len())
}

/// Hugging Face repo a library file came from, read from its folder: the downloader stores
/// files as `<library>/<owner>/<repo>/<path in repo>` (LM Studio uses the same layout, so a
/// linked LM Studio folder yields one too when it is a library root). `None` elsewhere.
pub fn repo_of(model: &LocalModel, roots: &[PathBuf]) -> Option<String> {
    let root = root_of(&model.path, roots)?;
    let rel = crate::paths::normalize(&model.path);
    let rel = rel
        .strip_prefix(crate::paths::normalize(root))
        .ok()?
        .to_path_buf();
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    // A file sits inside `<owner>/<repo>/`; a model folder (mistral.rs) is that folder itself.
    let min = if model.path.is_dir() { 2 } else { 3 };
    if parts.len() < min {
        return None;
    }
    let repo_id = format!("{}/{}", parts[0], parts[1]);
    crate::downloader::DownloadManager::validate_repo_id(&repo_id).ok()?;
    Some(repo_id)
}

/// Whether and where the missing parts of a split model can be downloaded again.
///
/// Sources, in this order: a recorded custom-URL download (the address of a part that is
/// still there), the Hugging Face repo folder the downloader uses
/// (`<library>/<owner>/<repo>/<path in repo>`), a recorded Hugging Face address of a file that
/// was moved out of its repo folder, then a Hugging Face repo URL embedded in GGUF metadata.
/// Without any of them the reason is explained and, for a file inside a library folder, the
/// user may give the address of one part (`url_allowed`).
pub fn repair_info(model: &LocalModel, roots: &[PathBuf]) -> RepairInfo {
    let records = super::sources::all();
    repair_info_with(model, roots, &|p| {
        records.get(&super::state_key(p)).cloned()
    })
}

/// [`repair_info`] over given source records.
pub fn repair_info_with(
    model: &LocalModel,
    roots: &[PathBuf],
    source_of: &dyn Fn(&Path) -> Option<super::sources::ModelSource>,
) -> RepairInfo {
    let fail = |key: &'static str, url_allowed: bool| RepairInfo {
        reason: Some(err(key).into()),
        url_allowed,
        ..Default::default()
    };
    if model.origin == ModelOrigin::Linked || model.source.is_some() {
        return fail("library.repair_linked", false);
    }
    let Some(root) = root_of(&model.path, roots) else {
        return fail("library.repair_outside_library", false);
    };
    let recorded = model.shard_paths.iter().find_map(|p| source_of(p));
    let by_url = |source: &super::sources::ModelSource| RepairInfo {
        repo_id: None,
        target_dir: model.path.parent().map(Path::to_path_buf),
        reason: None,
        source_url: Some(
            super::sources::fresh_hf_resolve_url(&source.url).unwrap_or_else(|| source.url.clone()),
        ),
        url_allowed: true,
    };
    // A custom-URL download is repaired from its address even when its folder looks like a repo.
    if let Some(source) = recorded.as_ref().filter(|s| s.custom) {
        return by_url(source);
    }
    let rel = crate::paths::normalize(&model.path);
    let rel = rel
        .strip_prefix(crate::paths::normalize(root))
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    if parts.len() >= 3 {
        let repo_id = format!("{}/{}", parts[0], parts[1]);
        if crate::downloader::DownloadManager::validate_repo_id(&repo_id).is_ok() {
            return RepairInfo {
                target_dir: Some(root.join(&parts[0]).join(&parts[1])),
                repo_id: Some(repo_id),
                url_allowed: true,
                ..Default::default()
            };
        }
    }
    match recorded {
        Some(source) => by_url(&source),
        None => {
            let metadata_urls = model.gguf.as_ref().into_iter().flat_map(|g| {
                [
                    g.repo_url.as_deref(),
                    g.url.as_deref(),
                    g.source_repo_url.as_deref(),
                    g.source_url.as_deref(),
                ]
                .into_iter()
                .flatten()
            });
            for url in metadata_urls {
                if let Some(fresh) = super::sources::fresh_hf_resolve_url(url) {
                    return RepairInfo {
                        target_dir: model.path.parent().map(Path::to_path_buf),
                        source_url: Some(fresh),
                        url_allowed: true,
                        ..Default::default()
                    };
                }
                if let Some(repo_id) = crate::downloader::repo_from_hf_url(url) {
                    return RepairInfo {
                        repo_id: Some(repo_id),
                        target_dir: model.path.parent().map(Path::to_path_buf),
                        url_allowed: true,
                        ..Default::default()
                    };
                }
            }
            fail("library.repair_no_source", true)
        }
    }
}

/// Path of `file` inside its repo folder, with `/` separators.
fn repo_path(file: &Path, target_dir: &Path) -> Option<String> {
    let rel = crate::paths::normalize(file);
    let rel = rel.strip_prefix(crate::paths::normalize(target_dir)).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// From the repo's split variants, the one this model belongs to, reduced to the missing
/// parts. `missing` holds file names (see `ShardStatus::missing`).
pub fn missing_parts_variant(
    model: &LocalModel,
    target_dir: &Path,
    variants: &[crate::downloader::HfModelVariant],
    missing: &[String],
) -> Result<crate::downloader::HfModelVariant, String> {
    let present: HashSet<String> = model
        .shard_paths
        .iter()
        .filter_map(|p| repo_path(p, target_dir))
        .map(|p| p.to_lowercase())
        .collect();
    let variant = variants
        .iter()
        .filter(|v| v.is_split)
        .find(|v| {
            v.shards
                .iter()
                .any(|s| present.contains(&s.filename.to_lowercase()))
        })
        .ok_or_else(|| String::from(err("library.repair_no_matching_variant")))?;
    let wanted: HashSet<String> = missing.iter().map(|m| m.to_lowercase()).collect();
    let mut subset = variant.clone();
    subset.shards.retain(|s| {
        let name = s.filename.rsplit('/').next().unwrap_or(&s.filename);
        wanted.contains(&name.to_lowercase())
    });
    if subset.shards.is_empty() {
        return Err(err("library.repair_parts_not_in_repo").into());
    }
    subset.total_size_bytes = subset.shards.iter().map(|s| s.size_bytes).sum();
    Ok(subset)
}

/// Queues the missing parts of a split model into the folder the model lives in. Returns the
/// download job id.
///
/// `url`: the address of one part of the set (any part), for files without a record; when given
/// it is used even where a repo folder or a record exists. Otherwise a Hugging Face repo folder
/// is repaired from the repo's file list and anything else from the recorded address.
pub async fn repair(
    downloads: &crate::downloader::DownloadManager,
    model: &LocalModel,
    roots: &[PathBuf],
    token: Option<&str>,
    url: Option<&str>,
) -> Result<u64, String> {
    if !model.is_split || model.is_ready {
        return Err(err("library.repair_nothing_missing").into());
    }
    let info = repair_info(model, roots);
    let url = url.map(str::trim).filter(|u| !u.is_empty());
    let missing = super::shard_status(model).missing;
    if url.is_none() {
        if let (Some(repo_id), Some(target_dir)) = (&info.repo_id, &info.target_dir) {
            let variants =
                crate::downloader::DownloadManager::get_model_variants(repo_id, token).await?;
            let subset = missing_parts_variant(model, target_dir, &variants, &missing)?;
            return downloads.start_variant_download_to(
                repo_id.clone(),
                subset,
                target_dir.clone(),
            );
        }
    }
    if !info.url_allowed {
        return Err(info.reason.unwrap_or_default());
    }
    let base = url
        .map(str::to_string)
        .or(info.source_url.clone())
        .ok_or_else(|| info.reason.clone().unwrap_or_default())?;
    // The parts sit next to the ones that are there, whatever the folder layout.
    let target_dir = model
        .path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| String::from(err("library.repair_outside_library")))?;
    let parts = missing
        .iter()
        .map(|name| Ok((super::sources::part_url(&base, name)?, name.clone())))
        .collect::<Result<Vec<_>, String>>()?;
    downloads
        .start_custom_url_parts(parts, format!("{} (onarım)", model.name), target_dir)
        .await
}

// --- last use ---

/// Records that a server was started with `config` (model file, definition and that
/// definition's default preset) in `state.db`. Best effort: failures are only logged.
pub fn record_launch(config: &LaunchConfig) {
    if config.router_mode || config.model_path.as_os_str().is_empty() {
        return;
    }
    let db = match crate::state::shared() {
        Ok(db) => db,
        Err(e) => {
            eprintln!("[Harness Studio] Son kullanım kaydedilemedi: {}", e);
            return;
        }
    };
    let preset = config.definition_id.as_deref().and_then(|id| {
        crate::domain::store::Domain::global()
            .definition(id)
            .ok()
            .and_then(|d| d.default_preset_id)
    });
    if let Err(e) = db.record_model_use(
        &super::state_key(&config.model_path),
        config.definition_id.as_deref(),
        preset.as_deref(),
    ) {
        eprintln!("[Harness Studio] Son kullanım kaydedilemedi: {}", e);
    }
}
