//! Import models from Ollama and LM Studio.
//!
//! Ollama stores GGUF weights as content-addressed blobs (`blobs/sha256-<hex>`)
//! described by manifests; LM Studio keeps plain `.gguf` files in
//! `<publisher>/<repo>/`. Models can be *linked* (referenced in place — the
//! other app's files are never modified or deleted) or *copied* into the
//! primary library (hard link when possible, so no extra disk space).

use crate::config::AppSettings;
use crate::error_key::err;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    Ollama,
    Lmstudio,
}

impl ImportSource {
    fn label(&self) -> &'static str {
        match self {
            ImportSource::Ollama => "ollama",
            ImportSource::Lmstudio => "lmstudio",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCandidate {
    pub source: ImportSource,
    /// Display name: `llama3.1:8b` (Ollama) or `publisher/repo/file` (LM Studio).
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    /// Vision projector belonging to this model, if any.
    pub mmproj_path: Option<PathBuf>,
    pub architecture: Option<String>,
    pub quantization: Option<String>,
    /// Already linked or copied.
    pub imported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImportScan {
    pub ollama_dir: Option<PathBuf>,
    pub lmstudio_dir: Option<PathBuf>,
    pub candidates: Vec<ImportCandidate>,
}

/// A model referenced in place (not stored in a library directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalModel {
    pub name: String,
    pub path: PathBuf,
    pub source: ImportSource,
    #[serde(default)]
    pub mmproj_path: Option<PathBuf>,
    #[serde(default)]
    pub added_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImportMode {
    Link,
    Copy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub name: String,
    pub ok: bool,
    /// "linked" | "hardlinked" | "copied" | error message
    pub detail: String,
    /// Model id (launch path) in the library after import.
    pub model_id: Option<String>,
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

pub(crate) fn is_gguf(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|_| &magic == b"GGUF")
        .unwrap_or(false)
}

pub fn ollama_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("OLLAMA_MODELS") {
        candidates.push(PathBuf::from(dir));
    }
    if let Some(h) = home() {
        candidates.push(h.join(".ollama").join("models"));
    }
    if cfg!(target_os = "linux") {
        candidates.push(PathBuf::from("/usr/share/ollama/.ollama/models"));
    }
    candidates
        .into_iter()
        .find(|d| d.join("manifests").is_dir())
}

pub fn lmstudio_dir() -> Option<PathBuf> {
    let home = home()?;
    let from_settings = fs::read_to_string(home.join(".lmstudio").join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("downloadsFolder")
                .and_then(|d| d.as_str())
                .map(PathBuf::from)
        });
    [
        from_settings,
        Some(home.join(".lmstudio").join("models")),
        Some(home.join(".cache").join("lm-studio").join("models")),
    ]
    .into_iter()
    .flatten()
    .find(|d| d.is_dir())
}

/// `registry.ollama.ai/library/llama3.1/8b` → `llama3.1:8b`;
/// other registries/namespaces keep their prefix (`hf.co/user/repo:Q4_K_M`).
pub fn ollama_model_name(rel: &[String]) -> Option<String> {
    let (tag, rest) = rel.split_last()?;
    let (model, prefix) = rest.split_last()?;
    let prefix = match prefix {
        [registry, ns] if registry == "registry.ollama.ai" && ns == "library" => String::new(),
        [registry, rest @ ..] if registry == "registry.ollama.ai" => format!("{}/", rest.join("/")),
        all => format!("{}/", all.join("/")),
    };
    Some(format!("{}{}:{}", prefix, model, tag))
}

pub fn parse_ollama_digest(digest: &str) -> Option<&str> {
    let hex = digest
        .strip_prefix("sha256:")
        .or_else(|| digest.strip_prefix("sha256-"))?;
    if hex.len() == 64
        && hex
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    {
        Some(hex)
    } else {
        None
    }
}

pub fn blob_path(models_dir: &Path, digest: &str) -> Option<PathBuf> {
    let hex = parse_ollama_digest(digest)?;
    let blobs_dir = models_dir.join("blobs");
    let candidate = blobs_dir.join(format!("sha256-{}", hex));
    if crate::paths::is_within(&candidate, &blobs_dir) {
        Some(candidate)
    } else {
        None
    }
}

fn scan_ollama(dir: &Path) -> Vec<ImportCandidate> {
    let manifests = dir.join("manifests");
    let mut files = Vec::new();
    collect_files(&manifests, &mut files, 6);
    let mut out = Vec::new();
    for manifest in files {
        let Ok(rel) = manifest.strip_prefix(&manifests) else {
            continue;
        };
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        let Some(name) = ollama_model_name(&parts) else {
            continue;
        };
        let Some(json) = fs::read_to_string(&manifest)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        else {
            continue;
        };
        let layer = |media: &str| {
            json.get("layers")?
                .as_array()?
                .iter()
                .find(|l| l.get("mediaType").and_then(|m| m.as_str()) == Some(media))
                .and_then(|l| l.get("digest")?.as_str())
                .and_then(|d| blob_path(dir, d))
        };
        let Some(model_path) = layer("application/vnd.ollama.image.model") else {
            continue;
        };
        if !model_path.is_file() || !is_gguf(&model_path) {
            continue; // e.g. safetensors-based models of the newer engine
        }
        let mmproj_path =
            layer("application/vnd.ollama.image.projector").filter(|p| p.is_file() && is_gguf(p));
        out.push(candidate(
            ImportSource::Ollama,
            name,
            model_path,
            mmproj_path,
        ));
    }
    out
}

fn scan_lmstudio(dir: &Path) -> Vec<ImportCandidate> {
    let mut files = Vec::new();
    collect_files(dir, &mut files, 4);
    let ggufs: Vec<PathBuf> = files
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("gguf"))
                .unwrap_or(false)
        })
        .collect();
    let is_mmproj = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_lowercase().contains("mmproj"))
            .unwrap_or(false)
    };
    let split = regex::Regex::new(r"(?i)-(\d{5})-of-\d{5}\.gguf$").unwrap();
    let mut out = Vec::new();
    for path in ggufs.iter().filter(|p| !is_mmproj(p)) {
        // Split models: only the first shard is a launch target.
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if let Some(caps) = split.captures(fname) {
            if &caps[1] != "00001" {
                continue;
            }
        }
        let mmproj = ggufs
            .iter()
            .find(|m| is_mmproj(m) && m.parent() == path.parent())
            .cloned();
        let name = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches(".gguf")
            .to_string();
        out.push(candidate(
            ImportSource::Lmstudio,
            name,
            path.clone(),
            mmproj,
        ));
    }
    out
}

fn candidate(
    source: ImportSource,
    name: String,
    path: PathBuf,
    mmproj_path: Option<PathBuf>,
) -> ImportCandidate {
    let meta = crate::gguf::read_gguf_metadata(&path).ok();
    ImportCandidate {
        source,
        name,
        size_bytes: fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        mmproj_path,
        architecture: meta.as_ref().and_then(|m| m.architecture.clone()),
        quantization: meta.map(|m| m.quantization),
        path,
        imported: false,
    }
}

pub fn collect_files(dir: &Path, out: &mut Vec<PathBuf>, max_depth: usize) {
    const MAX_ALLOWED_DEPTH: usize = 8;
    let depth_limit = max_depth.min(MAX_ALLOWED_DEPTH);
    if depth_limit == 0 {
        return;
    }
    let mut visited = std::collections::HashSet::new();
    if let Ok(c) = fs::canonicalize(dir) {
        visited.insert(c);
    }
    let mut stack = vec![(dir.to_path_buf(), 0usize)];

    while let Some((current_dir, current_depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let Ok(meta) = fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                // Do not follow directory symlinks/junctions
                if fs::metadata(&p).map(|m| m.is_file()).unwrap_or(false) {
                    out.push(p);
                }
            } else if meta.is_dir() {
                let canon = fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
                if !visited.insert(canon) {
                    continue;
                }
                if current_depth < depth_limit {
                    stack.push((p, current_depth + 1));
                }
            } else {
                out.push(p);
            }
        }
    }
}

// --- external (linked) models registry ---

/// `external_models.json`: v1 was a bare array; v2 is `{schema_version, models: [...]}`.
pub const EXTERNAL_SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "external_models.json",
    current: 2,
    envelope: crate::store::Envelope::Wrapped("models"),
    migrations: &[external_v1_to_v2],
};

fn external_v1_to_v2(doc: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(crate::store::wrap_legacy(doc, "models"))
}

fn external_file() -> PathBuf {
    paths::config_dir().join("external_models.json")
}

pub fn load_external() -> Vec<ExternalModel> {
    crate::store::JsonStore::new(external_file())
        .load_versioned::<Vec<ExternalModel>>(&EXTERNAL_SCHEMA)
        .ok()
        .flatten()
        .map(|v| v.data)
        .unwrap_or_default()
}

fn save_external(list: &[ExternalModel]) -> Result<(), String> {
    crate::store::JsonStore::new(external_file()).save_versioned(&EXTERNAL_SCHEMA, list)
}

pub fn external_for(path: &Path) -> Option<ExternalModel> {
    load_external().into_iter().find(|e| e.path == path)
}

/// Forgets a linked model. The file itself (owned by Ollama / LM Studio) is untouched.
pub fn unlink_external(path: &Path) -> Result<(), String> {
    crate::store::JsonStore::new(external_file()).update_versioned::<Vec<ExternalModel>, _>(
        &EXTERNAL_SCHEMA,
        |list| {
            let before = list.len();
            list.retain(|e| e.path != path);
            if list.len() == before {
                Err(err("importer.not_linked")
                    .with("path", path.display())
                    .into())
            } else {
                Ok(())
            }
        },
    )
}

fn imported_dir(source: &ImportSource) -> PathBuf {
    AppSettings::load()
        .primary_models_dir()
        .join("imported")
        .join(source.label())
}

fn copy_target(c: &ImportCandidate) -> Option<PathBuf> {
    let file = format!("{}.gguf", c.name.replace([':', '/'], "-"));
    Some(imported_dir(&c.source).join(paths::sanitize_file_name(&file)?))
}

/// Installed Ollama models whose weights are GGUF (the ones a Modelfile `FROM <name>` can
/// use), sorted by name, and the models directory they were read from. Read-only.
pub fn scan_ollama_installed() -> (Option<PathBuf>, Vec<ImportCandidate>) {
    let dir = ollama_dir();
    let mut list = dir.as_deref().map(scan_ollama).unwrap_or_default();
    list.sort_by_key(|a| a.name.to_lowercase());
    (dir, list)
}

/// Scans both apps and marks what is already imported.
pub fn scan() -> ImportScan {
    let ollama = ollama_dir();
    let lmstudio = lmstudio_dir();
    let mut candidates = Vec::new();
    if let Some(d) = &ollama {
        candidates.extend(scan_ollama(d));
    }
    if let Some(d) = &lmstudio {
        candidates.extend(scan_lmstudio(d));
    }
    let external = load_external();
    let library = AppSettings::load().all_models_dirs();
    for c in &mut candidates {
        c.imported = external.iter().any(|e| e.path == c.path)
            || library.iter().any(|d| paths::is_within(&c.path, d))
            || copy_target(c).map(|t| t.exists()).unwrap_or(false);
    }
    candidates.sort_by_key(|a| a.name.to_lowercase());
    ImportScan {
        ollama_dir: ollama,
        lmstudio_dir: lmstudio,
        candidates,
    }
}

/// Hard link when on the same volume (instant, no extra space), else a full copy.
fn link_or_copy(src: &Path, dst: &Path) -> Result<&'static str, String> {
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir).map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })?;
    }
    if dst.exists() {
        return Err(err("importer.target_exists")
            .with("path", dst.display())
            .into());
    }
    if fs::hard_link(src, dst).is_ok() {
        return Ok("hardlinked");
    }
    let tmp = dst.with_extension("gguf.importing");
    fs::copy(src, &tmp).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        String::from(
            err("fs.copy_failed")
                .with("from", src.display())
                .with("to", tmp.display())
                .with("error", e),
        )
    })?;
    fs::rename(&tmp, dst).map_err(|e| {
        String::from(
            err("fs.rename_failed")
                .with("from", tmp.display())
                .with("to", dst.display())
                .with("error", e),
        )
    })?;
    Ok("copied")
}

/// Imports the given model paths. Paths must come from a fresh [`scan`] — anything
/// else is rejected, so the frontend can't make us link/copy arbitrary files.
pub fn import(paths_to_import: &[PathBuf], mode: ImportMode) -> Vec<ImportResult> {
    let scanned = scan().candidates;
    let mut external = load_external();
    let mut results = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for path in paths_to_import {
        let Some(c) = scanned.iter().find(|c| &c.path == path) else {
            results.push(ImportResult {
                name: path.display().to_string(),
                ok: false,
                detail: err("importer.not_in_scan").into(),
                model_id: None,
            });
            continue;
        };
        let result = match mode {
            ImportMode::Link => {
                if !external.iter().any(|e| e.path == c.path) {
                    external.push(ExternalModel {
                        name: c.name.clone(),
                        path: c.path.clone(),
                        source: c.source.clone(),
                        mmproj_path: c.mmproj_path.clone(),
                        added_at: now,
                    });
                }
                Ok(("linked", c.path.clone()))
            }
            ImportMode::Copy => match copy_target(c) {
                None => Err(err("importer.invalid_file_name")
                    .with("name", &c.name)
                    .into()),
                Some(target) => {
                    let mut created_files: Vec<PathBuf> = Vec::new();
                    let res: Result<(&'static str, PathBuf), String> = (|| {
                        let how = link_or_copy(&c.path, &target)?;
                        created_files.push(target.clone());

                        if let Some(mm) = &c.mmproj_path {
                            let name = target
                                .file_name()
                                .map(|n| format!("mmproj-{}", n.to_string_lossy()))
                                .unwrap_or_default();
                            let mm_target = target.with_file_name(name);
                            if !mm_target.exists() {
                                link_or_copy(mm, &mm_target)?;
                                created_files.push(mm_target);
                            }
                        }
                        Ok((how, target))
                    })();

                    match res {
                        Ok(val) => Ok(val),
                        Err(e) => {
                            for path in created_files {
                                if path.exists() {
                                    let _ = fs::remove_file(&path);
                                }
                            }
                            Err(e)
                        }
                    }
                }
            },
        };
        results.push(match result {
            Ok((how, id)) => ImportResult {
                name: c.name.clone(),
                ok: true,
                detail: how.to_string(),
                model_id: Some(id.to_string_lossy().to_string()),
            },
            Err(e) => ImportResult {
                name: c.name.clone(),
                ok: false,
                detail: e,
                model_id: None,
            },
        });
    }
    if mode == ImportMode::Link {
        if let Err(e) = save_external(&external) {
            for r in results.iter_mut().filter(|r| r.ok) {
                r.ok = false;
                r.detail = e.clone();
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(s: &str) -> Vec<String> {
        s.split('/').map(String::from).collect()
    }

    #[test]
    fn test_ollama_model_name() {
        assert_eq!(
            ollama_model_name(&parts("registry.ollama.ai/library/llama3.1/8b")).as_deref(),
            Some("llama3.1:8b")
        );
        assert_eq!(
            ollama_model_name(&parts("registry.ollama.ai/someone/model/latest")).as_deref(),
            Some("someone/model:latest")
        );
        assert_eq!(
            ollama_model_name(&parts("hf.co/bartowski/Qwen-GGUF/Q4_K_M")).as_deref(),
            Some("hf.co/bartowski/Qwen-GGUF:Q4_K_M")
        );
        assert_eq!(ollama_model_name(&parts("x")), None);
    }

    #[test]
    fn test_scan_fake_ollama_and_lmstudio_layouts() {
        let tmp = tempfile::tempdir().unwrap();
        // Ollama
        let o = tmp.path().join("ollama");
        let man = o.join("manifests/registry.ollama.ai/library/tiny/latest");
        fs::create_dir_all(man.parent().unwrap()).unwrap();
        fs::create_dir_all(o.join("blobs")).unwrap();
        let hash_a = "a".repeat(64);
        let hash_b = "b".repeat(64);
        fs::write(
            o.join(format!("blobs/sha256-{}", hash_a)),
            b"GGUF\x03\x00\x00\x00",
        )
        .unwrap();
        fs::write(o.join(format!("blobs/sha256-{}", hash_b)), b"not a gguf").unwrap();
        fs::write(
            &man,
            format!(
                r#"{{"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{}"}},
                          {{"mediaType":"application/vnd.ollama.image.projector","digest":"sha256:{}"}}]}}"#,
                hash_a, hash_b
            ),
        )
        .unwrap();
        let found = scan_ollama(&o);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "tiny:latest");
        assert!(found[0].path.ends_with(format!("blobs/sha256-{}", hash_a)));
        assert_eq!(found[0].mmproj_path, None, "non-GGUF projector ignored");

        // LM Studio
        let l = tmp.path().join("lms");
        let repo = l.join("pub/repo-GGUF");
        fs::create_dir_all(&repo).unwrap();
        for f in [
            "m-Q4_K_M.gguf",
            "mmproj-m-F16.gguf",
            "big-00001-of-00002.gguf",
            "big-00002-of-00002.gguf",
        ] {
            fs::write(repo.join(f), b"GGUF").unwrap();
        }
        let found = scan_lmstudio(&l);
        let names: Vec<_> = found.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(found.len(), 2, "{:?}", names);
        assert!(names.contains(&"pub/repo-GGUF/m-Q4_K_M"));
        assert!(found.iter().all(|c| c.mmproj_path.is_some()));
    }

    #[test]
    fn test_link_or_copy_hardlinks_on_same_volume() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.gguf");
        fs::write(&src, b"GGUF data").unwrap();
        let dst = tmp.path().join("sub/b.gguf");
        let how = link_or_copy(&src, &dst).unwrap();
        assert!(how == "hardlinked" || how == "copied");
        assert_eq!(fs::read(&dst).unwrap(), b"GGUF data");
        assert!(link_or_copy(&src, &dst).is_err(), "never overwrites");
    }

    /// Scans the real Ollama / LM Studio folders of this machine (read-only).
    #[test]
    #[ignore]
    fn e2e_scan_real_stores() {
        let scan = scan();
        eprintln!(
            "ollama={:?} lmstudio={:?} candidates={}",
            scan.ollama_dir,
            scan.lmstudio_dir,
            scan.candidates.len()
        );
        for c in scan.candidates.iter().take(40) {
            eprintln!(
                "{:?} {} {:?} {:?} {} mmproj={}",
                c.source,
                c.name,
                c.architecture,
                c.quantization,
                c.size_bytes,
                c.mmproj_path.is_some()
            );
        }
    }

    /// Links and hard-links the smallest real model (never modifies the source files).
    #[test]
    #[ignore]
    fn e2e_import_link_and_copy_real_model() {
        let first = scan();
        let Some(c) = first
            .candidates
            .iter()
            .min_by_key(|c| c.size_bytes)
            .cloned()
        else {
            eprintln!("no Ollama / LM Studio models on this machine, skipping");
            return;
        };
        let src_len = fs::metadata(&c.path).unwrap().len();

        let r = import(std::slice::from_ref(&c.path), ImportMode::Link);
        assert!(r[0].ok, "{:?}", r);
        let models = crate::models::ModelManager.scan_models();
        let linked = models
            .iter()
            .find(|m| m.path == c.path)
            .expect("linked model listed");
        assert!(linked.source.is_some());
        assert!(
            crate::models::ModelManager
                .delete_model(&linked.id)
                .is_err(),
            "never deletes"
        );
        assert!(scan()
            .candidates
            .iter()
            .any(|x| x.path == c.path && x.imported));

        let r = import(std::slice::from_ref(&c.path), ImportMode::Copy);
        assert!(r[0].ok, "{:?}", r);
        let copied = PathBuf::from(r[0].model_id.clone().unwrap());
        assert_eq!(fs::metadata(&copied).unwrap().len(), src_len);
        eprintln!("copy mode: {}", r[0].detail);
        fs::remove_file(&copied).unwrap();

        unlink_external(&c.path).unwrap();
        assert_eq!(
            fs::metadata(&c.path).unwrap().len(),
            src_len,
            "source untouched"
        );
        assert!(
            import(&[PathBuf::from("C:/Windows/notepad.exe")], ImportMode::Link)[0]
                .ok
                .eq(&false)
        );
    }

    #[test]
    fn test_parse_ollama_digest_strict() {
        let valid_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(
            parse_ollama_digest(&format!("sha256:{}", valid_hex)),
            Some(valid_hex)
        );
        assert_eq!(
            parse_ollama_digest(&format!("sha256-{}", valid_hex)),
            Some(valid_hex)
        );

        // Uppercase rejected
        assert_eq!(
            parse_ollama_digest(&format!("sha256:{}", valid_hex.to_uppercase())),
            None
        );
        // Path traversal / separators rejected
        assert_eq!(parse_ollama_digest("sha256:../../evil"), None);
        assert_eq!(parse_ollama_digest("sha256-../../evil"), None);
        assert_eq!(
            parse_ollama_digest(&format!("sha256:{}/sub", &valid_hex[..60])),
            None
        );
        // Length != 64 rejected
        assert_eq!(
            parse_ollama_digest(&format!("sha256:{}", &valid_hex[..63])),
            None
        );
        assert_eq!(parse_ollama_digest(&format!("sha256:{}0", valid_hex)), None);
        // Wrong prefix rejected
        assert_eq!(parse_ollama_digest(&format!("sha512:{}", valid_hex)), None);
    }

    #[test]
    fn test_blob_path_stays_inside_blobs() {
        let tmp = tempfile::tempdir().unwrap();
        let models_dir = tmp.path();
        let valid_hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

        let path = blob_path(models_dir, &format!("sha256:{}", valid_hex)).unwrap();
        assert!(path.starts_with(models_dir.join("blobs")));
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            format!("sha256-{}", valid_hex)
        );

        // Path traversal attempts must return None
        assert!(blob_path(models_dir, "sha256:../../etc/passwd").is_none());
        assert!(blob_path(models_dir, "sha256-..\\..\\windows\\system32").is_none());
    }

    #[test]
    fn test_import_copy_transactional_rollback() {
        let tmp = tempfile::tempdir().unwrap();
        let src_model = tmp.path().join("model.gguf");
        fs::write(&src_model, b"GGUF_MODEL_DATA").unwrap();
        let non_existent_mmproj = tmp.path().join("non_existent_mmproj.gguf");

        let candidate = ImportCandidate {
            source: ImportSource::Ollama,
            name: "test-model".to_string(),
            size_bytes: 15,
            mmproj_path: Some(non_existent_mmproj.clone()),
            architecture: None,
            quantization: None,
            path: src_model.clone(),
            imported: false,
        };

        let target = tmp.path().join("imported/model.gguf");
        assert!(!target.exists());

        // Perform transactional copy import: failure on mmproj rolls back main target
        let mut created_files: Vec<PathBuf> = Vec::new();
        let res: Result<(&'static str, PathBuf), String> = (|| {
            let how = link_or_copy(&candidate.path, &target)?;
            created_files.push(target.clone());

            if let Some(mm) = &candidate.mmproj_path {
                let name = target
                    .file_name()
                    .map(|n| format!("mmproj-{}", n.to_string_lossy()))
                    .unwrap_or_default();
                let mm_target = target.with_file_name(name);
                if !mm_target.exists() {
                    link_or_copy(mm, &mm_target)?;
                    created_files.push(mm_target);
                }
            }
            Ok((how, target.clone()))
        })();

        if res.is_err() {
            for path in created_files {
                if path.exists() {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        assert!(res.is_err());
        assert!(!target.exists(), "Main copy must be removed on failure");

        // Now create valid mmproj and retry: must succeed without "Hedef zaten var" error
        fs::write(&non_existent_mmproj, b"GGUF_MMPROJ_DATA").unwrap();
        let mut created_files2: Vec<PathBuf> = Vec::new();
        let res2: Result<(&'static str, PathBuf), String> = (|| {
            let how = link_or_copy(&candidate.path, &target)?;
            created_files2.push(target.clone());

            if let Some(mm) = &candidate.mmproj_path {
                let name = target
                    .file_name()
                    .map(|n| format!("mmproj-{}", n.to_string_lossy()))
                    .unwrap_or_default();
                let mm_target = target.with_file_name(name);
                if !mm_target.exists() {
                    link_or_copy(mm, &mm_target)?;
                    created_files2.push(mm_target);
                }
            }
            Ok((how, target.clone()))
        })();

        assert!(res2.is_ok());
        assert!(target.exists());
    }

    #[test]
    fn test_collect_files_depth_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut curr = tmp.path().to_path_buf();
        for i in 1..=6 {
            curr = curr.join(format!("level{}", i));
            fs::create_dir_all(&curr).unwrap();
            fs::write(curr.join("file.txt"), b"hi").unwrap();
        }

        let mut collected = Vec::new();
        collect_files(tmp.path(), &mut collected, 3);
        assert_eq!(collected.len(), 3);
    }
}
