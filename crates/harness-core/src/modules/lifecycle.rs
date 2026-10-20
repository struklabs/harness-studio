//! Durable runtime replacement. Backups stay at a sibling path so rollback restores venv paths.
use super::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Folder sizes for the module list. A walk can take seconds (an external agent install is a
/// whole Python environment), so it runs on its own thread and the list never waits for it
/// except briefly for a size that an install or removal just invalidated. Sizes are kept in
/// `module-sizes.json`, so the first list after a restart already shows the last known numbers
/// (served as aged while a refresh runs); a folder never measured reads 0 until its walk ends.
/// [`sizes_pending`] tells the list whether any number may still change.
#[derive(Default)]
struct DiskCache {
    sizes: BTreeMap<PathBuf, Measured>,
    measuring: BTreeSet<PathBuf>,
}
#[derive(Clone, Copy)]
struct Measured {
    /// `None`: restored from disk, so its age is unknown and a refresh always runs.
    at: Option<Instant>,
    bytes: u64,
    changed: bool,
}
static DISK_CACHE: OnceLock<Mutex<DiskCache>> = OnceLock::new();
const SIZE_FRESH: Duration = Duration::from_secs(15);
const SIZE_WAIT: Duration = Duration::from_millis(250);
fn sizes_file() -> PathBuf {
    paths::config_dir().join("module-sizes.json")
}
fn disk_cache() -> &'static Mutex<DiskCache> {
    DISK_CACHE.get_or_init(|| {
        let saved: BTreeMap<PathBuf, u64> = JsonStore::new(sizes_file()).load().unwrap_or_default();
        Mutex::new(DiskCache {
            sizes: saved
                .into_iter()
                .map(|(path, bytes)| {
                    (
                        path,
                        Measured {
                            at: None,
                            bytes,
                            changed: false,
                        },
                    )
                })
                .collect(),
            measuring: BTreeSet::new(),
        })
    })
}
fn persist_sizes(cache: &DiskCache) {
    let saved: BTreeMap<&PathBuf, u64> = cache.sizes.iter().map(|(p, m)| (p, m.bytes)).collect();
    let _ = JsonStore::new(sizes_file()).save(&saved);
}
/// Set whenever a listing served a size that may still change (never measured, aged, or
/// invalidated with its walk unfinished). The list reports it so the UI asks again soon.
static PROVISIONAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(super) fn begin_listing() {
    PROVISIONAL.store(false, std::sync::atomic::Ordering::SeqCst);
}
pub(super) fn listing_provisional() -> bool {
    PROVISIONAL.load(std::sync::atomic::Ordering::SeqCst)
}
fn cached_size(path: &Path) -> u64 {
    cached_size_with(disk_cache(), path, SIZE_WAIT, size)
}
fn cached_size_with(
    cache: &'static Mutex<DiskCache>,
    path: &Path,
    wait: Duration,
    measure: fn(&Path) -> u64,
) -> u64 {
    let mut waiting = None;
    let known = {
        let mut guard = cache.lock().unwrap();
        let known = guard.sizes.get(path).copied();
        if known.is_some_and(|m| !m.changed && m.at.is_some_and(|at| at.elapsed() < SIZE_FRESH)) {
            return known.unwrap().bytes;
        }
        PROVISIONAL.store(true, std::sync::atomic::Ordering::SeqCst);
        if guard.measuring.insert(path.to_path_buf()) {
            let target = path.to_path_buf();
            let (tx, rx) = std::sync::mpsc::channel();
            waiting = Some(rx);
            std::thread::spawn(move || {
                let bytes = measure(&target);
                let mut guard = cache.lock().unwrap();
                guard.measuring.remove(&target);
                guard.sizes.insert(
                    target,
                    Measured {
                        at: Some(Instant::now()),
                        bytes,
                        changed: false,
                    },
                );
                if DISK_CACHE
                    .get()
                    .is_some_and(|global| std::ptr::eq(global, cache))
                {
                    persist_sizes(&guard);
                }
                let _ = tx.send(bytes);
            });
        }
        known
    };
    match known {
        Some(m) if !m.changed => m.bytes,
        // Invalidated by an install or removal: give the new walk a moment to finish.
        Some(m) => match waiting.and_then(|rx| rx.recv_timeout(wait).ok()) {
            Some(bytes) => bytes,
            None => m.bytes,
        },
        // Never measured: the list does not wait for it; the walk lands for the next one.
        None => 0,
    }
}
/// Marks every size as outdated after a change.
fn invalidate_disk_cache() {
    if let Some(cache) = DISK_CACHE.get() {
        for m in cache.lock().unwrap().sizes.values_mut() {
            m.changed = true;
        }
    }
    super::invalidate_agent_probe();
}
pub(super) struct RefreshDisk;
impl Drop for RefreshDisk {
    fn drop(&mut self) {
        invalidate_disk_cache();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    id: String,
    previous: Box<ModuleStore>,
    had_runtime: bool,
    #[serde(default)]
    shared: Vec<SharedRuntime>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SharedRuntime {
    pub(super) relative: PathBuf,
    pub(super) existed: bool,
}
fn shared_path(root: &Path, entry: &SharedRuntime) -> Result<PathBuf, String> {
    let parts: Vec<_> = entry.relative.components().collect();
    if parts.is_empty()
        || parts.len() > 2
        || parts
            .iter()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
    {
        return Err("modules.unsafe_path".into());
    }
    let name = entry.relative.file_name().unwrap().to_string_lossy();
    let allowed = entry.relative == Path::new("python")
        || entry.relative.parent() == Some(Path::new("uv")) && version(&name).is_some()
        || entry.relative.parent() == Some(Path::new("python"))
            && name
                .strip_prefix("cpython-")
                .and_then(|v| v.split('-').next())
                .and_then(version)
                .is_some();
    if !allowed {
        return Err("modules.unsafe_path".into());
    }
    Ok(root.join(&entry.relative))
}
pub(super) fn shared_backup(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(".module-rollback");
    path.with_file_name(name)
}
#[derive(Default, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RemovalOptions {
    #[serde(default)]
    pub delete_history: bool,
    #[serde(default)]
    pub delete_logs: bool,
}
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DataChoice {
    pub id: String,
    pub disk_bytes: u64,
    pub path: String,
}
pub(super) fn runtime(id: &str) -> PathBuf {
    if id == "agents" {
        agents::managed_root()
    } else {
        envs::env_dir_in(&envs::root(), ENV)
    }
}
pub(super) fn backup(id: &str) -> PathBuf {
    runtime(id).with_extension("rollback")
}
pub(super) fn erase(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
        Ok(m) if m.file_type().is_symlink() => Err("modules.unsafe_path".into()),
        Ok(m) if m.is_file() => std::fs::remove_file(path).map_err(|e| e.to_string()),
        Ok(_) => std::fs::remove_dir_all(path).map_err(|e| e.to_string()),
    }
}
fn begin(data: &mut ModuleStore, id: &str) -> Result<(), String> {
    begin_with_shared(data, id, vec![])
}
fn begin_with_shared(
    data: &mut ModuleStore,
    id: &str,
    shared: Vec<SharedRuntime>,
) -> Result<(), String> {
    if !shared.is_empty() {
        ensure_path_unused(&envs::root())?;
    }
    begin_at(
        data,
        id,
        &runtime(id),
        &backup(id),
        &store(),
        &envs::root(),
        shared,
    )
}
pub(super) fn begin_at(
    data: &mut ModuleStore,
    id: &str,
    path: &Path,
    backup: &Path,
    file: &JsonStore,
    shared_root: &Path,
    shared: Vec<SharedRuntime>,
) -> Result<(), String> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("modules.unsafe_path".into());
    }
    if backup.exists() {
        erase(backup)?;
    } // Retired backup after a committed transaction.
    for entry in &shared {
        let target = shared_path(shared_root, entry)?;
        if std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("modules.unsafe_path".into());
        }
        erase(&shared_backup(&target))?;
    }
    data.transaction = Some(Transaction {
        id: id.into(),
        previous: Box::new(data.clone()),
        had_runtime: path.exists(),
        shared,
    });
    file.save(data)?;
    // All paths and their original existence are journaled before moving the first one.
    for entry in &data.transaction.as_ref().unwrap().shared {
        let target = shared_path(shared_root, entry)?;
        if entry.existed {
            std::fs::rename(&target, shared_backup(&target)).map_err(|e| e.to_string())?;
        }
    }
    if path.exists() {
        std::fs::rename(path, backup).map_err(|e| e.to_string())?;
    }
    data.modules.entry(id.into()).or_default().status = Status::Installing;
    file.save(data)
}

fn restore_runtime(path: &Path, backup: &Path, had_runtime: bool) -> Result<(), String> {
    if backup.exists() {
        erase(path)?;
        std::fs::rename(backup, path).map_err(|e| e.to_string())?;
    } else if !had_runtime {
        erase(path)?;
    }
    Ok(())
}

pub(super) fn rollback(data: &mut ModuleStore) -> Result<(), String> {
    let Some(tx) = data.transaction.clone() else {
        return Ok(());
    };
    // IDs from disk must resolve to a bundled backend, never arbitrary paths.
    backend(&tx.id)?;
    unused(&tx.id)?;
    if !tx.shared.is_empty() {
        ensure_path_unused(&envs::root())?;
    }
    rollback_at(
        data,
        &runtime(&tx.id),
        &backup(&tx.id),
        &store(),
        &envs::root(),
    )
}
pub(super) fn rollback_at(
    data: &mut ModuleStore,
    path: &Path,
    backup: &Path,
    file: &JsonStore,
    shared_root: &Path,
) -> Result<(), String> {
    let Some(tx) = data.transaction.clone() else {
        return Ok(());
    };
    for entry in tx.shared.iter().rev() {
        let target = shared_path(shared_root, entry)?;
        restore_runtime(&target, &shared_backup(&target), entry.existed)?;
    }
    restore_runtime(path, backup, tx.had_runtime)?;
    *data = *tx.previous;
    file.save(data)
}
pub(super) fn recover(data: &mut ModuleStore) -> Result<(), String> {
    if let Ok(_operation) = OPERATION.try_lock() {
        if data.transaction.is_some() {
            let Ok(_build) = envs::BUILD_LOCK.try_lock() else {
                return Ok(());
            };
            rollback(data)?;
            invalidate_disk_cache();
        }
    }
    Ok(())
}
pub fn ensure_enabled(id: &str) -> Result<(), String> {
    let data = load()?;
    if data.transaction.as_ref().is_some_and(|tx| tx.id == id)
        || data.modules.get(id).is_some_and(|s| {
            matches!(
                s.status,
                Status::Disabled | Status::Installing | Status::NotInstalled
            )
        })
    {
        return Err("modules.disabled".into());
    }
    Ok(())
}
pub(super) fn unused(id: &str) -> Result<(), String> {
    if id == "agents" && agents::task_status().status == "running" {
        return Err("modules.in_use".into());
    }
    ensure_path_unused(&runtime(id))
}
pub(super) fn ensure_path_unused(dir: &Path) -> Result<(), String> {
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing()
            .with_exe(sysinfo::UpdateKind::Always)
            .with_cmd(sysinfo::UpdateKind::Always)
            .with_cwd(sysinfo::UpdateKind::Always),
    );
    if system.processes().values().any(|p| {
        p.exe().is_some_and(|exe| exe.starts_with(dir))
            || p.cwd().is_some_and(|cwd| cwd.starts_with(dir))
            || p.cmd().iter().any(|arg| Path::new(arg).starts_with(dir))
    }) {
        return Err("modules.in_use".into());
    }
    Ok(())
}
pub(super) fn component_bytes(id: &str) -> u64 {
    let path = match id {
        "transcribe-cpp" => runtime("speech"),
        "hermes-agent" => {
            if runtime("agents").exists() || backup("agents").exists() {
                runtime("agents")
            } else {
                agents::external_runtime().unwrap_or_else(|| runtime("agents"))
            }
        }
        "managed-uv" => envs::root().join("uv"),
        "managed-python" => envs::root().join("python"),
        _ => return 0,
    };
    let bytes = cached_size(&path);
    match id {
        "transcribe-cpp" => bytes.saturating_add(cached_size(&backup("speech"))),
        "hermes-agent" => bytes.saturating_add(cached_size(&backup("agents"))),
        _ => bytes,
    }
}
// Never follow directory symlinks into models or external user folders.
fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| size(&e.path()))
        .sum()
}
fn account(m: &ModuleManifest, measure: impl Fn(&str) -> u64) -> (u64, u64) {
    let mut exclusive = 0u64;
    let mut shared = 0u64;
    let mut counted = BTreeSet::new();
    for component in &m.components {
        if !counted.insert(&component.id) {
            continue;
        }
        let target = if m.shared_component_ids.contains(&component.id) {
            &mut shared
        } else {
            &mut exclusive
        };
        *target = target.saturating_add(measure(&component.id));
    }
    (exclusive, shared)
}
pub(super) fn module_bytes(m: &ModuleManifest) -> u64 {
    account(m, component_bytes).0
}
pub(super) fn shared_bytes(m: &ModuleManifest) -> u64 {
    account(m, component_bytes).1
}

pub(super) fn update_available(data: &ModuleStore, m: &ModuleManifest) -> bool {
    data.modules.get(&m.id).is_some_and(|s| {
        s.installed_version
            .as_ref()
            .is_some_and(|v| version(v) < version(&m.version))
    }) || m.components.iter().any(|c| {
        data.components
            .get(&c.id)
            .is_some_and(|old| old.version != c.version)
    })
}
pub(super) fn data_choices(id: &str) -> Vec<DataChoice> {
    if id != "agents" {
        return vec![];
    }
    ["history", "logs"]
        .into_iter()
        .map(|id| DataChoice {
            id: id.into(),
            disk_bytes: data_paths(id).iter().map(|p| cached_size(p)).sum(),
            path: agents::home()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        })
        .collect()
}
fn data_paths(id: &str) -> Vec<PathBuf> {
    let Ok(home) = agents::home() else {
        return vec![];
    };
    let names: &[&str] = if id == "history" {
        &["sessions", "state.db", "state.db-wal", "state.db-shm"]
    } else {
        &["logs"]
    };
    names.iter().map(|name| home.join(name)).collect()
}

fn preflight(m: &ModuleManifest) -> Result<(), String> {
    std::fs::create_dir_all(runtime(&m.id).parent().unwrap()).map_err(|e| e.to_string())?;
    let (free, total) =
        crate::storage::StorageManager::get_directory_disk_space(runtime(&m.id).parent().unwrap());
    check_space(m, free, total)
}
fn check_space(m: &ModuleManifest, free: u64, total: u64) -> Result<(), String> {
    let required = m.components.iter().fold(128u64 * 1024 * 1024, |bytes, c| {
        bytes
            .saturating_add(c.disk_bytes)
            .saturating_add(c.download_bytes)
    });
    if total == 0 || free < required {
        return Err("modules.disk_space".into());
    }
    Ok(())
}
/// All slow filesystem work and probes run on a dedicated worker, never on the UI executor.
pub async fn action(id: &str, action: &str) -> Result<(), String> {
    let id = id.to_string();
    let action = action.to_string();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || handle.block_on(run(&id, &action)))
        .await
        .map_err(|e| e.to_string())?
}
async fn run(id: &str, action: &str) -> Result<(), String> {
    let manifest = installable(id)?;
    let _refresh_disk = RefreshDisk;
    let _operation = OPERATION
        .try_lock()
        .map_err(|_| "modules.busy".to_string())?;
    let _build = envs::BUILD_LOCK
        .try_lock()
        .map_err(|_| "modules.busy".to_string())?;
    let previous = {
        let _lock = STATE_LOCK.lock().unwrap();
        let mut data = load()?;
        rollback(&mut data)?;
        data
    };
    if !matches!(
        action,
        "install" | "update" | "repair" | "enable" | "disable"
    ) {
        return Err("modules.unknown".into());
    }
    unused(id)?;
    if id == "speech"
        && envs::status_in_quick(&envs::root(), &envs::TRANSCRIBE_CPP)
            .progress
            .is_some()
    {
        return Err("modules.busy".into());
    }
    if action == "enable" {
        verify(id).await?;
    }
    if matches!(action, "enable" | "disable") {
        let _lock = STATE_LOCK.lock().unwrap();
        let mut data = load()?;
        let state = data.modules.get_mut(id).ok_or("modules.not_installed")?;
        if !matches!(state.status, Status::Installed | Status::Disabled) {
            return Err("modules.interrupted".into());
        }
        if action == "disable" && id == "agents" {
            agents::disconnect()?;
        }
        state.status = if action == "enable" {
            Status::Installed
        } else {
            Status::Disabled
        };
        return store().save(&data);
    }
    if action == "update" && !update_available(&previous, &manifest) {
        return Err("modules.no_update".into());
    }
    if action == "install"
        && previous
            .modules
            .get(id)
            .is_some_and(|s| matches!(s.status, Status::Installed | Status::Disabled))
    {
        return Ok(());
    }
    let shared = if id == "speech" {
        broken_shared().await?
    } else {
        vec![]
    };
    if !shared.is_empty() {
        ensure_path_unused(&envs::root())?;
        for entry in &shared {
            let component = if entry.relative.starts_with("uv") {
                "managed-uv"
            } else {
                "managed-python"
            };
            if previous
                .components
                .get(component)
                .is_some_and(|c| c.owners.iter().any(|owner| owner != id))
            {
                return Err("modules.shared_conflict".into());
            }
        }
    }
    // Re-verify first. Healthy repair needs no network or filesystem mutation.
    if action == "repair"
        && shared.is_empty()
        && !update_available(&previous, &manifest)
        && verify(id).await.is_ok()
    {
        let _lock = STATE_LOCK.lock().unwrap();
        let mut data = load()?;
        data.claim(&manifest);
        if previous
            .modules
            .get(id)
            .is_some_and(|s| s.status == Status::Disabled)
        {
            data.modules.get_mut(id).unwrap().status = Status::Disabled;
        }
        return store().save(&data);
    }
    for c in &manifest.components {
        if previous
            .components
            .get(&c.id)
            .is_some_and(|old| old.owners.iter().any(|o| o != id) && old.version != c.version)
        {
            return Err("modules.shared_conflict".into());
        }
    }
    preflight(&manifest)?;
    // Check the actual package source before moving the working installation.
    let url = if id == "agents" {
        "https://hermes-agent.nousresearch.com/install.ps1"
    } else {
        "https://pypi.org/simple/transcribe-cpp/"
    };
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| "modules.offline".to_string())?;
    {
        let _lock = STATE_LOCK.lock().unwrap();
        let mut data = load()?;
        unused(id)?; // Re-check after network preflight before moving any runtime.
        if let Err(error) = begin_with_shared(&mut data, id, shared) {
            rollback(&mut data)?;
            return Err(error);
        }
    }
    let result = if id == "agents" {
        agents::install_managed().await
    } else {
        envs::install_locked(ENV).await.map(|_| ())
    };
    let result = match result {
        Ok(()) => verify(id).await,
        Err(e) => Err(e),
    };
    let _lock = STATE_LOCK.lock().unwrap();
    let mut data = load()?;
    match result {
        Ok(()) => {
            let shared = data
                .transaction
                .as_ref()
                .map(|tx| tx.shared.clone())
                .unwrap_or_default();
            data.claim(&manifest);
            if previous
                .modules
                .get(id)
                .is_some_and(|s| s.status == Status::Disabled)
            {
                data.modules.get_mut(id).unwrap().status = Status::Disabled;
            }
            data.transaction = None;
            if let Err(error) = store().save(&data) {
                data = load()?;
                rollback(&mut data)?;
                return Err(error);
            }
            erase(&backup(id))?;
            for entry in shared {
                erase(&shared_backup(&shared_path(&envs::root(), &entry)?))?;
            }
            Ok(())
        }
        Err(error) => {
            rollback(&mut data).map_err(|_| "modules.rollback_failed".to_string())?;
            let state = data.modules.entry(id.into()).or_default();
            if state.installed_version.is_none() {
                state.status = Status::Failed;
            }
            state.error = Some(error.clone());
            store().save(&data)?;
            Err(error)
        }
    }
}
async fn probe_support(path: &Path, python: bool) -> bool {
    let mut command = tokio::process::Command::new(path);
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if python {
        command.args(["-I", "-c", "import ssl, sqlite3"]);
    } else {
        command.arg("--version");
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    matches!(tokio::time::timeout(Duration::from_secs(15), command.status()).await, Ok(Ok(status)) if status.success())
}
async fn broken_shared() -> Result<Vec<SharedRuntime>, String> {
    let root = envs::root();
    let uv = root.join("uv").join(pyenv::UV_VERSION);
    let uv_binary = uv.join(if cfg!(windows) { "uv.exe" } else { "uv" });
    let mut damaged = Vec::new();
    if !probe_support(&uv_binary, false).await {
        damaged.push(SharedRuntime {
            relative: PathBuf::from("uv").join(pyenv::UV_VERSION),
            existed: uv.exists(),
        });
    }
    let python = root.join("python");
    if !python.exists() {
        damaged.push(SharedRuntime {
            relative: "python".into(),
            existed: false,
        });
    } else {
        let entries = std::fs::read_dir(&python).map_err(|e| e.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            if !name
                .to_string_lossy()
                .starts_with(&format!("cpython-{}.", pyenv::PYTHON_SERIES))
                || name.to_string_lossy().ends_with(".module-rollback")
                || !entry.file_type().map_err(|e| e.to_string())?.is_dir()
            {
                continue;
            }
            let exe = entry.path().join(if cfg!(windows) {
                "python.exe"
            } else {
                "bin/python3"
            });
            if !probe_support(&exe, true).await {
                damaged.push(SharedRuntime {
                    relative: PathBuf::from("python").join(name),
                    existed: true,
                });
            }
        }
    }
    Ok(damaged)
}

pub(super) async fn verify(id: &str) -> Result<(), String> {
    if id == "agents" {
        return agents::verify();
    }
    let python = envs::python_in(&envs::root(), ENV);
    let mut cmd = tokio::process::Command::new(python);
    cmd.args(["-c", envs::TRANSCRIBE_CPP.verify])
        .envs(pyenv::python_env(&runtime(id)))
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let status = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.status())
        .await
        .map_err(|_| "modules.interrupted")?
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("modules.interrupted".into())
    }
}
pub fn remove_with_options(id: &str, options: &RemovalOptions) -> Result<(), String> {
    installable(id)?;
    let _refresh_disk = RefreshDisk;
    let _operation = OPERATION
        .try_lock()
        .map_err(|_| "modules.busy".to_string())?;
    let _build = envs::BUILD_LOCK
        .try_lock()
        .map_err(|_| "modules.busy".to_string())?;
    let _lock = STATE_LOCK.lock().unwrap();
    let mut data = load()?;
    rollback(&mut data)?;
    unused(id)?;
    if id == "speech"
        && envs::status_in_quick(&envs::root(), &envs::TRANSCRIBE_CPP)
            .progress
            .is_some()
    {
        return Err("modules.busy".into());
    }
    if id == "agents" {
        if options.delete_history || options.delete_logs {
            // External Hermes processes may also hold the same history database open.
            ensure_path_unused(&agents::home()?)?;
            if let Some(root) = agents::external_runtime() {
                ensure_path_unused(&root)?;
            }
        }
        agents::disconnect()?;
    }
    let mut next = data.clone();
    let unused = next.release(id);
    let backend_id = if id == "agents" { "hermes-agent" } else { ENV };
    let removing_runtime = unused.iter().any(|c| c == backend_id);
    if removing_runtime {
        if let Err(error) = begin(&mut data, id) {
            rollback(&mut data)?;
            return Err(error);
        }
    }
    // Shared Python and uv also support non-module engines; retain them even with zero module owners.
    if let Err(error) = store().save(&next) {
        if removing_runtime {
            rollback(&mut data)?;
        }
        return Err(error);
    }
    if removing_runtime {
        erase(&backup(id))?;
    }
    if id == "agents" {
        if options.delete_history {
            for path in data_paths("history") {
                erase(&path)?;
            }
            agents::clear_history();
        }
        if options.delete_logs {
            for path in data_paths("logs") {
                erase(&path)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_replacement_restores_runtime_and_first_install_leaves_no_partial_files() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("runtime");
        let backup = temp.path().join("runtime.rollback");
        std::fs::create_dir(&current).unwrap();
        std::fs::write(current.join("working"), "old working backend").unwrap();
        std::fs::rename(&current, &backup).unwrap();
        std::fs::create_dir(&current).unwrap();
        std::fs::write(current.join("partial"), "broken update").unwrap();
        restore_runtime(&current, &backup, true).unwrap();
        assert_eq!(
            std::fs::read_to_string(current.join("working")).unwrap(),
            "old working backend"
        );
        assert!(!current.join("partial").exists());
        // Replaying recovery after a crash is safe.
        restore_runtime(&current, &backup, true).unwrap();
        assert!(current.join("working").exists());
        restore_runtime(&current, &backup, false).unwrap();
        assert!(!current.exists());
    }
    #[test]
    fn journal_retains_disabled_state_versions_and_component_owners() {
        let manifest = bundled().remove(0);
        let mut data = ModuleStore::default();
        data.claim(&manifest);
        data.modules.get_mut("speech").unwrap().status = Status::Disabled;
        let previous = data.clone();
        data.transaction = Some(Transaction {
            id: "speech".into(),
            previous: Box::new(previous.clone()),
            had_runtime: true,
            shared: vec![],
        });
        data.modules.get_mut("speech").unwrap().status = Status::Installing;
        let saved: ModuleStore =
            serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(*saved.transaction.unwrap().previous, previous);
    }
    #[test]
    fn disk_accounting_counts_nested_files_once_and_missing_paths_as_zero() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("shared")).unwrap();
        std::fs::write(temp.path().join("private"), [0; 13]).unwrap();
        std::fs::write(temp.path().join("shared/python"), [0; 29]).unwrap();
        assert_eq!(size(temp.path()), 42);
        assert_eq!(size(&temp.path().join("shared")), 29);
        assert_eq!(size(&temp.path().join("missing")), 0);
        let m = bundled().remove(0);
        let mut data = ModuleStore::default();
        data.claim(&m);
        assert!(!update_available(&data, &m));
        data.components.get_mut(ENV).unwrap().version = "old-lock".into();
        assert!(update_available(&data, &m));
        reconcile(&mut data, &m, true, false);
        assert_eq!(data.components[ENV].version, "old-lock");
    }
    #[test]
    fn releasing_one_owner_never_releases_another_modules_component() {
        let first = bundled().remove(0);
        let mut second = first.clone();
        second.id = "second".into();
        let mut data = ModuleStore::default();
        data.claim(&first);
        data.claim(&second);
        data.modules.get_mut("speech").unwrap().status = Status::Disabled;
        assert!(data.release("speech").is_empty());
        assert_eq!(
            data.components[ENV].owners,
            BTreeSet::from(["second".into()])
        );
        assert!(data.release("second").contains(&ENV.to_string()));
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    #[test]
    fn interrupted_update_replays_journal_with_real_files_and_all_owners() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("speech");
        let backup = temp.path().join("speech.rollback");
        let file = JsonStore::new(temp.path().join("modules.json"));
        let manifest = bundled().remove(0);
        let mut data = ModuleStore::default();
        data.claim(&manifest);
        let mut other = manifest.clone();
        other.id = "other".into();
        data.claim(&other);
        data.modules.get_mut("speech").unwrap().status = Status::Disabled;
        let expected = data.clone();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("working"), "previous").unwrap();
        begin_at(
            &mut data,
            "speech",
            &path,
            &backup,
            &file,
            temp.path(),
            vec![],
        )
        .unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("partial"), "failed package").unwrap();
        // Simulate termination after the installer changed files; reconstruct from the durable journal.
        let mut recovered: ModuleStore = file.load().unwrap();
        rollback_at(&mut recovered, &path, &backup, &file, temp.path()).unwrap();
        assert_eq!(recovered, expected);
        assert_eq!(file.load::<ModuleStore>().unwrap(), expected);
        assert_eq!(
            std::fs::read_to_string(path.join("working")).unwrap(),
            "previous"
        );
        assert!(!path.join("partial").exists());
        assert!(!backup.exists());
    }
    #[test]
    fn first_install_failure_restores_empty_store_without_touching_user_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("speech");
        let backup = temp.path().join("speech.rollback");
        let file = JsonStore::new(temp.path().join("modules.json"));
        let sentinel = temp.path().join("model.gguf");
        std::fs::write(&sentinel, "valuable").unwrap();
        let mut data = ModuleStore::default();
        begin_at(
            &mut data,
            "speech",
            &path,
            &backup,
            &file,
            temp.path(),
            vec![],
        )
        .unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("partial"), "partial").unwrap();
        rollback_at(&mut data, &path, &backup, &file, temp.path()).unwrap();
        assert_eq!(data, ModuleStore::default());
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "valuable");
    }
}

#[cfg(test)]
mod accounting_tests {
    use super::*;
    #[test]
    fn private_and_shared_bytes_are_separate_and_each_component_is_counted_once() {
        let mut first = bundled().remove(0);
        first.components.push(first.components[1].clone());
        let mut second = first.clone();
        second.id = "second".into();
        second.components[0].id = "other-runtime".into();
        let measured = |id: &str| match id {
            "transcribe-cpp" => 13,
            "other-runtime" => 19,
            "managed-python" => 7,
            "managed-uv" => 29,
            _ => 0,
        };
        assert_eq!(account(&first, measured), (13, 36));
        assert_eq!(account(&second, measured), (19, 36));
        // Two modules plus the single shared pool, without charging the pool twice.
        assert_eq!(
            account(&first, measured).0
                + account(&second, measured).0
                + account(&first, measured).1,
            68
        );
    }
    #[test]
    fn space_preflight_includes_downloads_full_replacement_and_reserve() {
        let m = bundled().remove(0);
        let required = m
            .components
            .iter()
            .map(|c| c.disk_bytes + c.download_bytes)
            .sum::<u64>()
            + 128 * 1024 * 1024;
        assert!(check_space(&m, required - 1, required * 2).is_err());
        assert!(check_space(&m, required, required * 2).is_ok());
        assert!(check_space(&m, required, 0).is_err());
    }
}

#[cfg(test)]
mod shared_repair_tests {
    use super::*;
    #[test]
    fn failed_shared_repair_restores_damaged_bytes_and_keeps_healthy_versions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let current = root.join("runtime");
        let backup = root.join("runtime.rollback");
        let bad = root.join("python/cpython-3.12.1-test");
        let healthy = root.join("python/cpython-3.12.2-test");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::create_dir_all(&healthy).unwrap();
        std::fs::write(bad.join("python"), "original damaged bytes").unwrap();
        std::fs::write(healthy.join("python"), "working shared version").unwrap();
        std::fs::create_dir(&current).unwrap();
        std::fs::write(current.join("backend"), "old backend").unwrap();
        let file = JsonStore::new(root.join("modules.json"));
        let mut data = ModuleStore::default();
        data.claim(&bundled().remove(0));
        let expected = data.clone();
        let shared = vec![
            SharedRuntime {
                relative: "python/cpython-3.12.1-test".into(),
                existed: true,
            },
            SharedRuntime {
                relative: PathBuf::from("uv").join(pyenv::UV_VERSION),
                existed: false,
            },
        ];
        begin_at(&mut data, "speech", &current, &backup, &file, root, shared).unwrap();
        std::fs::create_dir(&bad).unwrap();
        std::fs::write(bad.join("python"), "partial replacement").unwrap();
        let uv = root.join("uv").join(pyenv::UV_VERSION);
        std::fs::create_dir_all(&uv).unwrap();
        std::fs::write(uv.join("uv"), "new tool").unwrap();
        let mut recovered = file.load().unwrap();
        rollback_at(&mut recovered, &current, &backup, &file, root).unwrap();
        assert_eq!(recovered, expected);
        assert_eq!(
            std::fs::read_to_string(bad.join("python")).unwrap(),
            "original damaged bytes"
        );
        assert_eq!(
            std::fs::read_to_string(healthy.join("python")).unwrap(),
            "working shared version"
        );
        assert!(!uv.exists());
        assert!(current.join("backend").exists());
    }
    #[test]
    fn shared_repair_journal_cannot_escape_the_managed_support_root() {
        let root = Path::new("support");
        for path in [
            "../models",
            "python/../../models",
            "models",
            "uv/unknown",
            "python/not-a-runtime",
        ] {
            assert!(
                shared_path(
                    root,
                    &SharedRuntime {
                        relative: path.into(),
                        existed: true
                    }
                )
                .is_err(),
                "{path}"
            );
        }
    }
}
#[cfg(test)]
mod disk_cache_tests {
    use super::*;
    fn slow(_: &Path) -> u64 {
        std::thread::sleep(Duration::from_millis(600));
        7
    }
    #[test]
    fn slow_measurement_never_blocks_the_list_and_lands_for_the_next_one() {
        static CACHE: OnceLock<Mutex<DiskCache>> = OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let path = Path::new("big-runtime");
        let started = Instant::now();
        assert_eq!(
            cached_size_with(cache, path, Duration::from_millis(20), slow),
            0
        );
        // A second list while the walk runs returns at once too, without a second walk.
        assert_eq!(
            cached_size_with(cache, path, Duration::from_millis(20), slow),
            0
        );
        assert!(started.elapsed() < Duration::from_millis(400));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            cached_size_with(cache, path, Duration::from_millis(20), slow),
            7
        );
    }
    #[test]
    fn stale_size_is_served_while_it_refreshes() {
        static CACHE: OnceLock<Mutex<DiskCache>> = OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let path = Path::new("old-runtime");
        let long_ago = Instant::now()
            .checked_sub(SIZE_FRESH * 2)
            .unwrap_or_else(Instant::now);
        cache.lock().unwrap().sizes.insert(
            path.to_path_buf(),
            Measured {
                at: Some(long_ago),
                bytes: 3,
                changed: false,
            },
        );
        let started = Instant::now();
        assert_eq!(
            cached_size_with(cache, path, Duration::from_millis(20), slow),
            3
        );
        assert!(started.elapsed() < Duration::from_millis(400));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            cached_size_with(cache, path, Duration::from_millis(20), slow),
            7
        );
    }
    #[test]
    fn invalidated_size_waits_briefly_then_shows_the_new_number() {
        fn quick(_: &Path) -> u64 {
            9
        }
        static CACHE: OnceLock<Mutex<DiskCache>> = OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let path = Path::new("changed-runtime");
        cache.lock().unwrap().sizes.insert(
            path.to_path_buf(),
            Measured {
                at: Some(Instant::now()),
                bytes: 3,
                changed: true,
            },
        );
        assert_eq!(
            cached_size_with(cache, path, Duration::from_secs(5), quick),
            9
        );
        let slow_changed = Path::new("slow-changed-runtime");
        cache.lock().unwrap().sizes.insert(
            slow_changed.to_path_buf(),
            Measured {
                at: Some(Instant::now()),
                bytes: 3,
                changed: true,
            },
        );
        assert_eq!(
            cached_size_with(cache, slow_changed, Duration::from_millis(20), slow),
            3
        );
    }
}
#[cfg(test)]
mod first_list_tests {
    use super::*;
    fn quick(_: &Path) -> u64 {
        9
    }
    fn slow(_: &Path) -> u64 {
        std::thread::sleep(Duration::from_millis(600));
        7
    }
    #[test]
    fn never_measured_folder_does_not_hold_the_first_list() {
        static CACHE: OnceLock<Mutex<DiskCache>> = OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let path = Path::new("cold-runtime");
        // Even with a generous wait the first answer is immediate (0) while the walk runs.
        let started = Instant::now();
        assert_eq!(
            cached_size_with(cache, path, Duration::from_secs(5), slow),
            0
        );
        assert!(started.elapsed() < Duration::from_millis(400));
        assert!(PROVISIONAL.load(std::sync::atomic::Ordering::SeqCst));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            cached_size_with(cache, path, Duration::from_secs(5), quick),
            7
        );
    }
    #[test]
    fn persisted_size_is_served_at_once_and_refreshed() {
        static CACHE: OnceLock<Mutex<DiskCache>> = OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let path = Path::new("restored-runtime");
        cache.lock().unwrap().sizes.insert(
            path.to_path_buf(),
            Measured {
                at: None,
                bytes: 5,
                changed: false,
            },
        );
        let started = Instant::now();
        assert_eq!(
            cached_size_with(cache, path, Duration::from_secs(5), slow),
            5
        );
        assert!(started.elapsed() < Duration::from_millis(400));
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            cached_size_with(cache, path, Duration::from_secs(5), quick),
            7
        );
    }
}
