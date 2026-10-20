//! Offline cleanup shared by the desktop helper and the NSIS uninstaller.
//! Never infer ownership from a filename prefix in a shared directory.
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Re-downloadable files only. In particular Lemonade's models are user data.
pub const DISPOSABLE: &[&str] = &[
    "bin",
    "helpers",
    "cache",
    "logs",
    "slot-cache",
    "registry",
    "lemonade/cache",
    "lemonade/lemond.log",
];

#[derive(Debug, serde::Serialize)]
pub struct CleanupPlan {
    pub paths: Vec<PathBuf>,
    pub external_models: Vec<PathBuf>,
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

/// Refuse junctions/symlinks in every existing ancestor. Recursive deletion unlinks links
/// inside a directory; it must never start through a redirected ancestor.
pub fn check_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("Unsafe cleanup path: {}", path.display()));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(m) if redirected(&m) => {
                return Err(format!("Redirected cleanup path: {}", ancestor.display()));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
    }
    Ok(())
}

fn redirected(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    m.file_type().is_symlink()
}

/// Root must be the identifier's dedicated directory or an explicitly marked isolated root.
pub fn validate_root(root: &Path, identifier: &str) -> Result<(), String> {
    check_path(root)?;
    // A marked custom root must still never be a drive root, the home directory or an ancestor of it.
    if root.parent().is_none() || dirs::home_dir().is_some_and(|home| home.starts_with(root)) {
        return Err("Cleanup refuses a drive root or the home directory".into());
    }
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        || !identifier.contains('.')
        || (root.file_name().and_then(|s| s.to_str()) != Some(identifier)
            && fs::read_to_string(root.join(".harness-owned"))
                .ok()
                .as_deref()
                != Some(identifier))
    {
        return Err("Cleanup requires a dedicated Harness data directory".into());
    }
    Ok(())
}

pub fn plan(root: &Path, identifier: &str, delete_data: bool) -> Result<CleanupPlan, String> {
    validate_root(root, identifier)?;
    let paths = if delete_data {
        vec![root.to_path_buf()]
    } else {
        DISPOSABLE.iter().map(|p| root.join(p)).collect()
    };
    let settings = crate::config::AppSettings::load();
    let external_models = settings
        .all_models_dirs()
        .into_iter()
        .filter(|p| !crate::paths::is_within(p, root))
        .collect();
    Ok(CleanupPlan {
        paths,
        external_models,
    })
}

/// Symlinks and junctions are unlinked, never traversed, including nested reparse points.
pub fn remove_owned(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        check_path(parent)?;
    }
    let m = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io(e)),
    };
    if redirected(&m) {
        // Windows junctions and directory symlinks report as links, not directories.
        fs::remove_dir(path)
            .or_else(|_| fs::remove_file(path))
            .map_err(io)
    } else if m.is_dir() {
        for entry in fs::read_dir(path).map_err(io)? {
            remove_owned(&entry.map_err(io)?.path())?;
        }
        fs::remove_dir(path).map_err(io)
    } else {
        fs::remove_file(path).map_err(io)
    }
}

/// Run only after the desktop process has exited. Keep the lock inode while deleting contents.
pub fn execute(
    root: &Path,
    identifier: &str,
    delete_data: bool,
    external: &[PathBuf],
) -> Result<(), String> {
    let plan = plan(root, identifier, delete_data)?;
    for path in external {
        check_path(path)?;
        if !delete_data
            || !plan.external_models.contains(path)
            || path.parent().is_none_or(|p| p.parent().is_none())
            || dirs::home_dir().is_some_and(|home| home.starts_with(path))
            || root.starts_with(path)
            || std::env::current_exe().is_ok_and(|exe| exe.starts_with(path))
        {
            return Err(format!(
                "Unapproved external model directory: {}",
                path.display()
            ));
        }
    }
    if !root.exists() {
        return Ok(());
    }
    let lock = crate::instance::InstanceLock::acquire(
        root,
        crate::instance::InstanceKind::Headless,
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(|e| e.to_string())?;
    // Clear credentials before deleting the account ledger. Failure preserves the data for retry.
    if delete_data {
        crate::secrets::delete_all_owned()?;
    }
    if delete_data {
        for entry in fs::read_dir(root).map_err(io)? {
            let entry = entry.map_err(io)?;
            if entry.file_name() != crate::instance::LOCK_FILE {
                remove_owned(&entry.path())?;
            }
        }
    } else {
        for path in plan.paths {
            remove_owned(&path)?;
        }
    }
    for path in external {
        remove_owned(path)?;
    }
    drop(lock);
    if delete_data {
        remove_owned(&root.join(crate::instance::LOCK_FILE))?;
        fs::remove_dir(root).map_err(io)?;
    }
    Ok(())
}

/// The helper starts before the desktop exits; waiting also releases SQLite and WebView locks.
/// After 5 seconds the process is stopped so cleanup never hangs.
pub fn wait_for_exit(pid: u32) -> Result<(), String> {
    let mut system = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    for attempt in 0..70 {
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        let Some(process) = system.process(pid) else {
            return Ok(());
        };
        if attempt == 50 {
            process.kill();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("Harness is still running; cleanup was cancelled".into())
}

/// Web storage and OS caches use the bundle identifier, independently of HARNESS_DATA_DIR.
pub fn remove_platform_caches(identifier: &str, root: &Path) -> Result<(), String> {
    validate_root(root, identifier)?;
    let mut paths = Vec::new();
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        paths.push(local.join(identifier));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(cache) = dirs::cache_dir() {
            paths.push(cache.join(identifier));
        }
        if let Some(config) = dirs::config_dir() {
            paths.push(config.join(identifier));
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        for directory in ["Caches", "Logs", "WebKit", "HTTPStorages"] {
            paths.push(home.join("Library").join(directory).join(identifier));
        }
        paths.push(
            home.join("Library/Preferences")
                .join(format!("{identifier}.plist")),
        );
        paths.push(
            home.join("Library/Saved Application State")
                .join(format!("{identifier}.savedState")),
        );
    }
    for path in paths {
        if !crate::paths::is_within(&path, root) {
            remove_owned(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// macOS temp dirs live under the /var symlink, which check_path rightly refuses.
    fn base(temp: &tempfile::TempDir) -> PathBuf {
        if cfg!(target_os = "macos") {
            temp.path().canonicalize().unwrap()
        } else {
            temp.path().to_path_buf()
        }
    }
    #[test]
    fn allowlist_preserves_models_and_user_content() {
        let temp = tempfile::tempdir().unwrap();
        let root = base(&temp).join("com.harnessstudio.cleanup-test");
        fs::create_dir_all(&root).unwrap();
        for name in [
            "models",
            "config",
            "conversations",
            "outputs",
            "datasets",
            "lemonade/models",
        ] {
            let p = root.join(name);
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join("keep"), b"data").unwrap();
        }
        for name in DISPOSABLE {
            let p = root.join(name);
            fs::create_dir_all(&p).unwrap();
        }
        // Test the same deletion allowlist without accessing any process-global secret store.
        for p in DISPOSABLE {
            remove_owned(&root.join(p)).unwrap();
        }
        for name in [
            "models",
            "config",
            "conversations",
            "outputs",
            "datasets",
            "lemonade/models",
        ] {
            assert!(root.join(name).join("keep").is_file());
        }
        assert!(validate_root(&base(&temp), "com.harnessstudio.app").is_err());
        // A marker cannot legitimise the home directory or a drive root.
        if let Some(home) = dirs::home_dir() {
            assert!(validate_root(&home, "com.harnessstudio.app").is_err());
        }
        assert!(validate_root(
            Path::new(if cfg!(windows) { "C:\\" } else { "/" }),
            "com.harnessstudio.app"
        )
        .is_err());
        assert!(validate_root(&root.join("../escape"), "com.harnessstudio.cleanup-test").is_err());
    }
    #[test]
    fn deletion_unlinks_nested_links_without_following_them() {
        let temp = tempfile::tempdir().unwrap();
        let root = base(&temp).join("owned");
        let outside = base(&temp).join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep"), b"data").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        #[cfg(windows)]
        {
            // Junctions do not require Developer Mode or administrator privileges.
            assert!(std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(root.join("link"))
                .arg(&outside)
                .output()
                .unwrap()
                .status
                .success());
        }
        assert!(check_path(&root.join("link/keep")).is_err());
        remove_owned(&root).unwrap();
        assert!(outside.join("keep").is_file());
    }
    #[test]
    fn keep_data_cleanup_is_fast_and_skips_models() {
        let temp = tempfile::tempdir().unwrap();
        let root = base(&temp).join("com.harnessstudio.cleanup-speed");
        for dir in ["bin", "cache", "logs", "models"] {
            let p = root.join(dir);
            fs::create_dir_all(&p).unwrap();
            for i in 0..2000 {
                fs::write(p.join(format!("f{i}")), b"x").unwrap();
            }
        }
        // A models directory that cannot be read proves the cleanup never enters it.
        let start = std::time::Instant::now();
        for p in DISPOSABLE {
            remove_owned(&root.join(p)).unwrap();
        }
        println!("cleanup of 4000 files: {:?}", start.elapsed());
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        assert!(!root.join("bin").exists() && !root.join("cache").exists());
        assert_eq!(fs::read_dir(root.join("models")).unwrap().count(), 2000);
    }
}
