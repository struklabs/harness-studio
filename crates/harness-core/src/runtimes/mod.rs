//! Runtime registry (P2 "alternative runtimes"): the official llama.cpp runtime plus named
//! extra llama-server builds, chosen per server profile / model definition.
//!
//! * `official` is virtual: whatever Settings > Runtime resolves to (the managed official
//!   build, or the configured custom binary). A launch without a runtime choice uses it, so
//!   nothing changes for existing profiles.
//! * Extra runtimes live in `config/runtimes.json`. Each one comes from a GitHub release
//!   (downloaded into `<data>/bin/runtimes/<id>/`, archive checked against the SHA-256 digest
//!   GitHub publishes; without a digest only after an explicit confirmation, same rule as the
//!   main runtime) or from a local folder (referenced in place, or copied under the data dir).
//! * Every file that can run code (`llama-server`, the ggml/llama libraries...) is hashed when
//!   the runtime is added; a launch refuses a runtime whose files changed since
//!   (`runtime.changed`) until it is verified again.
//! * Capabilities (flags, devices) come from the per-binary capability cache
//!   (`capabilities::registry`), so the parameter catalog is gated by the chosen binary.
//! * Tensor types a build can load: see [`quant`].

pub mod github;
pub mod quant;

use crate::error_key::err;

use crate::config::AppSettings;
use crate::runtime::{binary_name, RuntimeManager};
use crate::store::{Envelope, JsonStore, Schema};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Id of the virtual official runtime (Settings > Runtime).
pub const OFFICIAL_ID: &str = "official";
/// Id of the virtual system runtime: a llama-server found on PATH or in a known install
/// folder (WinGet, scoop...). Listed when one is found, so the state is shown honestly even
/// when the official runtime falls back to it.
pub const SYSTEM_ID: &str = "system";

pub const ERR_NOT_FOUND: &str = "runtime.not_found";
pub const ERR_ID_TAKEN: &str = "runtime.id_taken";
pub const ERR_INVALID_ID: &str = "runtime.invalid_id";
pub const ERR_NO_SERVER: &str = "runtime.no_server_binary";
/// The request names a runtime family this build does not know.
pub const ERR_UNKNOWN_FAMILY: &str = "runtime.unknown_family";
/// No runtime of the family the model needs is installed (`runtime.family_not_installed:
/// whisper.cpp`): a whisper ggml file runs on a whisper.cpp runtime, an image model on
/// stable-diffusion.cpp.
pub const ERR_FAMILY_NOT_INSTALLED: &str = "runtime.family_not_installed";
/// The chosen runtime belongs to another family than the model needs
/// (`runtime.family_mismatch: <id> (llama.cpp, needs whisper.cpp)`).
pub const ERR_FAMILY_MISMATCH: &str = "runtime.family_mismatch";
pub const ERR_NOT_RUNNABLE: &str = "runtime.not_runnable";
pub const ERR_CHANGED: &str = "runtime.changed";
pub const ERR_IN_USE: &str = "runtime.in_use";
pub const ERR_OFFICIAL: &str = "runtime.official_fixed";
pub const ERR_QUANT_UNSUPPORTED: &str = "runtime.quant_unsupported";

/// `runtimes.json`: `{schema_version, runtimes: [...]}`.
pub const RUNTIMES_SCHEMA: Schema = Schema {
    file: "runtimes.json",
    current: 1,
    envelope: Envelope::Wrapped("runtimes"),
    migrations: &[],
};

/// Where a runtime came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RuntimeSource {
    /// A GitHub release archive (`owner/repo`, release tag, installed archives).
    Github {
        repo: String,
        tag: String,
        assets: Vec<InstalledAsset>,
    },
    /// A folder the user picked. `copied` = copied under the data dir at add time; otherwise
    /// the folder is used in place (never modified or deleted by Harness).
    Local {
        #[cfg_attr(feature = "openapi", schema(value_type = String))]
        path: PathBuf,
        copied: bool,
    },
}

/// A release archive a runtime was installed from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstalledAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// SHA-256 the download was checked against (`None` = none published; installed only
    /// after the user confirmed).
    pub sha256: Option<String>,
}

/// How far the files of a runtime can be trusted.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RuntimeTrust {
    /// Every archive matched the SHA-256 digest its GitHub release publishes.
    Verified,
    /// At least one archive had no digest; the user confirmed installing it anyway.
    UserConfirmed,
    /// The user's own folder: its files were hashed when it was added (changes are detected,
    /// the origin is not verified).
    LocalFiles,
}

/// One hashed file of a runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FileDigest {
    pub name: String,
    pub size: u64,
    pub mtime_ms: Option<u64>,
    pub sha256: String,
}

/// An extra runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuntimeEntry {
    /// `[a-z0-9-]`, unique; used in definitions/profiles (`runtime` setting).
    pub id: String,
    pub name: String,
    /// Runtime family (`llama.cpp`, later `whisper.cpp`); entries written before families
    /// existed are llama.cpp.
    #[serde(default = "crate::engine::default_family")]
    pub family: String,
    pub source: RuntimeSource,
    /// Folder that holds `llama-server`.
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub dir: PathBuf,
    /// First line of `llama-server --version`.
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub build: Option<u64>,
    #[serde(default)]
    pub commit: Option<String>,
    /// ggml backend of the build (`vulkan`, `rocm`, `cuda`, `cpu`...).
    #[serde(default)]
    pub backend: Option<String>,
    pub trust: RuntimeTrust,
    /// Hashes of the executable files (checked at launch by size + mtime, fully by verify).
    #[serde(default)]
    pub files: Vec<FileDigest>,
    /// Quant names its `llama-quantize --help` lists (empty = not detected).
    #[serde(default)]
    pub quant_types: Vec<String>,
    #[serde(default)]
    pub added_at: u64,
    #[serde(default)]
    pub verified_at: u64,
}

impl RuntimeEntry {
    pub fn effective_backend(&self) -> Option<String> {
        self.backend.clone().or_else(|| {
            if self.family != crate::engine::AUDIO_CPP {
                return None;
            }
            match &self.source {
                RuntimeSource::Github { assets, .. } => assets
                    .iter()
                    .find_map(|asset| crate::engine::audio_cpp::backend_from_archive(&asset.name)),
                RuntimeSource::Local { .. } => None,
            }
        })
    }
    /// The engine of this entry's family; `None` for a family this build does not know.
    pub fn engine(&self) -> Option<&'static dyn crate::engine::Engine> {
        crate::engine::engine(&self.family)
    }

    pub fn binary(&self) -> PathBuf {
        match self.engine() {
            Some(e) => self.dir.join(e.release_source().binary_file()),
            None => self.dir.join(binary_name()),
        }
    }

    pub fn quant_info(&self) -> quant::RuntimeQuantInfo {
        quant::RuntimeQuantInfo {
            quant_types: (!self.quant_types.is_empty()).then(|| self.quant_types.clone()),
            official: false,
            legacy_prism_q2_0: self.is_legacy_prism_q2_0_runtime(),
        }
    }

    fn is_legacy_prism_q2_0_runtime(&self) -> bool {
        match &self.source {
            RuntimeSource::Github { repo, tag, .. } => {
                repo.eq_ignore_ascii_case("PrismML-Eng/llama.cpp")
                    && tag.to_ascii_lowercase().starts_with("prism-b9601")
            }
            RuntimeSource::Local { .. } => {
                self.build.is_some_and(|build| build <= 9601)
                    && self
                        .version
                        .as_deref()
                        .unwrap_or(&self.name)
                        .to_ascii_lowercase()
                        .contains("prism")
            }
        }
    }
}

pub fn now_secs() -> u64 {
    crate::capabilities::now_secs()
}

pub(crate) fn mtime_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}

/// Files of a runtime folder worth hashing: executables and shared libraries (what a launch
/// can load). Top level only; archives, logs and docs are ignored.
fn is_code_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(ext, "exe" | "dll" | "so" | "dylib" | "metal" | "metallib")
        || lower.contains(".so.")
        || (ext.is_empty()
            && (lower.starts_with("llama-")
                || lower.starts_with("whisper-")
                || lower.starts_with("sd-")
                || lower.starts_with("mistralrs")
                || lower.starts_with("ggml")
                || lower.starts_with("lib")))
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let read_failed = |e: std::io::Error| -> String {
        err("fs.read_failed")
            .with("path", path.display())
            .with("error", e)
            .into()
    };
    let mut f = std::fs::File::open(path).map_err(read_failed)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(read_failed)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(crate::hex::lower(&hasher.finalize()))
}

/// SHA-256 of every code file of `dir` (sorted by name). Blocking.
pub fn hash_manifest(dir: &Path) -> Result<Vec<FileDigest>, String> {
    let mut out = Vec::new();
    let read_dir_failed = |e: std::io::Error| -> String {
        err("fs.read_dir_failed")
            .with("path", dir.display())
            .with("error", e)
            .into()
    };
    for entry in std::fs::read_dir(dir).map_err(read_dir_failed)? {
        let entry = entry.map_err(read_dir_failed)?;
        let meta = entry.metadata().map_err(read_dir_failed)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !meta.is_file() || !is_code_file(&name) {
            continue;
        }
        out.push(FileDigest {
            sha256: sha256_file(&entry.path())?,
            size: meta.len(),
            mtime_ms: mtime_ms(&meta),
            name,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Cheap launch-time check: every recorded file still exists with the same size and mtime.
/// Returns the names that differ.
pub fn changed_files(entry: &RuntimeEntry) -> Vec<String> {
    entry
        .files
        .iter()
        .filter(|f| match std::fs::metadata(entry.dir.join(&f.name)) {
            Ok(m) => m.len() != f.size || mtime_ms(&m) != f.mtime_ms,
            Err(_) => true,
        })
        .map(|f| f.name.clone())
        .collect()
}

/// A freshly extracted build can take long on its first run (the OS scans every new DLL; the
/// ggml backends initialize), so the first `--version` gets more time than the usual probe.
const FIRST_RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

/// `--version` of a llama.cpp binary (stdout, else stderr). Blocking.
fn probe_version(binary: &Path) -> Result<String, String> {
    probe_version_with(binary, "--version")
}

/// What `arg` (the family's [`crate::engine::Engine::version_arg`]) prints (stdout, else
/// stderr); a binary that does not run, or exits non-zero, is an error. Blocking.
fn probe_version_with(binary: &Path, arg: &str) -> Result<String, String> {
    if !binary.is_file() {
        return Err(format!("{}: {}", ERR_NO_SERVER, binary.display()));
    }
    let mut cmd = crate::process::quiet_command(binary);
    cmd.arg(arg)
        .current_dir(binary.parent().unwrap_or(Path::new(".")));
    match crate::runtime::run_command_with_timeout(
        &mut cmd,
        FIRST_RUN_TIMEOUT,
        crate::runtime::PROBE_MAX_BYTES,
    ) {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Ok(if stdout.is_empty() { stderr } else { stdout })
        }
        Ok(out) => Err(format!("{}: exit {}", ERR_NOT_RUNNABLE, out.status)),
        Err(e) => Err(format!("{}: {}", ERR_NOT_RUNNABLE, e)),
    }
}

/// Version fields, backend and quant types of a runtime folder (runs the binaries).
struct Probe {
    version: Option<String>,
    build: Option<u64>,
    commit: Option<String>,
    backend: Option<String>,
    quant_types: Vec<String>,
}

fn probe_dir(dir: &Path, engine: &dyn crate::engine::Engine) -> Result<Probe, String> {
    let binary = dir.join(engine.release_source().binary_file());
    if !binary.is_file() {
        return Err(format!("{}: {}", ERR_NO_SERVER, dir.display()));
    }
    let text = probe_version_with(&binary, engine.version_arg())?;
    if engine.family() != crate::engine::LLAMA_CPP {
        // The binary prints no version (whisper-server): the registry takes the release tag
        // (`version_from_tag`); one that does (mistral.rs) gives it here. The quant types are
        // llama-quantize's.
        return Ok(Probe {
            version: engine.version_text(&text),
            build: None,
            commit: None,
            backend: crate::runtime::detect_backend_in_dir(dir),
            quant_types: Vec::new(),
        });
    }
    let info = crate::capabilities::parse::parse_version(&text);
    Ok(Probe {
        version: Some(info.raw.clone()).filter(|s| !s.is_empty()),
        build: info.build,
        commit: info.commit,
        backend: crate::runtime::detect_backend_in_dir(dir),
        quant_types: quant::detect_quant_types(&binary).unwrap_or_default(),
    })
}

/// Version and build number a release tag stands for: `b5130` → (`b5130`, 5130), `v1.9.2` →
/// (`1.9.2`, none). For families whose binary does not print a version.
pub(crate) fn version_from_tag(tag: &str) -> (Option<String>, Option<u64>) {
    let tag = tag.trim();
    if let Some(n) = tag.strip_prefix('b').and_then(|n| n.parse::<u64>().ok()) {
        return (Some(tag.to_string()), Some(n));
    }
    // stable-diffusion.cpp: `master-956-1b0ba10` (the number counts the commits).
    if let Some(n) = tag
        .strip_prefix("master-")
        .and_then(|rest| rest.split('-').next())
        .and_then(|n| n.parse::<u64>().ok())
    {
        return (Some(tag.to_string()), Some(n));
    }
    let v = tag.strip_prefix('v').unwrap_or(tag);
    ((!v.is_empty()).then(|| v.to_string()), None)
}

/// `[a-z0-9-]{1,40}`, starting with a letter or digit, not `official`.
pub fn validate_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && id != OFFICIAL_ID
        && id != SYSTEM_ID;
    if ok {
        Ok(())
    } else {
        Err(format!("{}: {}", ERR_INVALID_ID, id))
    }
}

/// Id from a display name (`PrismML llama.cpp (Bonsai)` -> `prismml-llama-cpp-bonsai`).
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut out: String = out.trim_end_matches('-').chars().take(40).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() || out == OFFICIAL_ID || out == SYSTEM_ID {
        "runtime".into()
    } else {
        out
    }
}

/// A local folder to register.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddLocal {
    /// Folder with `llama-server` (or the `llama-server` file itself).
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub path: PathBuf,
    pub name: String,
    /// Wanted id (default: from the name).
    #[serde(default)]
    pub id: Option<String>,
    /// Copy the folder's top-level files under the data dir instead of using it in place.
    #[serde(default)]
    pub copy: bool,
    /// Runtime family of the folder (default llama.cpp).
    #[serde(default = "crate::engine::default_family")]
    pub family: String,
}

/// Result of a full check of a runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VerifyReport {
    pub id: String,
    /// Files match the recorded hashes and `llama-server --version` runs.
    pub ok: bool,
    pub version: Option<String>,
    /// Recorded files whose SHA-256 differs now.
    pub changed: Vec<String>,
    /// Recorded files that are gone.
    pub missing: Vec<String>,
    /// Code files that weren't there when the runtime was added.
    pub added: Vec<String>,
    /// The new hashes were accepted (local runtimes, `accept_changes`).
    pub accepted: bool,
    pub error: Option<String>,
}

/// The registry file and the folder managed runtimes are installed into.
#[derive(Debug, Clone)]
pub struct RuntimeRegistry {
    file: PathBuf,
    root: PathBuf,
}

impl RuntimeRegistry {
    /// `<data>/config/runtimes.json`, runtimes under `<data>/bin/runtimes/`.
    pub fn global() -> Self {
        Self::with_paths(
            crate::paths::config_dir().join(RUNTIMES_SCHEMA.file),
            crate::paths::bin_dir().join("runtimes"),
        )
    }

    pub fn with_paths(file: PathBuf, root: PathBuf) -> Self {
        Self { file, root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn store(&self) -> JsonStore {
        JsonStore::new(&self.file)
    }

    pub fn entries(&self) -> Result<Vec<RuntimeEntry>, String> {
        Ok(self
            .store()
            .load_versioned_or_default::<Vec<RuntimeEntry>>(&RUNTIMES_SCHEMA)?
            .data)
    }

    pub fn get(&self, id: &str) -> Result<RuntimeEntry, String> {
        self.entries()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("{}: {}", ERR_NOT_FOUND, id))
    }

    /// A free id: `wanted` (must be free) or one derived from `name`.
    pub fn unique_id(&self, wanted: Option<&str>, name: &str) -> Result<String, String> {
        let taken: Vec<String> = self.entries()?.into_iter().map(|e| e.id).collect();
        if let Some(w) = wanted.map(str::trim).filter(|w| !w.is_empty()) {
            validate_id(w)?;
            if taken.iter().any(|t| t == w) {
                return Err(format!("{}: {}", ERR_ID_TAKEN, w));
            }
            return Ok(w.to_string());
        }
        let base = slug(name);
        let mut id = base.clone();
        let mut n = 2;
        while taken.contains(&id) {
            id = format!("{}-{}", base, n);
            n += 1;
        }
        Ok(id)
    }

    /// Adds (or replaces, same id) an entry.
    pub fn save_entry(&self, entry: RuntimeEntry) -> Result<(), String> {
        self.store()
            .update_versioned::<Vec<RuntimeEntry>, ()>(&RUNTIMES_SCHEMA, |list| {
                match list.iter_mut().find(|e| e.id == entry.id) {
                    Some(e) => *e = entry,
                    None => list.push(entry),
                }
                Ok(())
            })
    }

    /// Registers a local folder (in place or copied). Runs `--version`, hashes the code files
    /// and reads the quant types. Blocking.
    pub fn add_local(&self, req: &AddLocal) -> Result<RuntimeEntry, String> {
        let engine = crate::engine::engine(&req.family)
            .ok_or_else(|| format!("{}: {}", ERR_UNKNOWN_FAMILY, req.family))?;
        let binary = engine.release_source().binary_file();
        let mut src = crate::paths::normalize(&req.path);
        if src.is_file() {
            src = src
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| format!("{}: {}", ERR_NO_SERVER, req.path.display()))?;
        }
        if !src.join(&binary).is_file() {
            return Err(format!("{}: {}", ERR_NO_SERVER, src.display()));
        }
        let name = if req.name.trim().is_empty() && engine.family() == crate::engine::MISTRALRS {
            // Its folder is cargo's `target/release`: the family says more than `release`.
            engine.family().to_string()
        } else if req.name.trim().is_empty() {
            src.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "llama.cpp".into())
        } else {
            req.name.trim().to_string()
        };
        let id = self.unique_id(req.id.as_deref(), &name)?;
        // Probe the source first: a folder that doesn't run is never copied.
        probe_version_with(&src.join(&binary), engine.version_arg())?;
        let dir = if req.copy {
            let dest = self.root.join(&id);
            if dest.exists() {
                std::fs::remove_dir_all(&dest).map_err(|e| {
                    String::from(
                        err("fs.remove_failed")
                            .with("path", dest.display())
                            .with("error", e),
                    )
                })?;
            }
            copy_top_level(&src, &dest)?;
            dest
        } else {
            src.clone()
        };
        let result = (|| {
            let probe = probe_dir(&dir, engine)?;
            let files = hash_manifest(&dir)?;
            let now = now_secs();
            let entry = RuntimeEntry {
                id: id.clone(),
                name,
                family: engine.family().to_string(),
                source: RuntimeSource::Local {
                    path: src.clone(),
                    copied: req.copy,
                },
                dir: dir.clone(),
                version: probe.version,
                build: probe.build,
                commit: probe.commit,
                backend: probe.backend,
                trust: RuntimeTrust::LocalFiles,
                files,
                quant_types: probe.quant_types,
                added_at: now,
                verified_at: now,
            };
            self.save_entry(entry.clone())?;
            Ok(entry)
        })();
        if result.is_err() && req.copy {
            let _ = std::fs::remove_dir_all(&dir);
        }
        result
    }

    /// Re-hashes every file and runs `--version`. With `accept_changes` a local runtime takes
    /// the new hashes as its baseline (the user updated their own folder); a downloaded one
    /// never does (reinstall it instead). Blocking.
    pub fn verify(&self, id: &str, accept_changes: bool) -> Result<VerifyReport, String> {
        let mut entry = self.get(id)?;
        let mut report = VerifyReport {
            id: id.to_string(),
            ok: false,
            version: None,
            changed: Vec::new(),
            missing: Vec::new(),
            added: Vec::new(),
            accepted: false,
            error: None,
        };
        let now = match hash_manifest(&entry.dir) {
            Ok(files) => files,
            Err(e) => {
                report.error = Some(e);
                return Ok(report);
            }
        };
        for f in &entry.files {
            match now.iter().find(|n| n.name == f.name) {
                Some(n) if n.sha256 == f.sha256 => {}
                Some(_) => report.changed.push(f.name.clone()),
                None => report.missing.push(f.name.clone()),
            }
        }
        for n in &now {
            if !entry.files.iter().any(|f| f.name == n.name) {
                report.added.push(n.name.clone());
            }
        }
        let clean =
            report.changed.is_empty() && report.missing.is_empty() && report.added.is_empty();
        let probe = match entry.engine() {
            Some(engine) => probe_dir(&entry.dir, engine),
            None => Err(format!("{}: {}", ERR_UNKNOWN_FAMILY, entry.family)),
        };
        match &probe {
            // A family whose binary prints no version keeps the release tag's.
            Ok(p) => report.version = p.version.clone().or_else(|| entry.version.clone()),
            Err(e) => report.error = Some(e.clone()),
        }
        let local = matches!(entry.source, RuntimeSource::Local { .. });
        if clean || (accept_changes && local) {
            if let Ok(p) = probe {
                report.accepted = !clean;
                report.ok = true;
                entry.files = now;
                if p.version.is_some() {
                    entry.version = p.version;
                    entry.build = p.build;
                    entry.commit = p.commit;
                }
                entry.backend = p.backend.or_else(|| entry.effective_backend());
                entry.quant_types = p.quant_types;
                entry.verified_at = now_secs();
                self.save_entry(entry)?;
            }
        }
        Ok(report)
    }

    /// Unregisters a runtime. Its folder is deleted only when Harness created it (under the
    /// runtimes root); a local folder used in place is never touched. Blocking.
    pub fn remove(&self, id: &str) -> Result<RuntimeEntry, String> {
        if id == OFFICIAL_ID {
            return Err(ERR_OFFICIAL.to_string());
        }
        let entry = self.get(id)?;
        self.store()
            .update_versioned::<Vec<RuntimeEntry>, ()>(&RUNTIMES_SCHEMA, |list| {
                list.retain(|e| e.id != id);
                Ok(())
            })?;
        if self.owns_dir(&entry) && entry.dir.exists() {
            std::fs::remove_dir_all(&entry.dir).map_err(|e| {
                String::from(
                    err("fs.remove_failed")
                        .with("path", entry.dir.display())
                        .with("error", e),
                )
            })?;
            self.shared_store().prune();
        }
        Ok(entry)
    }

    /// Shared companion libraries (`<bin>/shared`, the runtimes root's parent; the managed
    /// build uses the same store).
    pub fn shared_store(&self) -> crate::shared_libs::SharedStore {
        crate::shared_libs::SharedStore::in_bin(self.root.parent().unwrap_or(&self.root))
    }

    /// The runtime's folder was created by Harness (download or copy).
    pub fn owns_dir(&self, entry: &RuntimeEntry) -> bool {
        let managed = match &entry.source {
            RuntimeSource::Github { .. } => true,
            RuntimeSource::Local { copied, .. } => *copied,
        };
        managed && crate::paths::is_within(&entry.dir, &self.root)
    }

    /// The binary of a registered runtime after the launch check (files unchanged).
    pub fn launch_binary(&self, id: &str) -> Result<PathBuf, String> {
        let entry = self.get(id)?;
        let binary = entry.binary();
        if !binary.is_file() {
            return Err(format!("{}: {}", ERR_NO_SERVER, binary.display()));
        }
        let changed = changed_files(&entry);
        if !changed.is_empty() {
            return Err(format!("{}: {} ({})", ERR_CHANGED, id, changed.join(", ")));
        }
        Ok(binary)
    }
}

/// Copies the regular files at the top level of `src` into `dest` (created).
fn copy_top_level(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", dest.display())
                .with("error", e),
        )
    })?;
    let read_dir_failed = |e: std::io::Error| -> String {
        err("fs.read_dir_failed")
            .with("path", src.display())
            .with("error", e)
            .into()
    };
    for entry in std::fs::read_dir(src).map_err(read_dir_failed)? {
        let entry = entry.map_err(read_dir_failed)?;
        if entry.file_type().map_err(read_dir_failed)?.is_file() {
            let to = dest.join(entry.file_name());
            std::fs::copy(entry.path(), &to).map_err(|e| {
                String::from(
                    err("fs.copy_failed")
                        .with("from", entry.path().display())
                        .with("to", to.display())
                        .with("error", e),
                )
            })?;
        }
    }
    Ok(())
}

/// `None`, empty and `official` all mean the official runtime.
pub fn is_official(id: Option<&str>) -> bool {
    id.map(str::trim)
        .is_none_or(|id| id.is_empty() || id == OFFICIAL_ID)
}

/// The llama-server binary for a runtime choice: the official one (Settings > Runtime) or a
/// registered runtime whose files are unchanged. Blocking.
pub fn resolve_binary_in(
    reg: &RuntimeRegistry,
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
    id: Option<&str>,
) -> Result<PathBuf, String> {
    if is_official(id) {
        crate::orchestrator::resolve_server_binary(runtime_mgr, settings)
    } else if id.map(str::trim) == Some(SYSTEM_ID) {
        RuntimeManager::detect_system_binary_cached()
            .ok_or_else(|| format!("{}: {}", ERR_NOT_FOUND, SYSTEM_ID))
    } else {
        reg.launch_binary(id.unwrap_or_default().trim())
    }
}

impl RuntimeRegistry {
    /// The runtime a launch of `family` uses when none is chosen: the newest installed one
    /// (a downloaded or added build; `official` / `system` are llama.cpp's).
    pub fn default_for_family(&self, family: &str) -> Result<RuntimeEntry, String> {
        self.entries()?
            .into_iter()
            .filter(|e| e.family == family)
            .max_by_key(|e| e.added_at)
            .ok_or_else(|| format!("{}: {}", ERR_FAMILY_NOT_INSTALLED, family))
    }
}

/// The binary a launch of `family` runs: the chosen runtime when it belongs to that family
/// (a registered runtime whose files are unchanged), else, with no choice (or `official`),
/// the family's default ([`RuntimeRegistry::default_for_family`]). llama.cpp launches use
/// [`resolve_binary_in`]. Blocking.
pub fn resolve_family_binary_in(
    reg: &RuntimeRegistry,
    family: &str,
    id: Option<&str>,
) -> Result<PathBuf, String> {
    if is_official(id) {
        let entry = reg.default_for_family(family)?;
        return reg.launch_binary(&entry.id);
    }
    let id = id.unwrap_or_default().trim();
    let entry = reg.get(id)?;
    if entry.family != family {
        return Err(format!(
            "{}: {} ({}, needs {})",
            ERR_FAMILY_MISMATCH, id, entry.family, family
        ));
    }
    reg.launch_binary(id)
}

/// Engine of a runtime id: the official and system runtimes are llama.cpp builds; a
/// registered one has its family (an unknown family is an error, not a llama.cpp guess).
pub fn engine_for_in(
    reg: &RuntimeRegistry,
    id: Option<&str>,
) -> Result<&'static dyn crate::engine::Engine, String> {
    if is_official(id) || id.map(str::trim) == Some(SYSTEM_ID) {
        return Ok(crate::engine::llama_cpp());
    }
    let entry = reg.get(id.unwrap_or_default().trim())?;
    entry
        .engine()
        .ok_or_else(|| format!("{}: {}", ERR_UNKNOWN_FAMILY, entry.family))
}

/// [`engine_for_in`] with the global registry.
pub fn engine_for(id: Option<&str>) -> Result<&'static dyn crate::engine::Engine, String> {
    engine_for_in(&RuntimeRegistry::global(), id)
}

/// [`resolve_binary_in`] with the global registry.
pub fn resolve_binary(
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
    id: Option<&str>,
) -> Result<PathBuf, String> {
    resolve_binary_in(&RuntimeRegistry::global(), runtime_mgr, settings, id)
}

/// What the UI/API/CLI list shows for one runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuntimeView {
    pub id: String,
    /// Empty for the official runtime (the UI shows its own label).
    pub name: String,
    /// `official`, `github` or `local`.
    pub kind: String,
    /// The registry entry (`None` for the official runtime).
    pub entry: Option<RuntimeEntry>,
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub binary_path: Option<PathBuf>,
    /// The binary can be launched now (exists, files unchanged).
    pub available: bool,
    /// Why not (error key + detail).
    pub error: Option<String>,
    pub version: Option<crate::capabilities::parse::VersionInfo>,
    pub backend: Option<String>,
    /// `--help` could be parsed (flag gating applies).
    pub help_parsed: bool,
    pub flag_count: usize,
    /// Catalog parameters this binary has no flag for (hidden and never sent for it).
    pub unsupported_params: Vec<String>,
    pub quant_types: Vec<String>,
    /// Official runtime only: what Settings > Runtime is set to and what is really used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub official: Option<OfficialState>,
}

/// The honest state of the official runtime (Settings > Runtime).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OfficialState {
    /// Configured mode: `managed` or `custom`.
    pub configured_mode: String,
    /// Where the binary used now comes from (`None` = nothing found).
    pub source: Option<crate::orchestrator::BinarySource>,
    /// The configured mode can't be honored (e.g. managed build not installed), so the binary
    /// used is a fallback (a system llama-server found on PATH / WinGet...).
    pub fallback: bool,
    pub managed_installed: bool,
    /// Version the settings record for the managed build.
    pub managed_version: Option<String>,
    /// A downloaded managed build that passed stage 1 but was never activated.
    pub staged: Option<crate::capabilities::parse::VersionInfo>,
}

/// Official runtime state (runs the staged build's `--version` when one waits). Blocking.
pub fn official_state(runtime_mgr: &RuntimeManager, settings: &AppSettings) -> OfficialState {
    let resolved = crate::orchestrator::resolve_server_binary_detail(runtime_mgr, settings).ok();
    let staged_bin = runtime_mgr.staging_dir().join(binary_name());
    let staged = (!runtime_mgr.is_busy() && staged_bin.is_file())
        .then(|| probe_version(&staged_bin).ok())
        .flatten()
        .map(|text| crate::capabilities::parse::parse_version(&text));
    OfficialState {
        configured_mode: match settings.runtime_mode {
            crate::config::RuntimeMode::Managed => "managed",
            crate::config::RuntimeMode::Custom => "custom",
        }
        .into(),
        source: resolved.as_ref().map(|r| r.source),
        fallback: resolved.as_ref().is_some_and(|r| r.fallback),
        managed_installed: runtime_mgr.get_managed_binary_path().is_some(),
        managed_version: settings.managed_runtime.installed_version.clone(),
        staged,
    }
}

/// Catalog parameter ids whose flags a binary lacks (empty when its help wasn't parsed).
pub fn unsupported_params(caps: &crate::capabilities::RuntimeCapabilities) -> Vec<String> {
    unsupported_params_of(crate::params::catalog(), caps)
}

/// [`unsupported_params`] for the catalog of another family.
pub fn unsupported_params_of(
    catalog: &crate::params::ParamCatalog,
    caps: &crate::capabilities::RuntimeCapabilities,
) -> Vec<String> {
    if !caps.help_parsed {
        return Vec::new();
    }
    catalog
        .params
        .iter()
        .filter(|p| {
            caps.first_flag(&p.flags).is_none()
                && (p.off_flags.is_empty() || caps.first_flag(&p.off_flags).is_none())
        })
        .map(|p| p.id.clone())
        .collect()
}

fn view_for_binary(
    id: String,
    name: String,
    kind: &str,
    entry: Option<RuntimeEntry>,
    binary: Result<PathBuf, String>,
) -> RuntimeView {
    match binary {
        Ok(binary) => {
            let engine = entry
                .as_ref()
                .and_then(|e| e.engine())
                .unwrap_or_else(|| crate::engine::llama_cpp());
            let caps = engine.detect_capabilities(&binary);
            let quant_types = match &entry {
                Some(e) => e.quant_types.clone(),
                None => quant::detect_quant_types(&binary).unwrap_or_default(),
            };
            let mut version = caps.version.clone();
            if version.raw.is_empty() {
                // A family whose binary prints no version: the release tag recorded at install.
                if let Some(e) = entry.as_ref().filter(|e| e.version.is_some()) {
                    version.raw = e.version.clone().unwrap_or_default();
                    version.version = e.version.clone();
                    version.build = e.build;
                }
            }
            RuntimeView {
                backend: entry
                    .as_ref()
                    .and_then(RuntimeEntry::effective_backend)
                    .or_else(|| {
                        binary
                            .parent()
                            .and_then(crate::runtime::detect_backend_in_dir)
                    }),
                id,
                name,
                kind: kind.into(),
                entry,
                available: true,
                error: None,
                version: Some(version),
                help_parsed: caps.help_parsed,
                flag_count: caps.flags.len(),
                unsupported_params: unsupported_params_of(engine.param_catalog(), &caps),
                quant_types,
                binary_path: Some(binary),
                official: None,
            }
        }
        Err(e) => RuntimeView {
            backend: entry.as_ref().and_then(RuntimeEntry::effective_backend),
            quant_types: entry
                .as_ref()
                .map(|e| e.quant_types.clone())
                .unwrap_or_default(),
            binary_path: entry.as_ref().map(|e| e.binary()),
            id,
            name,
            kind: kind.into(),
            entry,
            available: false,
            error: Some(e),
            version: None,
            help_parsed: false,
            flag_count: 0,
            unsupported_params: Vec::new(),
            official: None,
        },
    }
}

/// View of one runtime (probes its binary once; later calls hit the capability cache).
/// Blocking.
pub fn view_in(
    reg: &RuntimeRegistry,
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
    id: &str,
) -> Result<RuntimeView, String> {
    if is_official(Some(id)) {
        let mut view = view_for_binary(
            OFFICIAL_ID.into(),
            String::new(),
            "official",
            None,
            crate::orchestrator::resolve_server_binary(runtime_mgr, settings),
        );
        view.official = Some(official_state(runtime_mgr, settings));
        return Ok(view);
    }
    if id == SYSTEM_ID {
        return Ok(view_for_binary(
            SYSTEM_ID.into(),
            String::new(),
            "system",
            None,
            resolve_binary_in(reg, runtime_mgr, settings, Some(SYSTEM_ID)),
        ));
    }
    let entry = reg.get(id)?;
    let kind = match entry.source {
        RuntimeSource::Github { .. } => "github",
        RuntimeSource::Local { .. } => "local",
    };
    Ok(view_for_binary(
        entry.id.clone(),
        entry.name.clone(),
        kind,
        Some(entry.clone()),
        reg.launch_binary(id),
    ))
}

/// The official runtime first, then the registered ones. Blocking.
pub fn list_in(
    reg: &RuntimeRegistry,
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
) -> Result<Vec<RuntimeView>, String> {
    let mut out = vec![view_in(reg, runtime_mgr, settings, OFFICIAL_ID)?];
    if RuntimeManager::detect_system_binary_cached().is_some() {
        out.push(view_in(reg, runtime_mgr, settings, SYSTEM_ID)?);
    }
    for e in reg.entries()? {
        out.push(view_in(reg, runtime_mgr, settings, &e.id)?);
    }
    Ok(out)
}

/// How well each runtime fits a model file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuntimeFit {
    pub runtime_id: String,
    pub support: quant::Support,
    /// Tensor types this runtime can't load.
    pub missing: Vec<quant::TensorType>,
}

/// Runtime advice for a model (auto-suggestion from the GGUF tensor types).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuntimeAdvice {
    pub model_path: String,
    /// Tensor types that not every build loads (empty = nothing to check).
    pub special_types: Vec<quant::TensorType>,
    pub runtimes: Vec<RuntimeFit>,
    /// The official runtime can't load this file.
    pub official_unsupported: bool,
    /// A registered runtime that loads every tensor type (preferring `current` when it does).
    pub suggested_runtime: Option<String>,
    /// The fork known to add these types, when no registered runtime loads them.
    pub fork: Option<quant::ForkHint>,
}

fn system_quant_info() -> Option<quant::RuntimeQuantInfo> {
    RuntimeManager::detect_system_binary_cached().map(|b| quant::RuntimeQuantInfo {
        quant_types: quant::detect_quant_types(&b),
        // A system llama-server is an upstream build as far as anyone can tell.
        official: true,
        legacy_prism_q2_0: false,
    })
}

fn official_quant_info(
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
) -> quant::RuntimeQuantInfo {
    quant::RuntimeQuantInfo {
        quant_types: crate::orchestrator::resolve_server_binary(runtime_mgr, settings)
            .ok()
            .and_then(|b| quant::detect_quant_types(&b)),
        official: true,
        legacy_prism_q2_0: false,
    }
}

/// Which runtimes can load `model_path` (its GGUF tensor types), and what to suggest.
/// Blocking (reads the GGUF header, may run `llama-quantize --help` once per build).
pub fn advise_in(
    reg: &RuntimeRegistry,
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
    model_path: &Path,
    current: Option<&str>,
) -> Result<RuntimeAdvice, String> {
    let meta = crate::gguf::read_gguf_metadata(model_path)?;
    let types = quant::model_types(&meta.tensor_types, meta.legacy_prism_q2_0);
    let special = quant::special_types(&types);
    let mut runtimes = Vec::new();
    let mut official_unsupported = false;
    let mut suggested = None;
    if !special.is_empty() {
        let (support, missing) =
            quant::model_support(&official_quant_info(runtime_mgr, settings), &types);
        official_unsupported = support == quant::Support::Unsupported;
        runtimes.push(RuntimeFit {
            runtime_id: OFFICIAL_ID.into(),
            support,
            missing,
        });
        if let Some(info) = system_quant_info() {
            let (support, missing) = quant::model_support(&info, &types);
            runtimes.push(RuntimeFit {
                runtime_id: SYSTEM_ID.into(),
                support,
                missing,
            });
        }
        for e in reg
            .entries()?
            .into_iter()
            .filter(|e| e.family == crate::engine::LLAMA_CPP)
        {
            let (support, missing) = quant::model_support(&e.quant_info(), &types);
            runtimes.push(RuntimeFit {
                runtime_id: e.id.clone(),
                support,
                missing,
            });
        }
        if official_unsupported {
            let fits: Vec<&RuntimeFit> = runtimes
                .iter()
                .filter(|r| r.support == quant::Support::Supported)
                .collect();
            suggested = fits
                .iter()
                .find(|r| Some(r.runtime_id.as_str()) == current)
                .or_else(|| fits.first())
                .map(|r| r.runtime_id.clone());
        }
    }
    let fork = if official_unsupported && suggested.is_none() {
        quant::fork_for(&types)
    } else {
        None
    };
    Ok(RuntimeAdvice {
        model_path: model_path.to_string_lossy().to_string(),
        special_types: special,
        runtimes,
        official_unsupported,
        suggested_runtime: suggested,
        fork,
    })
}

/// [`advise_in`] with the global registry.
pub fn advise(
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
    model_path: &Path,
    current: Option<&str>,
) -> Result<RuntimeAdvice, String> {
    advise_in(
        &RuntimeRegistry::global(),
        runtime_mgr,
        settings,
        model_path,
        current,
    )
}

/// Launch guard: refuses (with the reason) a model whose tensor types the chosen runtime
/// certainly can't load, instead of llama-server's "invalid ggml type" after the spawn.
/// Unknown support never blocks. Blocking.
pub fn check_model_support(
    reg: &RuntimeRegistry,
    runtime_mgr: &RuntimeManager,
    settings: &AppSettings,
    runtime_id: Option<&str>,
    model_path: &Path,
) -> Result<(), String> {
    if !model_path.is_file() {
        return Ok(());
    }
    let Ok(meta) = crate::gguf::read_gguf_metadata(model_path) else {
        return Ok(());
    };
    if meta.architecture.as_deref() == Some("audiocpp") {
        return Err(crate::error_key::err("audio_cpp.llama_rejected").into());
    }
    let types = quant::model_types(&meta.tensor_types, meta.legacy_prism_q2_0);
    if quant::special_types(&types).is_empty() {
        return Ok(());
    }
    let info = if is_official(runtime_id) {
        official_quant_info(runtime_mgr, settings)
    } else if runtime_id.map(str::trim) == Some(SYSTEM_ID) {
        system_quant_info().unwrap_or_default()
    } else {
        reg.get(runtime_id.unwrap_or_default().trim())?.quant_info()
    };
    let (support, missing) = quant::model_support(&info, &types);
    if support != quant::Support::Unsupported {
        return Ok(());
    }
    let labels: Vec<String> = missing.iter().map(|t| t.label()).collect();
    let hint = quant::fork_for(&types)
        .map(|f| format!("; {}", f.repo))
        .unwrap_or_default();
    Err(format!(
        "{}: {}{}",
        ERR_QUANT_UNSUPPORTED,
        labels.join(", "),
        hint
    ))
}

#[cfg(test)]
mod mistralrs_tests;
#[cfg(test)]
mod sdcpp_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod whisper_tests;
