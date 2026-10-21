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

// ---------------------------------------------------------------------------------------------
// Selective removal: the uninstall experience lets the user pick what goes. Every category maps
// to app-owned paths below the validated data root (plus platform caches and credentials); app
// files are always removed by the OS uninstaller. Safety checks are the same as above.
// ---------------------------------------------------------------------------------------------

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Models,
    Conversations,
    McpServers,
    Skills,
    Tools,
    Agents,
    Engines,
    PythonEnvs,
    Caches,
    Settings,
}

/// Display order of the uninstall screen.
pub const ALL_CATEGORIES: [Category; 10] = [
    Category::Models,
    Category::Conversations,
    Category::McpServers,
    Category::Skills,
    Category::Tools,
    Category::Agents,
    Category::Engines,
    Category::PythonEnvs,
    Category::Caches,
    Category::Settings,
];

const MODULES_KEPT_BY_ENGINES: &str = "hermes";

impl Category {
    /// Re-downloadable categories are removed by default; user content is kept.
    pub fn default_selected(self) -> bool {
        matches!(
            self,
            Category::Agents | Category::Engines | Category::PythonEnvs | Category::Caches
        )
    }

    /// App-owned paths below `root` (never outside it). Platform caches are separate.
    pub fn paths(self, root: &Path) -> Vec<PathBuf> {
        let join = |names: &[&str]| names.iter().map(|n| root.join(n)).collect::<Vec<_>>();
        match self {
            Category::Models => join(&[
                "models",
                "lemonade/models",
                "config/model_sources.json",
                "config/model_hashes.json",
                "config/downloads.json",
            ]),
            Category::Conversations => join(&[
                "state.db",
                "state.db-wal",
                "state.db-shm",
                "state.db-journal",
                "outputs",
                "datasets",
            ]),
            Category::McpServers => join(&["config/mcp_servers.json"]),
            Category::Skills => join(&["skills"]),
            Category::Tools => join(&["tools", "config/tool_checks.json"]),
            Category::Agents => join(&["bin/modules/hermes", "agents"]),
            Category::Engines => {
                let mut paths = join(&[
                    "helpers",
                    "registry",
                    "config/runtime_capabilities.json",
                    "lemonade/bin",
                ]);
                // Everything in bin/ except the managed Python area and the agent module.
                for entry in fs::read_dir(root.join("bin"))
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let name = entry.file_name();
                    if name != "pyenv" && name != "modules" {
                        paths.push(entry.path());
                    }
                }
                for entry in fs::read_dir(root.join("bin/modules"))
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    if entry.file_name() != MODULES_KEPT_BY_ENGINES {
                        paths.push(entry.path());
                    }
                }
                paths
            }
            Category::PythonEnvs => join(&["bin/pyenv", "python", "uv", "venv", "envs"]),
            Category::Caches => join(&[
                "cache",
                "logs",
                "slot-cache",
                "lemonade/cache",
                "lemonade/lemond.log",
            ]),
            Category::Settings => join(&[
                "config/settings.json",
                "config/config.json",
                "config/presets.json",
                "config/profiles.json",
                "config/server_profiles.json",
                "config/integrations.json",
                "config/benchmarks.json",
                "config/external_models.json",
                "config/modules.json",
                "config/module-sizes.json",
            ]),
        }
    }
}

/// What the user asked to remove. `everything` removes the whole data directory.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Selection {
    #[serde(default)]
    pub everything: bool,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub external_models: Vec<PathBuf>,
}

impl Selection {
    pub fn effective(&self) -> Vec<Category> {
        ALL_CATEGORIES
            .iter()
            .copied()
            .filter(|c| self.everything || self.categories.contains(c))
            .collect()
    }
    fn removes_models(&self) -> bool {
        self.everything || self.categories.contains(&Category::Models)
    }
    fn removes_settings(&self) -> bool {
        self.everything || self.categories.contains(&Category::Settings)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryInfo {
    pub category: Category,
    /// `None` while the size is still being measured; ask again.
    pub bytes: Option<u64>,
    pub present: bool,
    pub default_selected: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Estimate {
    pub categories: Vec<CategoryInfo>,
    pub external_models: Vec<PathBuf>,
    pub complete: bool,
}

type SizeCache = std::sync::Mutex<std::collections::HashMap<PathBuf, u64>>;

fn size_cache() -> &'static SizeCache {
    static CACHE: std::sync::OnceLock<SizeCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Total size without following links. `None` when the deadline passes first.
fn measure(path: &Path, deadline: std::time::Instant) -> Option<u64> {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        if std::time::Instant::now() >= deadline {
            return None;
        }
        let Ok(m) = fs::symlink_metadata(&current) else {
            continue;
        };
        if redirected(&m) {
            continue;
        }
        if m.is_dir() {
            for entry in fs::read_dir(&current).into_iter().flatten().flatten() {
                stack.push(entry.path());
            }
        } else {
            total += m.len();
        }
    }
    Some(total)
}

/// Sizes already known from the module size cache (`config/module-sizes.json`).
fn saved_module_sizes(root: &Path) -> std::collections::HashMap<PathBuf, u64> {
    fs::read_to_string(root.join("config/module-sizes.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Fast estimate: cached numbers first, then walks until the budget is spent. Unfinished
/// categories report `None`; a later call continues with what earlier calls cached.
pub fn estimate(
    root: &Path,
    identifier: &str,
    budget: std::time::Duration,
) -> Result<Estimate, String> {
    validate_root(root, identifier)?;
    let deadline = std::time::Instant::now() + budget;
    let saved = saved_module_sizes(root);
    let mut complete = true;
    let mut categories = Vec::new();
    for category in ALL_CATEGORIES {
        let existing: Vec<PathBuf> = category
            .paths(root)
            .into_iter()
            .filter(|p| fs::symlink_metadata(p).is_ok())
            .collect();
        let mut bytes = Some(0u64);
        for path in &existing {
            let known = size_cache().lock().unwrap().get(path).copied();
            let size = known.or_else(|| saved.get(path).copied()).or_else(|| {
                let measured = measure(path, deadline)?;
                size_cache().lock().unwrap().insert(path.clone(), measured);
                Some(measured)
            });
            bytes = bytes.zip(size).map(|(a, b)| a + b);
            if bytes.is_none() {
                break;
            }
        }
        complete &= bytes.is_some();
        categories.push(CategoryInfo {
            category,
            bytes,
            present: !existing.is_empty(),
            default_selected: category.default_selected(),
        });
    }
    let external_models = crate::config::AppSettings::load()
        .all_models_dirs()
        .into_iter()
        .filter(|p| !crate::paths::is_within(p, root))
        .collect();
    Ok(Estimate {
        categories,
        external_models,
        complete,
    })
}

/// Progress of [`execute_selection`], one event per step.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Step {
    Started { step: String },
    Done { step: String },
    Failed { step: String, error: String },
}

/// Process-exe test: only files inside an owned directory count; installers are never matched.
pub fn is_owned_exe(exe: &Path, owned: &[PathBuf]) -> bool {
    let name = exe
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    !name.starts_with("uninst") && owned.iter().any(|dir| exe.starts_with(dir))
}

/// Processes to stop: owned executables, never this process and never threads. On Linux sysinfo
/// lists every thread as a process of its own with the same executable; `kill` on such an id
/// signals the whole thread group, so the uninstaller would otherwise kill itself.
fn owned_targets(system: &sysinfo::System, owned: &[PathBuf]) -> Vec<sysinfo::Pid> {
    let own = sysinfo::Pid::from_u32(std::process::id());
    system
        .processes()
        .iter()
        .filter(|(pid, process)| {
            **pid != own
                && process.thread_kind().is_none()
                && process.exe().is_some_and(|exe| is_owned_exe(exe, owned))
        })
        .map(|(pid, _)| *pid)
        .collect()
}

/// Stops Harness processes (the app, `harness serve`, engines under the data directory) and waits
/// at most `timeout` for them to exit. Returns how many were still alive when it gave up.
pub fn stop_processes(owned: &[PathBuf], timeout: std::time::Duration) -> usize {
    let mut system = sysinfo::System::new();
    let deadline = std::time::Instant::now() + timeout;
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let targets = owned_targets(&system, owned);
    for pid in &targets {
        if let Some(process) = system.process(*pid) {
            process.kill();
        }
    }
    while std::time::Instant::now() < deadline {
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&targets), true);
        if targets.iter().all(|pid| system.process(*pid).is_none()) {
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&targets), true);
    targets
        .iter()
        .filter(|pid| system.process(**pid).is_some())
        .count()
}

fn validate_external(root: &Path, selection: &Selection, plan: &CleanupPlan) -> Result<(), String> {
    for path in &selection.external_models {
        check_path(path)?;
        if !selection.removes_models()
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
    Ok(())
}

fn step_id(category: Category) -> String {
    serde_json::to_value(category)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Removes the selected categories, reporting each step. A failing step does not stop the
/// others; the first failure is returned at the end so the caller can show what remains.
/// Run only after the desktop process has exited.
pub fn execute_selection(
    root: &Path,
    identifier: &str,
    selection: &Selection,
    progress: &mut dyn FnMut(Step),
) -> Result<(), String> {
    // The plan is needed for its validation and the approved external folders.
    let plan = plan(root, identifier, selection.removes_models())?;
    validate_external(root, selection, &plan)?;
    if !root.exists() {
        return Ok(());
    }
    let lock = crate::instance::InstanceLock::acquire(
        root,
        crate::instance::InstanceKind::Headless,
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(|e| e.to_string())?;
    let mut first_error: Option<String> = None;
    let mut run = |step: String, work: &mut dyn FnMut() -> Result<(), String>| {
        progress(Step::Started { step: step.clone() });
        match work() {
            Ok(()) => progress(Step::Done { step }),
            Err(error) => {
                first_error.get_or_insert_with(|| error.clone());
                progress(Step::Failed { step, error });
            }
        }
    };
    for category in selection.effective() {
        run(step_id(category), &mut || {
            for path in category.paths(root) {
                remove_owned(&path)?;
            }
            if category == Category::Caches {
                remove_platform_caches(identifier, root)?;
            }
            if category == Category::Models {
                for path in &selection.external_models {
                    remove_owned(path)?;
                }
            }
            Ok(())
        });
    }
    if selection.removes_settings() {
        // Credentials before the account ledger; a failure keeps the ledger for a retry.
        run("credentials".into(), &mut || {
            crate::secrets::delete_all_owned()?;
            remove_owned(&root.join("config/secret-accounts.json"))
        });
    }
    if selection.everything {
        run("leftovers".into(), &mut || {
            for entry in fs::read_dir(root).map_err(io)? {
                let entry = entry.map_err(io)?;
                if entry.file_name() != crate::instance::LOCK_FILE {
                    remove_owned(&entry.path())?;
                }
            }
            Ok(())
        });
    }
    drop(lock);
    if selection.everything && first_error.is_none() {
        remove_owned(&root.join(crate::instance::LOCK_FILE))?;
        fs::remove_dir(root).map_err(io)?;
    }
    first_error.map_or(Ok(()), Err)
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

/// The per-user app data directory of the installed app (`<data dir>/<identifier>`). Only for this
/// root do the OS-level caches below belong to it; an isolated HARNESS_DATA_DIR copy shares the
/// bundle identifier with the real install and must never remove the real install's web view data.
pub fn is_default_root(root: &Path, identifier: &str) -> bool {
    dirs::data_dir().is_some_and(|dir| {
        crate::paths::normalize(&dir.join(identifier)) == crate::paths::normalize(root)
    })
}

/// Web storage and OS caches use the bundle identifier. Skipped unless `root` is the installed
/// app's own data directory.
pub fn remove_platform_caches(identifier: &str, root: &Path) -> Result<(), String> {
    validate_root(root, identifier)?;
    if !is_default_root(root, identifier) {
        return Ok(());
    }
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
    fn keep_data_cleanup_removes_only_disposable_dirs_and_skips_models() {
        let temp = tempfile::tempdir().unwrap();
        let root = base(&temp).join("com.harnessstudio.cleanup-speed");
        for dir in ["bin", "cache", "logs", "models"] {
            let p = root.join(dir);
            fs::create_dir_all(&p).unwrap();
            for i in 0..2000 {
                fs::write(p.join(format!("f{i}")), b"x").unwrap();
            }
        }
        // Behaviour, not wall-clock: the disposable directories are gone and the
        // models directory was never walked (all of its files are still there).
        // Speed is not asserted because it depends on machine load.
        for p in DISPOSABLE {
            remove_owned(&root.join(p)).unwrap();
        }
        assert!(!root.join("bin").exists() && !root.join("cache").exists());
        assert_eq!(fs::read_dir(root.join("models")).unwrap().count(), 2000);
    }

    #[test]
    fn categories_stay_below_root_and_split_user_content_from_engines() {
        let temp = tempfile::tempdir().unwrap();
        let root = base(&temp).join("com.harnessstudio.category-test");
        for dir in [
            "bin/pyenv",
            "bin/modules/hermes",
            "bin/modules/other",
            "bin/llama",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for category in ALL_CATEGORIES {
            for path in category.paths(&root) {
                assert!(path.starts_with(&root), "{category:?}: {}", path.display());
                assert!(!path.components().any(|c| matches!(c, Component::ParentDir)));
            }
        }
        let engines = Category::Engines.paths(&root);
        assert!(engines.contains(&root.join("bin/llama")));
        assert!(engines.contains(&root.join("bin/modules/other")));
        assert!(!engines.contains(&root.join("bin/pyenv")));
        assert!(!engines.contains(&root.join("bin/modules/hermes")));
        assert!(Category::PythonEnvs
            .paths(&root)
            .contains(&root.join("bin/pyenv")));
        assert!(Category::Agents
            .paths(&root)
            .contains(&root.join("bin/modules/hermes")));
        // User content is kept by default; re-downloadable things are removed.
        for kept in [
            Category::Models,
            Category::Conversations,
            Category::Skills,
            Category::Tools,
        ] {
            assert!(!kept.default_selected());
        }
        for removed in [Category::Engines, Category::PythonEnvs, Category::Caches] {
            assert!(removed.default_selected());
        }
        // No two categories claim the same path.
        let mut seen = std::collections::HashSet::new();
        for category in ALL_CATEGORIES {
            for path in category.paths(&root) {
                assert!(
                    seen.insert(path.clone()),
                    "{} claimed twice",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn selection_removes_only_chosen_categories() {
        let temp = tempfile::tempdir().unwrap();
        let root = base(&temp).join("com.harnessstudio.selection-test");
        for dir in ["models", "skills", "cache", "bin/llama", "bin/pyenv"] {
            fs::create_dir_all(root.join(dir)).unwrap();
            fs::write(root.join(dir).join("f"), b"x").unwrap();
        }
        let selection = Selection {
            categories: vec![Category::Caches, Category::Engines],
            ..Default::default()
        };
        assert_eq!(
            selection.effective(),
            vec![Category::Engines, Category::Caches]
        );
        for category in selection.effective() {
            for path in category.paths(&root) {
                remove_owned(&path).unwrap();
            }
        }
        assert!(!root.join("cache").exists() && !root.join("bin/llama").exists());
        for kept in ["models", "skills", "bin/pyenv"] {
            assert!(root.join(kept).join("f").is_file(), "{kept}");
        }
    }

    #[test]
    fn external_models_need_the_models_category() {
        let temp = tempfile::tempdir().unwrap();
        let root = base(&temp).join("com.harnessstudio.external-test");
        fs::create_dir_all(&root).unwrap();
        let external = base(&temp).join("elsewhere/models");
        fs::create_dir_all(&external).unwrap();
        let plan = CleanupPlan {
            paths: vec![],
            external_models: vec![external.clone()],
        };
        let without = Selection {
            categories: vec![Category::Caches],
            external_models: vec![external.clone()],
            ..Default::default()
        };
        assert!(validate_external(&root, &without, &plan).is_err());
        let with = Selection {
            categories: vec![Category::Models],
            external_models: vec![external],
            ..Default::default()
        };
        assert!(validate_external(&root, &with, &plan).is_ok());
        let unlisted = Selection {
            everything: true,
            external_models: vec![base(&temp).join("elsewhere/other")],
            ..Default::default()
        };
        assert!(validate_external(&root, &unlisted, &plan).is_err());
    }

    /// Linux lists threads as processes; killing one id kills the whole group, so the uninstaller
    /// must never target its own threads.
    #[cfg(target_os = "linux")]
    #[test]
    fn process_stop_never_targets_own_threads() {
        let (stop, wait) = std::sync::mpsc::channel::<()>();
        let helper = std::thread::spawn(move || {
            let _ = wait.recv();
        });
        let exe = std::env::current_exe().unwrap();
        let owned = vec![exe.parent().unwrap().to_path_buf()];
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let targets = owned_targets(&system, &owned);
        let own_tasks: Vec<u32> = fs::read_dir("/proc/self/task")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .collect();
        assert!(own_tasks.len() > 1);
        for task in own_tasks {
            assert!(!targets.contains(&sysinfo::Pid::from_u32(task)));
        }
        drop(stop);
        helper.join().unwrap();
    }

    #[test]
    fn process_stop_only_matches_owned_directories() {
        let dir = base(&tempfile::tempdir().unwrap()).join("Harness");
        let owned = vec![dir.clone()];
        assert!(is_owned_exe(&dir.join("harness-studio"), &owned));
        assert!(!is_owned_exe(&dir.join("uninstall.exe"), &owned));
        assert!(!is_owned_exe(
            &dir.with_file_name("Other").join("app"),
            &owned
        ));
    }

    #[test]
    fn estimate_is_fast_and_reports_unfinished_sizes() {
        let temp = tempfile::tempdir().unwrap();
        let id = "com.harnessstudio.estimate-test";
        let root = base(&temp).join(id);
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::write(root.join("cache/f"), vec![0u8; 1234]).unwrap();
        let start = std::time::Instant::now();
        let done = estimate(&root, id, std::time::Duration::from_secs(1)).unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        let caches = done
            .categories
            .iter()
            .find(|c| c.category == Category::Caches)
            .unwrap();
        assert_eq!(caches.bytes, Some(1234));
        assert!(caches.present && done.complete);
        // An elapsed deadline cannot finish a new walk; the size is reported as pending.
        fs::create_dir_all(root.join("skills")).unwrap();
        fs::write(root.join("skills/s"), b"x").unwrap();
        let pending = estimate(&root, id, std::time::Duration::ZERO).unwrap();
        let skills = pending
            .categories
            .iter()
            .find(|c| c.category == Category::Skills)
            .unwrap();
        assert!(skills.bytes.is_none() && !pending.complete);
    }

    #[test]
    fn isolated_roots_never_touch_the_installed_apps_platform_caches() {
        let temp = tempfile::tempdir().unwrap();
        let root = base(&temp).join("com.harnessstudio.isolated-test");
        fs::create_dir_all(&root).unwrap();
        assert!(!is_default_root(&root, "com.harnessstudio.isolated-test"));
        if let Some(data) = dirs::data_dir() {
            assert!(is_default_root(
                &data.join("com.harnessstudio.app"),
                "com.harnessstudio.app"
            ));
            assert!(!is_default_root(
                &data.join("com.harnessstudio.app"),
                "com.harnessstudio.other"
            ));
        }
        // Same identifier as the real app, but a marked custom root: nothing outside it is removed.
        fs::write(root.join(".harness-owned"), "com.harnessstudio.app").unwrap();
        assert!(remove_platform_caches("com.harnessstudio.app", &root).is_ok());
    }
}
