//! Background auto-update of the managed llama.cpp runtime
//! (`managed_runtime.auto_update`): checks GitHub periodically, downloads and
//! verifies new builds into staging, and activates them once no server is running.

use crate::config::{AppSettings, RuntimeMode};
use crate::operation_gate::OperationGate;
use crate::runtime::RuntimeManager;
use crate::servers::ServerRegistry;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const ACTIVATION_POLL: Duration = Duration::from_secs(30);
/// Marks which release the staging dir holds (a manual download wipes it).
const TAG_MARKER: &str = ".harness-auto-update-tag";

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdatePhase {
    #[default]
    Idle,
    Checking,
    Downloading,
    /// Downloaded and verified; waiting for all servers to stop.
    PendingActivation,
    Updated,
    /// The new build has archives without a SHA-256 digest: never installed in the
    /// background (the user can install it from Settings > Runtime after confirming).
    SkippedUnverified,
    /// The installed build was kept on purpose: the new release has no build of its backend
    /// yet, the staged build is another backend or sees no GPU the installed one sees, or the
    /// installed build is CPU-only on a machine with a GPU (`message` says which).
    KeptCurrent,
    Error,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AutoUpdateStatus {
    pub phase: AutoUpdatePhase,
    pub last_check: Option<u64>,
    pub latest_version: Option<String>,
    pub message: Option<String>,
    /// Installed build (`installed_version`, `backend`) when `phase` was set: a hold or error
    /// about a build that has since been replaced (Settings, Control API, CLI) no longer applies.
    #[serde(skip)]
    install: Option<(Option<String>, Option<String>)>,
}

impl AutoUpdateStatus {
    /// `KeptCurrent`, `SkippedUnverified` and `Error` describe the build installed when they were
    /// set; once another build is installed they are stale and read as `Idle`.
    fn current(mut self, install: &(Option<String>, Option<String>)) -> Self {
        let about_build = matches!(
            self.phase,
            AutoUpdatePhase::KeptCurrent
                | AutoUpdatePhase::SkippedUnverified
                | AutoUpdatePhase::Error
        );
        if about_build && self.install.as_ref().is_some_and(|i| i != install) {
            self.phase = AutoUpdatePhase::Idle;
            self.message = None;
        }
        self
    }
}

fn installed_build() -> (Option<String>, Option<String>) {
    let mr = AppSettings::load().managed_runtime;
    (mr.installed_version, mr.backend)
}

#[derive(Clone, Default)]
pub struct RuntimeAutoUpdater {
    status: Arc<Mutex<AutoUpdateStatus>>,
}

/// llama.cpp tags are `b<build>`; compares numerically so b10000 > b9999.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    let num = |t: &str| t.trim_start_matches('b').parse::<u64>().ok();
    match (num(latest), num(installed)) {
        (Some(l), Some(i)) => l > i,
        _ => latest != installed,
    }
}

/// `"<tag>: <asset>, <asset>"` when some archive of the release can't be verified.
fn unverified_notice(release: &crate::runtime::ReleaseAssetInfo) -> Option<String> {
    let names: Vec<String> = release
        .unverified_assets()
        .into_iter()
        .map(|a| a.name)
        .collect();
    (!names.is_empty()).then(|| format!("{}: {}", release.tag_name, names.join(", ")))
}

/// Marks a stage-1c refusal (`verify_staged_backend`) so it is reported as "kept current".
const KEPT_PREFIX: &str = "kept:";

/// Backend an update must keep: the recorded one (`managed_runtime.backend`), else the one
/// detected from the installed files (`runtime.update_backend_unknown` when neither is
/// known). A CPU-only build that
/// was never recorded (so never chosen explicitly) on a machine whose hardware recommends a
/// GPU build is not updated either: it is held with `runtime.update_cpu_on_gpu` so Settings
/// can say that the servers don't use the GPU (older versions could install such a build).
fn update_backend(
    configured: Option<&str>,
    detected: Option<&str>,
    recommended: crate::hardware::RecommendedBackend,
) -> Result<String, String> {
    let known = |t: Option<&str>| {
        t.map(str::trim)
            .filter(|t| !t.is_empty())
            .map(crate::runtime::normalize_backend)
    };
    if let Some(b) = known(configured) {
        return Ok(b);
    }
    match known(detected) {
        Some(b) if b == "cpu" && recommended != crate::hardware::RecommendedBackend::Cpu => {
            Err(crate::error_key::err("runtime.update_cpu_on_gpu")
                .with("recommended", recommended.as_str())
                .into())
        }
        Some(b) => Ok(b),
        None => Err(crate::error_key::err("runtime.update_backend_unknown").into()),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl RuntimeAutoUpdater {
    pub fn status(&self) -> AutoUpdateStatus {
        let status = self.status.lock().unwrap().clone();
        status.current(&installed_build())
    }

    fn set(&self, phase: AutoUpdatePhase, message: Option<String>) {
        let install = installed_build();
        let mut s = self.status.lock().unwrap();
        s.phase = phase;
        s.message = message;
        s.install = Some(install);
    }

    fn enabled() -> Option<AppSettings> {
        let settings = AppSettings::load();
        (settings.runtime_mode == RuntimeMode::Managed
            && settings.managed_runtime.auto_update
            && settings.managed_runtime.installed_version.is_some())
        .then_some(settings)
    }

    /// Background loop: first check after a short delay, then periodically.
    /// `on_updated` is called with the saved settings after a successful activation.
    pub async fn run_loop(
        self,
        runtime: Arc<RuntimeManager>,
        registry: Arc<ServerRegistry>,
        gate: Arc<OperationGate>,
        on_updated: impl Fn(&AppSettings),
    ) {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        // Builds installed before the shared library store: their CUDA runtime copies become
        // links (once per start; cheap when already shared).
        crate::shared_libs::dedupe_installed(&runtime, &crate::runtimes::RuntimeRegistry::global())
            .await;
        loop {
            if Self::enabled().is_some() {
                self.run_once(&runtime, &registry, &gate, &on_updated).await;
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    }

    /// One check → download → (wait) → activate cycle.
    /// `on_updated` is called with the saved settings after a successful activation.
    pub async fn run_once(
        &self,
        runtime: &Arc<RuntimeManager>,
        registry: &ServerRegistry,
        gate: &OperationGate,
        on_updated: &impl Fn(&AppSettings),
    ) {
        let Some(settings) = Self::enabled() else {
            return;
        };
        let installed = settings
            .managed_runtime
            .installed_version
            .clone()
            .unwrap_or_default();

        self.set(AutoUpdatePhase::Checking, None);
        let hw = crate::hardware::detect_hardware();
        // Updates keep the installed build's backend: the recorded one, else the one detected
        // from its files. Never the recommendation, never a fallback to another backend.
        let backend = match update_backend(
            settings.managed_runtime.backend.as_deref(),
            runtime.installed_backend().as_deref(),
            hw.recommended_backend,
        ) {
            Ok(b) => b,
            Err(e) => return self.set(AutoUpdatePhase::KeptCurrent, Some(e)),
        };
        let target = crate::runtime::install_query(&hw, Some(&backend), None, None);
        let release = match runtime
            .check_latest_release(&target.query, target.mode)
            .await
        {
            Ok(r) => r,
            Err(e)
                if crate::error_key::parse(&e).is_some_and(|(k, _)| {
                    k == "runtime.no_matching_asset" || k == "runtime.cuda_driver_too_old"
                }) =>
            {
                return self.set(AutoUpdatePhase::KeptCurrent, Some(e))
            }
            Err(e) => return self.set(AutoUpdatePhase::Error, Some(e)),
        };
        {
            let mut s = self.status.lock().unwrap();
            s.last_check = Some(now_secs());
            s.latest_version = Some(release.tag_name.clone());
        }
        if !is_newer(&release.tag_name, &installed) {
            return self.set(AutoUpdatePhase::Idle, None);
        }
        if let Some(message) = unverified_notice(&release) {
            return self.set(AutoUpdatePhase::SkippedUnverified, Some(message));
        }

        // Download + verify into staging.
        {
            let _guard = match runtime.try_busy() {
                Ok(g) => g,
                Err(e) => return self.set(AutoUpdatePhase::Idle, Some(e)),
            };
            self.set(AutoUpdatePhase::Downloading, None);
            let mut urls = vec![release.download_url.clone()];
            urls.extend(release.extra_assets.iter().map(|a| a.download_url.clone()));
            match runtime.download_and_extract_staging(&urls, false).await {
                Ok(()) => {}
                Err(e) if e == crate::runtime::ERR_INTEGRITY_UNVERIFIED => {
                    let notice =
                        unverified_notice(&release).unwrap_or_else(|| release.tag_name.clone());
                    return self.set(AutoUpdatePhase::SkippedUnverified, Some(notice));
                }
                Err(e) => return self.set(AutoUpdatePhase::Error, Some(e)),
            }
            let rt = runtime.clone();
            let expected = backend.clone();
            let verified = tokio::task::spawn_blocking(move || {
                rt.verify_stage1_binary()?;
                rt.verify_staged_backend(&expected)
                    .map_err(|e| format!("{KEPT_PREFIX}{e}"))
            })
            .await;
            match verified {
                Ok(Ok(())) => {
                    let marker = runtime.staging_dir().join(TAG_MARKER);
                    let _ = tokio::fs::write(marker, &release.tag_name).await;
                }
                Ok(Err(e)) => {
                    return match e.strip_prefix(KEPT_PREFIX) {
                        Some(e) => self.set(AutoUpdatePhase::KeptCurrent, Some(e.to_string())),
                        None => self.set(AutoUpdatePhase::Error, Some(e)),
                    }
                }
                Err(e) => {
                    return self.set(
                        AutoUpdatePhase::Error,
                        Some(
                            crate::error_key::err("task.join_failed")
                                .with("error", e)
                                .into(),
                        ),
                    )
                }
            }
        }

        // Activate as soon as nothing is running (never interrupts a server).
        self.set(AutoUpdatePhase::PendingActivation, None);
        loop {
            if Self::enabled().is_none() {
                return self.set(AutoUpdatePhase::Idle, None);
            }
            let staged = tokio::fs::read_to_string(runtime.staging_dir().join(TAG_MARKER))
                .await
                .ok();
            if staged.as_deref() != Some(release.tag_name.as_str()) {
                // Staging was replaced by a manual download; nothing to activate.
                return self.set(AutoUpdatePhase::Idle, None);
            }
            {
                let _runtime_lease = gate.runtime_write().await;
                if !registry.any_active() {
                    if let Ok(_guard) = runtime.try_busy() {
                        let _ =
                            tokio::fs::remove_file(runtime.staging_dir().join(TAG_MARKER)).await;
                        match runtime.activate_runtime(Some(&release.tag_name)) {
                            Ok(manifest) => {
                                let mut s = AppSettings::load();
                                crate::runtime::record_activation(&mut s, &manifest);
                                if let Err(e) = s.save() {
                                    return self.set(AutoUpdatePhase::Error, Some(e));
                                }
                                self.set(
                                    AutoUpdatePhase::Updated,
                                    Some(format!("{} → {}", installed, release.tag_name)),
                                );
                                on_updated(&s);
                                return;
                            }
                            Err(e) => return self.set(AutoUpdatePhase::Error, Some(e)),
                        }
                    }
                }
            }
            tokio::time::sleep(ACTIVATION_POLL).await;
        }
    }
}

/// Settings > Runtime's managed build, for the Control API and the CLI.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ManagedRuntimeStatus {
    pub installed_version: Option<String>,
    pub previous_version: Option<String>,
    /// Backend recorded for the installed build (what updates keep).
    pub backend: Option<String>,
    /// Backend detected from the installed build's files.
    pub installed_backend: Option<String>,
    /// What "roll back" restores (`None` = the backup slot holds no build).
    #[cfg_attr(feature = "openapi", schema(value_type = Option<Object>))]
    pub backup: Option<crate::runtime::RuntimeManifest>,
    pub auto_update: bool,
    /// Download of a running install (`None` when idle).
    #[cfg_attr(feature = "openapi", schema(value_type = Option<Object>))]
    pub download: Option<crate::runtime::RuntimeDownloadProgress>,
}

pub fn managed_status(runtime: &RuntimeManager) -> ManagedRuntimeStatus {
    let settings = AppSettings::load();
    let mr = settings.managed_runtime;
    ManagedRuntimeStatus {
        installed_version: mr.installed_version,
        previous_version: mr.previous_version,
        backend: mr.backend,
        installed_backend: runtime.installed_backend(),
        backup: runtime.backup_manifest(),
        auto_update: mr.auto_update,
        download: runtime.get_download_progress(),
    }
}

/// Result of [`install_managed`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ManagedInstall {
    /// Release tag that was installed.
    pub version: String,
    /// Backend of the installed build.
    pub backend: Option<String>,
    /// Archives that were downloaded.
    pub assets: Vec<String>,
    /// `llama-server --version` of the new build.
    pub version_text: String,
    /// The build it replaced (now in the backup slot).
    pub previous_version: Option<String>,
}

/// Downloads, verifies and activates the newest official build of the managed runtime, the
/// same steps as Settings > Runtime: `backend` given = exactly that backend (a switch), else
/// the installed build's backend (an update; never another one), else the hardware
/// recommendation (first install). Archives without a SHA-256 digest need `allow_unverified`.
/// Servers must be stopped (`runtime.stop_servers_first`); the installed build is untouched
/// until the new one passed stage 1 (`--version`, and for a GPU build the device check).
pub async fn install_managed(
    runtime: &Arc<RuntimeManager>,
    registry: &ServerRegistry,
    gate: &OperationGate,
    backend: Option<&str>,
    allow_unverified: bool,
) -> Result<ManagedInstall, String> {
    let settings = AppSettings::load();
    let hw = crate::hardware::detect_hardware();
    let target = crate::runtime::install_query(
        &hw,
        settings.managed_runtime.backend.as_deref(),
        runtime.installed_backend().as_deref(),
        backend,
    );
    let release = runtime
        .check_latest_release(&target.query, target.mode)
        .await?;
    crate::orchestrator::ensure_server_stopped(registry)?;
    let _busy = runtime.try_busy()?;
    let mut urls = vec![release.download_url.clone()];
    urls.extend(release.extra_assets.iter().map(|a| a.download_url.clone()));
    runtime
        .download_and_extract_staging(&urls, allow_unverified)
        .await?;
    let rt = runtime.clone();
    let expected = release.backend.clone();
    let version_text = tokio::task::spawn_blocking(move || {
        let text = rt.verify_stage1_binary()?;
        if let Some(b) = expected.as_deref() {
            rt.verify_staged_backend(b)?;
        }
        Ok::<_, String>(text)
    })
    .await
    .map_err(|e| String::from(crate::error_key::err("task.join_failed").with("error", e)))??;

    let _runtime_lease = gate.runtime_write().await;
    crate::orchestrator::ensure_server_stopped(registry)?;
    let manifest = runtime.activate_runtime(Some(&release.tag_name))?;
    let mut s = AppSettings::load();
    crate::runtime::record_activation(&mut s, &manifest);
    s.save()?;
    Ok(ManagedInstall {
        version: release.tag_name.clone(),
        backend: manifest.backend,
        assets: release.assets().into_iter().map(|a| a.name).collect(),
        version_text,
        previous_version: s.managed_runtime.previous_version,
    })
}

/// "Roll back" (Settings > Runtime): the backup build becomes the installed one again.
pub async fn rollback_managed(
    runtime: &RuntimeManager,
    registry: &ServerRegistry,
    gate: &OperationGate,
) -> Result<ManagedRuntimeStatus, String> {
    let _runtime_lease = gate.runtime_write().await;
    crate::orchestrator::ensure_server_stopped(registry)?;
    let _busy = runtime.try_busy()?;
    let restored = runtime.rollback_runtime()?;
    let mut s = AppSettings::load();
    crate::runtime::record_rollback(&mut s, &restored);
    s.save()?;
    Ok(managed_status(runtime))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real b4000 release (no digests): auto-update would skip it with a notice naming it.
    #[test]
    fn test_unverified_release_is_skipped_with_notice() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/github-release-b4000.json");
        let release: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let query = crate::hardware::AssetQuery {
            os: "win".into(),
            arch: "x64".into(),
            backend: Some("vulkan".into()),
        };
        let info =
            crate::runtime::pick_release(&[release], &query, crate::runtime::PickMode::Exact)
                .unwrap();
        assert_eq!(
            unverified_notice(&info).as_deref(),
            Some("b4000: llama-b4000-bin-win-vulkan-x64.zip")
        );
    }

    /// Updates keep the recorded backend, else the detected one; a never-recorded CPU build on
    /// GPU hardware (what the partial-upload bug installed) is held with a notice.
    #[test]
    fn test_update_backend() {
        use crate::hardware::RecommendedBackend::*;
        assert_eq!(
            update_backend(Some("vulkan"), Some("cpu"), Rocm).unwrap(),
            "vulkan"
        );
        assert_eq!(update_backend(Some("hip"), None, Vulkan).unwrap(), "rocm");
        assert_eq!(
            update_backend(None, Some("vulkan"), Rocm).unwrap(),
            "vulkan"
        );
        // Chosen explicitly (recorded): stays CPU.
        assert_eq!(
            update_backend(Some("cpu"), Some("cpu"), Rocm).unwrap(),
            "cpu"
        );
        // CPU-only hardware: CPU builds update normally.
        assert_eq!(update_backend(None, Some("cpu"), Cpu).unwrap(), "cpu");
        let e = update_backend(None, Some("cpu"), Rocm).unwrap_err();
        let (key, params) = crate::error_key::parse(&e).unwrap();
        assert_eq!(key, "runtime.update_cpu_on_gpu");
        assert_eq!(params["recommended"], "rocm");
        let e = update_backend(Some(" "), None, Vulkan).unwrap_err();
        assert!(e.starts_with("runtime.update_backend_unknown"), "{e}");
    }

    /// Real e2e (network, ~35 MB): the newest official Vulkan build for this machine through
    /// the same steps as Settings > Runtime and the CLI, into a temporary folder; it must run
    /// and list a Vulkan device where the machine has one.
    #[tokio::test]
    #[ignore] // network + download: cargo test -p harness-core -- --ignored install_managed
    async fn test_install_managed_vulkan_e2e() {
        let temp = tempfile::tempdir().unwrap();
        let mut mgr = RuntimeManager::default();
        mgr.base_bin_dir = Some(temp.path().to_path_buf());
        let runtime = Arc::new(mgr);
        let hw = crate::hardware::detect_hardware();
        let target = crate::runtime::install_query(&hw, None, None, Some("vulkan"));
        assert_eq!(target.mode, crate::runtime::PickMode::Exact);
        let release = runtime
            .check_latest_release(&target.query, target.mode)
            .await
            .unwrap();
        assert!(
            release.asset_name.contains("vulkan"),
            "{}",
            release.asset_name
        );
        assert!(release.unverified_assets().is_empty());
        let urls = vec![release.download_url.clone()];
        runtime
            .download_and_extract_staging(&urls, false)
            .await
            .unwrap();
        let text = runtime.verify_stage1_binary().unwrap();
        assert!(text.contains("version"), "{text}");
        runtime.verify_staged_backend("vulkan").unwrap();
        let m = runtime.activate_runtime(Some(&release.tag_name)).unwrap();
        assert_eq!(m.backend.as_deref(), Some("vulkan"));
        assert_eq!(runtime.installed_backend().as_deref(), Some("vulkan"));
        let devices = crate::devices::probe(&runtime.get_managed_binary_path().unwrap()).unwrap();
        eprintln!("{} devices: {:?}", release.tag_name, devices);
        if hw.gpus.iter().any(|g| g.is_discrete) {
            assert!(devices.iter().any(|d| d.id.starts_with("Vulkan")));
        }
    }

    /// A hold about the CPU build no longer shows once a Vulkan build was installed elsewhere
    /// (Control API / CLI); a hold about the installed build still does.
    #[test]
    fn test_hold_expires_when_build_changes() {
        let cpu = (Some("b11301".to_string()), None);
        let vulkan = (Some("b11302".to_string()), Some("vulkan".to_string()));
        let held = AutoUpdateStatus {
            phase: AutoUpdatePhase::KeptCurrent,
            message: Some("runtime.update_cpu_on_gpu".into()),
            install: Some(cpu.clone()),
            ..Default::default()
        };
        let still = held.clone().current(&cpu);
        assert_eq!(still.phase, AutoUpdatePhase::KeptCurrent);
        assert!(still.message.is_some());
        let stale = held.current(&vulkan);
        assert_eq!(stale.phase, AutoUpdatePhase::Idle);
        assert!(stale.message.is_none());
        let updated = AutoUpdateStatus {
            phase: AutoUpdatePhase::Updated,
            install: Some(cpu),
            ..Default::default()
        };
        assert_eq!(updated.current(&vulkan).phase, AutoUpdatePhase::Updated);
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("b10000", "b9999"));
        assert!(!is_newer("b9999", "b10000"));
        assert!(!is_newer("b5000", "b5000"));
        assert!(is_newer("b5001", "custom"));
    }
}
