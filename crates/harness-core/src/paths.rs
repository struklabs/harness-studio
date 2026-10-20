//! Centralized, CWD-independent application paths.
//!
//! All persistent data (settings, presets, managed runtimes, logs, default model
//! library) lives under a single data root. In the running app this root is
//! Tauri's per-user `app_data_dir()`; it is registered once at startup via
//! [`init`]. Unit tests never call `init`, so they fall back to an isolated
//! temp directory and never touch real user data.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Registers the data root. Only the first call has an effect.
pub fn init(root: PathBuf) {
    let _ = fs::create_dir_all(&root);
    let _ = DATA_ROOT.set(root);
}

/// Root directory for all persistent application data.
pub fn data_root() -> PathBuf {
    DATA_ROOT.get().cloned().unwrap_or_else(fallback_root)
}

/// Data root when `init` was never called (unit/integration tests). One directory per process:
/// a shared one leaks state between runs and between checkouts, so a leftover file (say a
/// `model_definitions.json` written by a newer build) could make an unrelated run read-only.
fn fallback_root() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        sweep_stale_test_dirs();
        std::env::temp_dir().join(format!("{TEST_DIR_PREFIX}-{}", std::process::id()))
    })
    .clone()
}

/// Name prefix of every temp directory tests create (see [`test_dir_prefix`]).
const TEST_DIR_PREFIX: &str = "harness-studio-test-data";

/// Prefix for a test's temp directory (`tempfile::Builder::prefix`): carries this process id so a
/// later test process can tell it is no longer in use. On Windows a test cannot delete a directory
/// whose SQLite files or child executables are still open when it ends, so instead of leaking one
/// directory per run, the next test process sweeps it. Test-only; never used by the app.
#[doc(hidden)]
pub fn test_dir_prefix(tag: &str) -> String {
    sweep_stale_test_dirs();
    format!("{TEST_DIR_PREFIX}-{}-{tag}-", std::process::id())
}

/// Best effort, once per process: removes test directories whose process has exited, and any
/// older than a day (a reused process id must not keep one forever).
fn sweep_stale_test_dirs() {
    static SWEPT: OnceLock<()> = OnceLock::new();
    SWEPT.get_or_init(|| {
        let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let own = std::process::id();
        let mut sys = sysinfo::System::new();
        sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::nothing(),
        );
        let day = std::time::Duration::from_secs(24 * 3600);
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(TEST_DIR_PREFIX) else {
                continue;
            };
            let pid = rest
                .trim_start_matches('-')
                .split('-')
                .next()
                .and_then(|p| p.parse::<u32>().ok());
            let exited = pid.is_some_and(|pid| {
                pid != own && sys.process(sysinfo::Pid::from_u32(pid)).is_none()
            });
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > day);
            if exited || (old && pid != Some(own)) {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    });
}

pub fn config_dir() -> PathBuf {
    data_root().join("config")
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn presets_file() -> PathBuf {
    config_dir().join("presets.json")
}

pub fn profiles_file() -> PathBuf {
    config_dir().join("profiles.json")
}

/// SQLite database for conversations and user state (see `state` module).
pub fn state_db_file() -> PathBuf {
    data_root().join("state.db")
}

pub fn bin_dir() -> PathBuf {
    data_root().join("bin")
}

pub fn logs_dir() -> PathBuf {
    data_root().join("logs")
}

pub fn default_models_dir() -> PathBuf {
    data_root().join("models")
}

/// Resolves a user-configured directory. Relative paths (legacy settings such as
/// `"models"`) are interpreted relative to the data root instead of the CWD.
pub fn resolve_dir(dir: &str) -> PathBuf {
    let p = PathBuf::from(dir);
    if p.is_absolute() {
        p
    } else {
        data_root().join(p)
    }
}

/// Lexically normalizes a path (resolves `.` and `..` without touching the disk).
pub fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonicalizes the longest existing prefix of `path` and appends the rest, so
/// paths that don't exist yet compare consistently with canonicalized ones
/// (e.g. the `\\?\` prefix on Windows).
pub(crate) fn canonicalize_lenient(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        normalize(path)
    } else {
        normalize(&std::env::current_dir().unwrap_or_default().join(path))
    };
    let mut existing = abs.as_path();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = fs::canonicalize(existing) {
            let mut out = c;
            for r in rest.iter().rev() {
                out.push(r);
            }
            return out;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => return abs,
        }
    }
}

/// Returns true if `path` is located inside `dir` (symlinks and `..` resolved).
/// A canonical path as people write it: without the Windows `\\?\` verbatim prefix.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

pub fn is_within(path: &Path, dir: &Path) -> bool {
    canonicalize_lenient(path).starts_with(canonicalize_lenient(dir))
}

/// Reduces an arbitrary user-supplied file name to a single safe path component.
/// Returns `None` if nothing usable remains.
pub fn sanitize_file_name(raw: &str) -> Option<String> {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') && !c.is_control())
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    let stem = cleaned.split('.').next().unwrap_or("");
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "NUL" | "AUX" | "PRN")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            });
    if cleaned.is_empty() || reserved {
        None
    } else {
        Some(cleaned)
    }
}

/// One-time migration of legacy CWD-relative data (`config/`, `bin/`) into the
/// data root. Existing files in the data root are never overwritten.
pub fn migrate_legacy_data() {
    let settings_dst = settings_file();
    if !settings_dst.exists() {
        let candidates = [
            PathBuf::from("config"),
            PathBuf::from("src-tauri").join("config"),
            PathBuf::from("..").join("config"),
        ];
        if let Some(src_dir) = candidates.iter().find(|c| c.join("settings.json").exists()) {
            let _ = fs::create_dir_all(config_dir());
            for name in ["settings.json", "presets.json"] {
                let src = src_dir.join(name);
                if src.exists() {
                    let _ = fs::copy(&src, config_dir().join(name));
                }
            }
            // Legacy model dirs were relative to the old CWD; pin them to absolute paths
            // so the user's existing library keeps working from the new location.
            let cwd = std::env::current_dir().unwrap_or_default();
            let _ = absolutize_legacy_model_dirs(&settings_dst, &cwd);
        }
    }

    let managed_dst = bin_dir().join("managed");
    let managed_src = PathBuf::from("bin").join("managed");
    if !managed_dst.exists() && managed_src.is_dir() {
        let _ = copy_dir_recursive(&managed_src, &managed_dst);
    }
}

fn absolutize_legacy_model_dirs(settings_path: &Path, base: &Path) -> Option<()> {
    let content = fs::read_to_string(settings_path).ok()?;
    let mut json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let fix = |v: &mut serde_json::Value| {
        if let Some(s) = v.as_str() {
            let p = PathBuf::from(s);
            if p.is_relative() {
                *v = serde_json::Value::String(
                    normalize(&base.join(p)).to_string_lossy().to_string(),
                );
            }
        }
    };
    if let Some(v) = json.get_mut("models_dir") {
        fix(v);
    }
    if let Some(arr) = json
        .get_mut("extra_models_dirs")
        .and_then(|v| v.as_array_mut())
    {
        arr.iter_mut().for_each(fix);
    }
    fs::write(settings_path, serde_json::to_string_pretty(&json).ok()?).ok()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_file_name() {
        assert_eq!(
            sanitize_file_name("model.gguf").as_deref(),
            Some("model.gguf")
        );
        assert_eq!(
            sanitize_file_name("../../evil.gguf").as_deref(),
            Some("evil.gguf")
        );
        assert_eq!(
            sanitize_file_name(r"..\..\evil.gguf").as_deref(),
            Some("evil.gguf")
        );
        assert_eq!(sanitize_file_name("a:b*?.gguf").as_deref(), Some("ab.gguf"));
        assert_eq!(sanitize_file_name(".."), None);
        assert_eq!(sanitize_file_name("  "), None);
        assert_eq!(sanitize_file_name("CON"), None);
        assert_eq!(sanitize_file_name("nul.gguf"), None);
        assert_eq!(sanitize_file_name("Com9.bin"), None);
        assert_eq!(sanitize_file_name("LPT1"), None);
        assert_eq!(
            sanitize_file_name("COM10.gguf").as_deref(),
            Some("COM10.gguf")
        );
    }

    #[test]
    fn test_is_within() {
        let root = std::env::temp_dir().join("harness_within_test");
        assert!(is_within(&root.join("a").join("b.gguf"), &root));
        assert!(!is_within(&root.join("..").join("x.gguf"), &root));
    }

    #[test]
    fn test_resolve_dir_relative_goes_to_data_root() {
        assert_eq!(resolve_dir("models"), data_root().join("models"));
        let abs = std::env::temp_dir().join("abs_models");
        assert_eq!(resolve_dir(abs.to_str().unwrap()), abs);
    }
}

/// `fs::rename` that waits out a briefly held handle. On Windows a virus scanner or the search
/// indexer opens freshly unpacked files for a moment, and moving their folder then fails with
/// "access denied"; the same move succeeds a second later. Other errors return at once.
pub fn rename_retrying(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempt < 20 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn rename_retrying_moves_a_folder_and_reports_a_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::create_dir(&a).unwrap();
        fs::write(a.join("x"), "1").unwrap();
        rename_retrying(&a, &b).unwrap();
        assert!(b.join("x").exists() && !a.exists());
        let e = rename_retrying(&a, &dir.path().join("c")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    }
}
