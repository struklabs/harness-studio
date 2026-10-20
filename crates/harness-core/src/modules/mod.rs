//! Bundled module contracts and lifecycle. Only backend dependencies are downloaded.
pub mod agents;
mod lifecycle;
pub mod paperclip;
use crate::{
    paths,
    pyenv::{self, envs},
    store::JsonStore,
};
pub use lifecycle::{
    action, ensure_enabled, remove_with_options as lifecycle_remove, RemovalOptions,
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
        if self.manifest_version != 1 {
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
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModuleView {
    pub manifest: ModuleManifest,
    pub state: ModuleState,
    pub incompatibility: Option<String>,
    pub progress: Option<pyenv::EnvProgress>,
    pub components: BTreeMap<String, OwnedComponent>,
    pub disk_bytes: u64,
    pub shared_disk_bytes: u64,
    pub update_available: bool,
    pub data_choices: Vec<lifecycle::DataChoice>,
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
    }
}
pub fn list() -> Result<Vec<ModuleView>, String> {
    let _lock = STATE_LOCK.lock().unwrap();
    let mut data = load()?;
    lifecycle::recover(&mut data)?;
    let backend = envs::status_in(&envs::root(), &envs::TRANSCRIBE_CPP);
    let active = OPERATION.try_lock().is_err() || backend.progress.is_some();
    let agent = agents::detect()?;
    let previous = data.clone();
    let adopting_agent = !data.modules.contains_key("agents") && agent.installed && !active;
    reconcile(&mut data, &selected("agents")?, agent.installed, active);
    if adopting_agent {
        data.components.get_mut("hermes-agent").unwrap().version =
            format!("external:{}", agent.version.as_deref().unwrap_or("unknown"));
    }
    reconcile(&mut data, &selected("speech")?, backend.ready, active);
    if data != previous {
        store().save(&data)?;
    }
    Ok(bundled()
        .into_iter()
        .map(|manifest| {
            let mut state = data.modules.get(&manifest.id).cloned().unwrap_or_default();
            if manifest.id == "speech" && backend.progress.is_some() {
                state.status = Status::Installing;
            }

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
                state,
                incompatibility,
                progress,
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
        .collect())
}
/// The job survives a disconnected API client; cancellation uses the installer's process-tree cleanup.
pub async fn install(id: &str) -> Result<(), String> {
    action(id, "install").await
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
        while envs::status_in(&envs::root(), &envs::TRANSCRIBE_CPP)
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
        m.manifest_version = 2;
        assert!(m.compatibility("windows-x86_64", "1.0.0").is_err());
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
}
