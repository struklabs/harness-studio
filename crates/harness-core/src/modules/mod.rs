//! Bundled module contracts and lifecycle. Only backend dependencies are downloaded.
pub mod agents;
pub mod bundle;
mod lifecycle;
mod openclaw;
pub mod paperclip;
use crate::{
    paths,
    pyenv::{self, envs},
    store::JsonStore,
};
pub use lifecycle::{
    action, ensure_enabled, ensure_not_disabled, remove_with_options as lifecycle_remove, start,
    RemovalOptions,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

/// Serialize managed Python startup with module filesystem mutations through process spawn.
pub(crate) fn managed_start_guard() -> Result<tokio::sync::MutexGuard<'static, ()>, String> {
    OPERATION.try_lock().map_err(|_| "modules.busy".into())
}

const ENV: &str = "transcribe-cpp";
static STATE_LOCK: Mutex<()> = Mutex::new(());
static OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LocaleKeys {
    pub en: String,
    pub tr: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Component {
    pub id: String,
    pub version: String,
    pub sources: Vec<String>,
    /// Estimates, including temporary download space; not a preflight free-space guarantee.
    pub download_bytes: u64,
    pub disk_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModuleManifest {
    pub id: String,
    pub version: String,
    pub manifest_version: u32,
    pub name: LocaleKeys,
    pub description: LocaleKeys,
    pub functions: Vec<LocaleKeys>,
    pub platforms: Vec<String>,
    pub hardware: LocaleKeys,
    pub components: Vec<Component>,
    pub permissions: Vec<LocaleKeys>,
    pub shared_component_ids: Vec<String>,
    pub min_app_version: String,
    pub planned: bool,
}
/// Module manifest schema versions this build installs. A manifest outside the range is
/// refused at install time (`modules.manifest_incompatible`) before anything is downloaded or
/// written: a newer one needs a newer app, an older one was dropped after its deprecation
/// window (`docs/guide/en/12-api-stability.md`). The schema of version 1 is
/// `sdk/module-manifest.v1.schema.json`.
pub const MANIFEST_VERSION_MIN: u32 = 1;
pub const MANIFEST_VERSION_MAX: u32 = 1;
fn version(v: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<_> = v.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}
impl ModuleManifest {
    pub fn compatibility(&self, platform: &str, app: &str) -> Result<(), String> {
        if !(MANIFEST_VERSION_MIN..=MANIFEST_VERSION_MAX).contains(&self.manifest_version) {
            return Err("modules.manifest_incompatible".into());
        }
        if !self.platforms.iter().any(|p| p == platform) {
            return Err("modules.platform_incompatible".into());
        }
        match (version(app), version(&self.min_app_version)) {
            (Some(have), Some(need)) if have >= need => Ok(()),
            _ => Err("modules.app_incompatible".into()),
        }
    }
}
/// Metadata stays in the binary; sources/estimates follow the existing installer pins.
pub fn bundled() -> Vec<ModuleManifest> {
    let mut modules: Vec<ModuleManifest> =
        serde_json::from_str(include_str!("bundled.json")).expect("bundled module manifests");
    let speech = &mut modules[0];
    speech.components = vec![
        Component {
            id: ENV.into(),
            version: envs::lock_digest(&envs::TRANSCRIBE_CPP),
            sources: vec!["https://pypi.org/simple".into()],
            download_bytes: 60 * 1024 * 1024,
            disk_bytes: envs::TRANSCRIBE_CPP.estimated_size_bytes,
        },
        Component {
            id: "managed-uv".into(),
            version: pyenv::UV_VERSION.into(),
            sources: pyenv::UV.iter().map(|s| s.url.into()).collect(),
            download_bytes: pyenv::UV.map_or(0, |s| s.size),
            disk_bytes: 60 * 1024 * 1024,
        },
        Component {
            id: "managed-python".into(),
            version: pyenv::PYTHON_SERIES.into(),
            sources: vec!["https://github.com/astral-sh/python-build-standalone/releases".into()],
            download_bytes: 40 * 1024 * 1024,
            disk_bytes: 150 * 1024 * 1024,
        },
    ];
    modules
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Status {
    #[default]
    NotInstalled,
    Installing,
    Installed,
    Disabled,
    Failed,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModuleState {
    pub status: Status,
    pub installed_version: Option<String>,
    pub components: Vec<String>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OwnedComponent {
    pub version: String,
    pub owners: BTreeSet<String>,
    #[serde(default)]
    pub disk_bytes: u64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleStore {
    pub modules: BTreeMap<String, ModuleState>,
    pub components: BTreeMap<String, OwnedComponent>,
    #[serde(default)]
    pub transaction: Option<lifecycle::Transaction>,
}
impl ModuleStore {
    pub fn claim(&mut self, manifest: &ModuleManifest) {
        self.release(&manifest.id);
        for c in &manifest.components {
            let entry = self.components.entry(c.id.clone()).or_default();
            entry.version = c.version.clone();
            entry.owners.insert(manifest.id.clone());
        }
        self.modules.insert(
            manifest.id.clone(),
            ModuleState {
                status: Status::Installed,
                installed_version: Some(manifest.version.clone()),
                components: manifest.components.iter().map(|c| c.id.clone()).collect(),
                error: None,
            },
        );
    }
    /// Returns components with no remaining owner. Shared support tools are retained by policy.
    pub fn release(&mut self, id: &str) -> Vec<String> {
        let mut unused = Vec::new();
        for (component, state) in &mut self.components {
            if state.owners.remove(id) && state.owners.is_empty() {
                unused.push(component.clone());
            }
        }
        self.components.retain(|_, state| !state.owners.is_empty());
        self.modules.insert(id.into(), ModuleState::default());
        unused
    }
}
fn store() -> JsonStore {
    JsonStore::new(paths::config_dir().join("modules.json"))
}
fn load() -> Result<ModuleStore, String> {
    if !paths::config_dir().join("modules.json").exists() {
        return Ok(ModuleStore::default());
    }
    store().load()
}
fn selected(id: &str) -> Result<ModuleManifest, String> {
    bundled()
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| "modules.unknown".into())
}
fn backend(id: &str) -> Result<ModuleManifest, String> {
    let m = selected(id)?;
    if m.planned {
        return Err("modules.planned".into());
    }
    Ok(m)
}
fn installable(id: &str) -> Result<ModuleManifest, String> {
    let m = backend(id)?;
    m.compatibility(
        &format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        env!("CARGO_PKG_VERSION"),
    )?;
    if id == "speech" {
        if let Some(error) = envs::platform_refusal(&envs::TRANSCRIBE_CPP) {
            return Err(error);
        }
    }
    Ok(m)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChangelogEntry {
    pub version: String,
    pub notes: LocaleKeys,
}
/// Catalog metadata of a module, kept apart from the manifest so the manifest schema and the
/// frozen API contract stay unchanged. Every field is optional; a module without an entry is
/// an official one with no extra metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModuleCatalog {
    /// Catalog category id, for example `speech`, `agents`, `training`.
    #[serde(default)]
    pub category: Option<String>,
    /// Free search keywords.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Publisher; absent for official modules (shown as Harness Studio).
    #[serde(default)]
    pub author: Option<String>,
    /// SPDX license identifier.
    #[serde(default)]
    pub license: Option<String>,
    /// Source repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// True for a third-party module. Community entries are never presented as official.
    #[serde(default)]
    pub community: bool,
    /// Capability ids the module provides, for example `transcribe`, `agent`.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Ids of other modules this one needs.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Release notes, newest first.
    #[serde(default)]
    pub changelog: Vec<ChangelogEntry>,
    /// ISO date (`YYYY-MM-DD`) of the latest release.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Editorial recommendation, listed first under "Recommended".
    #[serde(default)]
    pub featured: bool,
    /// Relative popularity for sorting, filled by a registry; 0 for bundled modules.
    #[serde(default)]
    pub popularity: u32,
}
/// Catalog metadata of the bundled modules, by module id.
pub fn bundled_catalog() -> std::collections::BTreeMap<String, ModuleCatalog> {
    serde_json::from_str(include_str!("catalog.json")).expect("bundled module catalog")
}
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModuleView {
    pub manifest: ModuleManifest,
    /// Catalog metadata (category, tags, author, license, community flag, changelog).
    pub catalog: ModuleCatalog,
    pub state: ModuleState,
    pub incompatibility: Option<String>,
    pub progress: Option<pyenv::EnvProgress>,
    pub components: BTreeMap<String, OwnedComponent>,
    pub disk_bytes: u64,
    pub shared_disk_bytes: u64,
    pub update_available: bool,
    pub data_choices: Vec<lifecycle::DataChoice>,
    /// Key files of an installed module that are gone (deleted by hand): the module needs the
    /// Repair action. Empty when nothing is missing.
    pub needs_repair: Vec<String>,
    /// Some numbers (folder sizes, the agent probe) are still being measured in the
    /// background: the list is complete in shape, ask again shortly for the final values.
    pub measuring: bool,
}
/// Adopt legacy installs once; explicit removal stays removed if another module owns the backend.
fn reconcile(data: &mut ModuleStore, manifest: &ModuleManifest, ready: bool, active: bool) {
    if active {
        return;
    }
    let state = data.modules.get(&manifest.id);
    if ready && state.is_none() {
        data.claim(manifest);
    } else if !ready && state.is_some_and(|state| state.status == Status::Installed) {
        let state = data.modules.get_mut(&manifest.id).unwrap();
        state.status = Status::Failed;
        state.error = Some("modules.interrupted".into());
    } else if ready
        && state.is_some_and(|state| {
            state.status == Status::Failed
                && state.installed_version.is_some()
                && state.error.as_deref() == Some("modules.interrupted")
        })
    {
        // A detection miss (for example a changed home folder) is not a failed install:
        // once the module is detected again its state follows.
        let state = data.modules.get_mut(&manifest.id).unwrap();
        state.status = Status::Installed;
        state.error = None;
    }
}
/// The agent probe spawns a process (about two seconds cold), so the list never waits for it:
/// it asks the cached result and, when there is none that is fresh, probes on a thread. Until
/// then the stored state stands (no adoption, no "interrupted" flip on unknown data).
struct AgentProbe {
    value: Option<(std::time::Instant, agents::Detection)>,
    running: bool,
    generation: u64,
}
static AGENT_PROBE: Mutex<AgentProbe> = Mutex::new(AgentProbe {
    value: None,
    running: false,
    generation: 0,
});
const AGENT_PROBE_FRESH: std::time::Duration = std::time::Duration::from_secs(30);
fn agent_probe() -> Option<agents::Detection> {
    agent_probe_with(&AGENT_PROBE, agents::detect)
}
fn agent_probe_with(
    slot: &'static Mutex<AgentProbe>,
    probe: fn() -> Result<agents::Detection, String>,
) -> Option<agents::Detection> {
    let mut guard = slot.lock().unwrap();
    if let Some((at, found)) = &guard.value {
        if at.elapsed() < AGENT_PROBE_FRESH {
            return Some(found.clone());
        }
    }
    if !guard.running {
        guard.running = true;
        let generation = guard.generation;
        std::thread::spawn(move || {
            let found = probe();
            let mut guard = slot.lock().unwrap();
            guard.running = false;
            if let (Ok(found), true) = (found, guard.generation == generation) {
                guard.value = Some((std::time::Instant::now(), found));
            }
        });
    }
    None
}
/// An install or removal changed what the probe sees.
fn invalidate_agent_probe() {
    let mut guard = AGENT_PROBE.lock().unwrap();
    guard.value = None;
    guard.generation += 1;
}
/// Key files whose absence means the installed module cannot start (checked on every list, so
/// only existence checks). `root` is the managed Python root (`envs::root()`).
fn speech_missing_files(root: &std::path::Path) -> Vec<String> {
    let mut missing = Vec::new();
    let venv = envs::env_dir_in(root, ENV).join("venv");
    if !pyenv::venv_python(&venv).is_file() {
        missing.push("python".into());
    }
    let interpreter = std::fs::read_dir(root.join("python"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!("cpython-{}.", pyenv::PYTHON_SERIES))
        })
        .any(|e| {
            e.path()
                .join(if cfg!(windows) {
                    "python.exe"
                } else {
                    "bin/python3"
                })
                .is_file()
        });
    if !interpreter {
        missing.push("managed-python".into());
    }
    let packages: Vec<std::path::PathBuf> = if cfg!(windows) {
        vec![venv.join("Lib/site-packages")]
    } else {
        std::fs::read_dir(venv.join("lib"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path().join("site-packages"))
            .collect()
    };
    let package = packages.iter().any(|dir| {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name == "transcribe_cpp" || name.starts_with("transcribe_cpp.")
            })
    });
    if !package {
        missing.push("transcribe-cpp".into());
    }
    missing
}
fn missing_files(id: &str) -> Vec<String> {
    match id {
        "speech" => speech_missing_files(&envs::root()),
        "agents" => agents::missing_files(),
        _ => vec![],
    }
}
pub fn list() -> Result<Vec<ModuleView>, String> {
    lifecycle::begin_listing();
    let _lock = STATE_LOCK.lock().unwrap();
    let mut data = load()?;
    lifecycle::recover(&mut data)?;
    let backend = envs::status_in_quick(&envs::root(), &envs::TRANSCRIBE_CPP);
    let active = OPERATION.try_lock().is_err() || backend.progress.is_some();
    let agent = agent_probe();
    let previous = data.clone();
    if let Some(agent) = &agent {
        let adopting_agent = !data.modules.contains_key("agents") && agent.installed && !active;
        reconcile(&mut data, &selected("agents")?, agent.installed, active);
        if adopting_agent {
            data.components.get_mut("hermes-agent").unwrap().version =
                format!("external:{}", agent.version.as_deref().unwrap_or("unknown"));
        }
    }
    reconcile(&mut data, &selected("speech")?, backend.ready, active);
    if data != previous {
        store().save(&data)?;
    }
    let catalog = bundled_catalog();
    let mut views: Vec<ModuleView> = bundled()
        .into_iter()
        .map(|manifest| {
            let mut state = data.modules.get(&manifest.id).cloned().unwrap_or_default();
            if manifest.id == "speech" && backend.progress.is_some() {
                state.status = Status::Installing;
            }
            let needs_repair =
                if !active && matches!(state.status, Status::Installed | Status::Disabled) {
                    missing_files(&manifest.id)
                } else {
                    vec![]
                };
            let incompatibility = manifest
                .compatibility(
                    &format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                    env!("CARGO_PKG_VERSION"),
                )
                .err();
            let progress = if manifest.id == "speech" {
                backend.progress.clone()
            } else {
                None
            };
            ModuleView {
                catalog: catalog.get(&manifest.id).cloned().unwrap_or_default(),
                state,
                incompatibility,
                progress,
                needs_repair,
                measuring: false,
                disk_bytes: lifecycle::module_bytes(&manifest),
                shared_disk_bytes: lifecycle::shared_bytes(&manifest),
                update_available: lifecycle::update_available(&data, &manifest),
                data_choices: lifecycle::data_choices(&manifest.id),
                components: manifest
                    .components
                    .iter()
                    .map(|spec| {
                        let mut c = data.components.get(&spec.id).cloned().unwrap_or_default();
                        c.disk_bytes = lifecycle::component_bytes(&spec.id);
                        (spec.id.clone(), c)
                    })
                    .collect(),
                manifest,
            }
        })
        .collect();
    let measuring = lifecycle::listing_provisional() || agent.is_none();
    for view in &mut views {
        view.measuring = measuring;
    }
    Ok(views)
}
/// The job survives a disconnected API client; cancellation uses the installer's process-tree cleanup.
pub async fn install(id: &str) -> Result<(), String> {
    action(id, "install").await
}
/// Like [`install`], but a long install (the Hermes Agent) continues in the background.
pub async fn start_install(id: &str) -> Result<(), String> {
    start(id, "install").await
}

pub fn cancel(id: &str) -> Result<(), String> {
    installable(id)?;
    if id == "agents" {
        agents::cancel_install();
    } else {
        envs::cancel_install(ENV);
    }
    Ok(())
}
pub fn remove(id: &str) -> Result<(), String> {
    lifecycle::remove_with_options(id, &RemovalOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Downloads the real optional backend into the isolated test data root.
    #[tokio::test]
    #[ignore = "downloads uv, Python, and the speech backend"]
    async fn real_speech_install_cancel_retry_and_remove_preserves_user_data() {
        let job = tokio::spawn(async { install("speech").await });
        while envs::status_in_quick(&envs::root(), &envs::TRANSCRIBE_CPP)
            .progress
            .is_none()
            && !job.is_finished()
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        cancel("speech").unwrap();
        assert!(job.await.unwrap().is_err());
        assert_eq!(list().unwrap()[0].state.status, Status::Failed);
        install("speech").await.unwrap(); // Existing installer verifies native_version().
        let view = list().unwrap().remove(0);
        assert_eq!(view.state.status, Status::Installed);
        assert_eq!(view.components["managed-python"].owners.len(), 1);
        let sentinel = paths::data_root().join("models/module-preservation.txt");
        std::fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        std::fs::write(&sentinel, "user model data").unwrap();
        remove("speech").unwrap();
        assert_eq!(list().unwrap()[0].state.status, Status::NotInstalled);
        assert_eq!(
            std::fs::read_to_string(sentinel).unwrap(),
            "user model data"
        );
        assert!(envs::root().join("python").is_dir());
        assert!(!envs::env_dir_in(&envs::root(), ENV).exists());
    }
    #[test]
    fn parses_bundled_and_rejects_incomplete_manifest() {
        let modules = bundled();
        assert_eq!(modules.len(), 5);
        assert!(!modules[0].planned);
        assert!(serde_json::from_str::<ModuleManifest>(r#"{"id":"speech"}"#).is_err());
        for m in modules {
            assert!(version(&m.version).is_some());
            assert!(!m.name.tr.is_empty());
        }
    }
    #[test]
    fn compatibility_checks_contract_platform_and_app() {
        let mut m = bundled().remove(0);
        assert!(m.compatibility("windows-x86_64", "0.1.2").is_ok());
        assert!(m.compatibility("linux-aarch64", "0.1.2").is_err());
        assert!(m.compatibility("windows-x86_64", "0.1.1").is_err());
        assert!(m.compatibility("windows-x86_64", "broken").is_err());
        m.manifest_version = MANIFEST_VERSION_MAX + 1;
        assert!(m.compatibility("windows-x86_64", "1.0.0").is_err());
        m.manifest_version = 0;
        assert!(m.compatibility("windows-x86_64", "1.0.0").is_err());
        m.manifest_version = MANIFEST_VERSION_MIN;
        assert!(m.compatibility("windows-x86_64", "1.0.0").is_ok());
    }
    #[test]
    fn manifest_schema_file_matches_the_manifest_type() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../sdk/module-manifest.v1.schema.json"
        ))
        .unwrap();
        let fields = |v: &serde_json::Value| -> BTreeSet<String> {
            v.as_object().unwrap().keys().cloned().collect()
        };
        let sample = serde_json::to_value(bundled().remove(0)).unwrap();
        assert_eq!(fields(&schema["properties"]), fields(&sample));
        let required: BTreeSet<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(required, fields(&sample));
        assert_eq!(schema["additionalProperties"], false);
        let declared = &schema["properties"]["manifest_version"];
        assert_eq!(declared["minimum"], MANIFEST_VERSION_MIN);
        assert_eq!(declared["maximum"], MANIFEST_VERSION_MAX);
        for m in bundled() {
            assert!((MANIFEST_VERSION_MIN..=MANIFEST_VERSION_MAX).contains(&m.manifest_version));
        }
    }
    #[test]
    fn recovery_adopts_legacy_installs_and_preserves_explicit_removal() {
        let manifest = bundled().remove(0);
        let mut data = ModuleStore::default();
        reconcile(&mut data, &manifest, true, false);
        assert_eq!(data.modules["speech"].status, Status::Installed);
        data.modules.get_mut("speech").unwrap().status = Status::Installing;
        reconcile(&mut data, &manifest, false, true);
        assert_eq!(data.modules["speech"].status, Status::Installing);
        data.modules.get_mut("speech").unwrap().status = Status::Installed;
        reconcile(&mut data, &manifest, false, false);
        assert_eq!(data.modules["speech"].status, Status::Failed);
        // A detection miss heals once the module is detected again ...
        reconcile(&mut data, &manifest, true, false);
        assert_eq!(data.modules["speech"].status, Status::Installed);
        assert_eq!(data.modules["speech"].error, None);
        // ... but a real failed install (no installed version) stays failed.
        data.modules.insert(
            "speech".into(),
            ModuleState {
                status: Status::Failed,
                error: Some("agents.install_failed".into()),
                ..Default::default()
            },
        );
        reconcile(&mut data, &manifest, true, false);
        assert_eq!(data.modules["speech"].status, Status::Failed);
        data.release("speech");
        reconcile(&mut data, &manifest, true, false);
        assert_eq!(data.modules["speech"].status, Status::NotInstalled);
    }
    #[test]
    fn listing_preserves_disabled_state_and_installed_versions() {
        let manifest = bundled().remove(0);
        let mut data = ModuleStore::default();
        data.claim(&manifest);
        data.modules.get_mut("speech").unwrap().status = Status::Disabled;
        data.modules.get_mut("speech").unwrap().installed_version = Some("0.0.1".into());
        data.components.get_mut(ENV).unwrap().version = "old-lock".into();
        let expected = data.clone();
        reconcile(&mut data, &manifest, true, false);
        reconcile(&mut data, &manifest, false, false);
        assert_eq!(data, expected);
    }
    #[test]
    fn shared_components_are_reference_counted_and_store_round_trips() {
        let first = bundled().remove(0);
        let mut second = first.clone();
        second.id = "other".into();
        let mut data = ModuleStore::default();
        data.claim(&first);
        data.claim(&first);
        data.claim(&second);
        assert_eq!(data.components[ENV].owners.len(), 2);
        assert!(data.release("speech").is_empty());
        assert_eq!(data.components[ENV].owners.len(), 1);
        let temp = tempfile::tempdir().unwrap();
        let file = JsonStore::new(temp.path().join("modules.json"));
        file.save(&data).unwrap();
        let mut data: ModuleStore = file.load().unwrap();
        assert!(data.release("other").contains(&ENV.to_string()));
        assert!(data.components.is_empty());
    }
    fn touch(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }
    #[test]
    fn deleted_package_files_are_reported_for_repair() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        assert_eq!(
            speech_missing_files(root),
            ["python", "managed-python", "transcribe-cpp"]
        );
        let venv = envs::env_dir_in(root, ENV).join("venv");
        touch(&pyenv::venv_python(&venv));
        let interpreter = root
            .join("python")
            .join(format!("cpython-{}.1-test", pyenv::PYTHON_SERIES));
        touch(&interpreter.join(if cfg!(windows) {
            "python.exe"
        } else {
            "bin/python3"
        }));
        let site = if cfg!(windows) {
            venv.join("Lib/site-packages")
        } else {
            venv.join("lib/python3.12/site-packages")
        };
        // A leftover dist-info is not the package itself.
        touch(&site.join("transcribe_cpp-1.0.dist-info/METADATA"));
        assert_eq!(speech_missing_files(root), ["transcribe-cpp"]);
        touch(&site.join("transcribe_cpp/__init__.py"));
        assert!(speech_missing_files(root).is_empty());
        std::fs::remove_dir_all(site.join("transcribe_cpp")).unwrap();
        assert_eq!(speech_missing_files(root), ["transcribe-cpp"]);
    }
    fn found() -> Result<agents::Detection, String> {
        std::thread::sleep(std::time::Duration::from_millis(300));
        Ok(agents::Detection {
            installed: true,
            version: Some("1".into()),
            executable: None,
            config_path: String::new(),
            missing_dependencies: vec![],
            connected: false,
            installation: Default::default(),
            model: None,
            managed: false,
        })
    }
    #[test]
    fn agent_probe_never_blocks_the_list_and_lands_for_the_next_one() {
        static SLOT: Mutex<AgentProbe> = Mutex::new(AgentProbe {
            value: None,
            running: false,
            generation: 0,
        });
        let started = std::time::Instant::now();
        assert!(agent_probe_with(&SLOT, found).is_none());
        assert!(agent_probe_with(&SLOT, found).is_none());
        assert!(started.elapsed() < std::time::Duration::from_millis(250));
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(agent_probe_with(&SLOT, found).is_some_and(|d| d.installed));
        // A change while a probe runs discards that probe's stale answer.
        SLOT.lock().unwrap().value = None;
        assert!(agent_probe_with(&SLOT, found).is_none());
        {
            let mut guard = SLOT.lock().unwrap();
            guard.value = None;
            guard.generation += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(agent_probe_with(&SLOT, found).is_none());
    }
}
#[cfg(test)]
mod catalog_tests {
    use super::*;
    #[test]
    fn every_bundled_module_has_catalog_metadata_and_is_official() {
        let catalog = bundled_catalog();
        for m in bundled() {
            let c = catalog
                .get(&m.id)
                .unwrap_or_else(|| panic!("{} has no catalog entry", m.id));
            assert!(c.category.is_some() && !c.tags.is_empty(), "{}", m.id);
            assert!(!c.community, "{} is official", m.id);
        }
        assert!(catalog
            .keys()
            .all(|id| bundled().iter().any(|m| &m.id == id)));
    }
    #[test]
    fn a_module_without_metadata_deserializes_as_official_defaults() {
        let c: ModuleCatalog = serde_json::from_str("{}").unwrap();
        assert!(!c.community && c.tags.is_empty() && c.category.is_none());
    }
}
