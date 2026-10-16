//! Several llama-server instances side by side (one `ProcessManager` each), plus
//! named server profiles (a saved launch configuration that can be started in one click).

use crate::process::{ActiveServerInfo, LaunchConfig, ProcessManager, ServerStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

/// Instance used by the Dashboard and by every command that doesn't name a server.
pub const DEFAULT_SERVER_ID: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServerSummary {
    pub id: String,
    pub status: ServerStatus,
    /// Connection info while starting/running.
    pub active: Option<ActiveServerInfo>,
    /// Why the last run failed (classified; `None` while it runs).
    #[serde(default)]
    pub failure: Option<crate::failure::ServerFailure>,
    /// A scheduled automatic restart after a crash.
    #[serde(default)]
    pub restart: Option<crate::process::RestartState>,
}

pub fn validate_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(crate::error_key::err("servers.invalid_id")
            .with("id", id)
            .into())
    }
}

fn is_active(status: &ServerStatus) -> bool {
    matches!(
        status,
        ServerStatus::Starting | ServerStatus::Running | ServerStatus::Stopping
    )
}

type Instances = Mutex<BTreeMap<String, Arc<ProcessManager>>>;

// Observe the existing registries without keeping servers alive. Tools need every live
// listener, including direct servers when the gateway is stopped and transient launches.
fn live_registries() -> &'static Mutex<Vec<Weak<Instances>>> {
    static REGISTRIES: OnceLock<Mutex<Vec<Weak<Instances>>>> = OnceLock::new();
    REGISTRIES.get_or_init(Mutex::default)
}

pub(crate) fn live_listener_ports() -> Vec<u16> {
    let registries = {
        let mut live = live_registries().lock().unwrap();
        let registries: Vec<_> = live.iter().filter_map(Weak::upgrade).collect();
        live.retain(|registry| registry.strong_count() > 0);
        registries
    };
    registries
        .into_iter()
        .flat_map(|instances| {
            instances
                .lock()
                .unwrap()
                .values()
                .filter_map(|pm| {
                    pm.active_server().map(|active| active.port).or_else(|| {
                        // A stopping process can still accept connections until it exits.
                        (pm.get_status() == ServerStatus::Stopping).then(|| pm.get_active_port())
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub struct ServerRegistry {
    instances: Arc<Instances>,
}

impl Default for ServerRegistry {
    fn default() -> Self {
        let mut map = BTreeMap::new();
        map.insert(
            DEFAULT_SERVER_ID.to_string(),
            Arc::new(ProcessManager::with_id(DEFAULT_SERVER_ID)),
        );
        let instances = Arc::new(Mutex::new(map));
        live_registries()
            .lock()
            .unwrap()
            .push(Arc::downgrade(&instances));
        Self { instances }
    }
}

impl ServerRegistry {
    /// Resolves an optional id from the frontend (None = default instance).
    pub fn resolve_id(id: Option<String>) -> Result<String, String> {
        let id = id.unwrap_or_else(|| DEFAULT_SERVER_ID.to_string());
        validate_id(&id)?;
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Option<Arc<ProcessManager>> {
        self.instances.lock().unwrap().get(id).cloned()
    }

    pub fn get_or_create(&self, id: &str) -> Arc<ProcessManager> {
        let mut created = false;
        let pm = self
            .instances
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_insert_with(|| {
                created = true;
                Arc::new(ProcessManager::with_id(id))
            })
            .clone();
        if created {
            crate::changes::servers_changed();
        }
        pm
    }

    pub fn all(&self) -> Vec<(String, Arc<ProcessManager>)> {
        self.instances
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn summaries(&self) -> Vec<ServerSummary> {
        self.all()
            .into_iter()
            .map(|(id, pm)| ServerSummary {
                id,
                status: pm.get_status(),
                active: pm.active_server(),
                failure: pm.last_failure(),
                restart: pm.restart_state(),
            })
            .collect()
    }

    /// `(id, connection info)` of every starting/running server (for telemetry).
    pub fn active_servers(&self) -> Vec<(String, ActiveServerInfo)> {
        self.all()
            .into_iter()
            .filter_map(|(id, pm)| pm.active_server().map(|a| (id, a)))
            .collect()
    }

    /// llama-server processes of the starting/running servers (for the RAM breakdown).
    pub fn server_processes(&self) -> Vec<crate::telemetry::ServerProc> {
        self.all()
            .into_iter()
            .filter_map(|(id, pm)| {
                let active = pm.active_server()?;
                let name = active
                    .model_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| id.clone());
                Some(crate::telemetry::ServerProc {
                    pid: pm.pid()?,
                    id,
                    name,
                })
            })
            .collect()
    }

    pub fn any_active(&self) -> bool {
        self.all().iter().any(|(_, pm)| is_active(&pm.get_status()))
    }

    /// Another instance of ours already uses this port.
    pub fn port_conflict(&self, id: &str, port: u16) -> Option<String> {
        self.all().into_iter().find_map(|(other, pm)| {
            (other != id && pm.active_server().map(|a| a.port) == Some(port)).then_some(other)
        })
    }

    /// Model ids served by the other starting/running instances (not `id`).
    pub fn served_aliases(&self, id: &str) -> Vec<String> {
        self.all()
            .into_iter()
            .filter(|(other, pm)| other != id && pm.active_server().is_some())
            .filter_map(|(_, pm)| pm.launch_config())
            .filter_map(|c| crate::process::served_alias(&c))
            .collect()
    }

    /// First port >= `from` that no running instance, no saved profile (`reserved`) and no other
    /// program on this machine uses.
    pub fn suggest_port(&self, host: &str, from: u16, reserved: &[u16]) -> Option<u16> {
        let in_use: Vec<u16> = self
            .active_servers()
            .into_iter()
            .map(|(_, a)| a.port)
            .chain(reserved.iter().copied())
            .collect();
        (from.max(1)..=u16::MAX).find(|p| !in_use.contains(p) && ensure_port_free(host, *p).is_ok())
    }

    /// True if any active server has this model (or, in router mode, directory) loaded.
    pub fn model_in_use(&self, model_path: &str) -> bool {
        let target = crate::operation_gate::normalized_path(std::path::Path::new(model_path));
        self.all().iter().any(|(_, pm)| {
            pm.active_server()
                .map(|a| {
                    let active = crate::operation_gate::normalized_path(&a.model_path);
                    active == target || (a.router_mode && target.starts_with(&active))
                })
                .unwrap_or(false)
        })
    }

    /// Stops and removes a non-default instance (and its logs).
    pub async fn remove(&self, id: &str) -> Result<(), String> {
        if id == DEFAULT_SERVER_ID {
            return Err(crate::error_key::err("servers.default_not_removable").into());
        }
        let Some(pm) = self.get(id) else {
            return Ok(());
        };
        pm.stop().await?;
        if pm.has_child() || pm.get_status() != ServerStatus::Stopped {
            return Err(crate::error_key::err("servers.remove_still_running").into());
        }

        let mut map = self.instances.lock().unwrap();
        if map.get(id).is_some_and(|current| Arc::ptr_eq(current, &pm)) {
            map.remove(id);
            drop(map);
            crate::changes::servers_changed();
        }
        Ok(())
    }

    pub async fn stop_all(&self) {
        let deadline = Instant::now() + Duration::from_secs(7);
        let stops: Vec<_> = self
            .all()
            .into_iter()
            .map(|(_, pm)| tokio::spawn(async move { pm.stop_until(deadline).await }))
            .collect();
        // One deadline covers every instance. Dropping timed-out JoinHandles detaches the
        // cleanup tasks, so they continue trying without making application exit wait forever.
        let _ = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            futures_util::future::join_all(stops),
        )
        .await;
    }
}

/// Fails if something (ours or another program) is already listening on `host:port`.
///
/// Binding alone isn't enough: on Windows a wildcard bind (0.0.0.0) succeeds even
/// while another process listens on 127.0.0.1 with the same port, so we also try
/// to connect to the address clients would use.
pub fn ensure_port_free(host: &str, port: u16) -> Result<(), String> {
    use std::net::{TcpListener, TcpStream, ToSocketAddrs};
    let in_use = || {
        Err(format!(
            "{}: {}:{}",
            crate::orchestrator::ERR_PORT_IN_USE,
            host,
            port
        ))
    };

    let connect_host = crate::process::connect_host(host);
    let connect_host = connect_host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(addrs) = (connect_host, port).to_socket_addrs() {
        for addr in addrs {
            if TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(150)).is_ok() {
                return in_use();
            }
        }
    }

    let bind_host = match host {
        "" => "0.0.0.0",
        h => h.trim_start_matches('[').trim_end_matches(']'),
    };
    match TcpListener::bind((bind_host, port)) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => in_use(),
        // Other errors (e.g. unresolvable host) are left for llama-server to report.
        Err(_) => Ok(()),
    }
}

/// [`ensure_port_free`] before a launch on a port the user chose: the error names the program
/// holding it (`servers.port_in_use_by`) when it can be found.
pub fn check_port_free(host: &str, port: u16) -> Result<(), String> {
    ensure_port_free(host, port).map_err(|_| crate::ports::in_use_error(port))
}

// --- Last session (Dashboard's default server) ---
//
// Stored as the `"default"` profile in `server_profiles.json` (see `domain::store`).

/// The API key a launch will use: a key typed in the UI (`Some("")` = none), otherwise the
/// one saved in the keychain under `api_key_ref`. A keychain that cannot be read is an error,
/// never "no key".
pub fn resolve_api_key(config: &LaunchConfig) -> Result<Option<String>, String> {
    match config.api_key.as_deref().map(str::trim) {
        Some("") => Ok(None),
        Some(key) => Ok(Some(key.to_string())),
        None => match config.api_key_ref.as_deref() {
            Some(r) => crate::secrets::api_key_try_get(r),
            None => Ok(None),
        },
    }
}

/// Moves the config's key into the keychain under `owner` and leaves only the reference in it.
fn store_api_key(config: &mut LaunchConfig, owner: &str) -> Result<(), String> {
    config.api_key_ref = crate::secrets::adopt_api_key(
        owner,
        config.api_key.as_deref(),
        config.api_key_ref.as_deref(),
    )?;
    config.api_key = None;
    Ok(())
}

fn domain() -> crate::domain::store::Domain {
    crate::domain::store::Domain::global()
}

fn persist_last_session(config: &LaunchConfig) -> Result<(), String> {
    let mut config = config.clone();
    config.binary_path = Default::default();
    config.mcp_config_path = None;
    // `config.api_key` is the resolved key of this launch (None = no key).
    let key = config.api_key.clone().unwrap_or_default();
    config.api_key = Some(key);
    config.api_key_ref = None;
    store_api_key(&mut config, crate::secrets::SESSION_REF)?;
    domain().save_session(&config)
}

/// Remembers the default server's launch configuration for "restore last session".
/// If the API key can't be stored in the keychain nothing is saved (a restored session must
/// never silently start without the key the user configured).
pub fn save_last_session(config: &LaunchConfig) {
    if let Err(e) = persist_last_session(config) {
        eprintln!("[Harness Studio] Son oturum kaydedilemedi: {}", e);
        let _ = domain().clear_session();
    }
}

/// The Dashboard's own server (`"default"`) as every view shows it while it is not running.
///
/// One source of truth: the saved `"default"` profile (updated by every start of that
/// server). The settings' server defaults (`server.host/port/default_ngl/default_ctx`) are
/// only the starting point of a Dashboard server that was never saved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultServer {
    /// The saved launch, or the settings defaults (no model) when `saved` is false.
    pub config: LaunchConfig,
    /// `config` is the saved `"default"` profile.
    pub saved: bool,
}

/// See [`DefaultServer`]. Never fails: unreadable files fall back to the settings defaults.
pub fn default_server() -> DefaultServer {
    let defaults = crate::domain::launch::LaunchDefaults::load();
    match domain().default_server(&defaults) {
        Ok(server) => server,
        Err(e) => {
            eprintln!("[Harness Studio] Varsayılan sunucu okunamadı: {}", e);
            DefaultServer {
                config: crate::domain::launch::resolve_launch(
                    &defaults,
                    None,
                    None,
                    &Default::default(),
                )
                .unwrap_or_default(),
                saved: false,
            }
        }
    }
}

pub fn load_last_session() -> Option<LaunchConfig> {
    match domain().session(&crate::domain::launch::LaunchDefaults::load()) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("[Harness Studio] Son oturum okunamadı: {}", e);
            None
        }
    }
}

// --- Profiles ---

/// A server profile in the shape the GUI uses (`list_server_profiles` & co.): the effective
/// launch config of a `domain::ServerProfile` and its `ModelDefinition`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema), schema(as = ProfileView))]
pub struct ServerProfile {
    /// Also used as the server instance id when the profile is started.
    pub id: String,
    pub name: String,
    /// Model id (= launch path) the profile serves; informational.
    #[serde(default)]
    pub model_id: Option<String>,
    /// Short free-text purpose ("OpenCode", "Open Design"...); shown in the lists.
    #[serde(default)]
    pub description: String,
    pub config: LaunchConfig,
    /// Start this profile when Harness Studio launches.
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub updated_at: u64,
}

pub fn load_profiles() -> Vec<ServerProfile> {
    match domain().profile_views(&crate::domain::launch::LaunchDefaults::load()) {
        Ok(list) => list,
        Err(e) => {
            eprintln!("[Harness Studio] Sunucu profilleri okunamadı: {}", e);
            Vec::new()
        }
    }
}

/// Inserts or replaces (by id) a profile. The binary path is never persisted:
/// it is resolved from the runtime settings at launch.
pub fn save_profile(mut profile: ServerProfile) -> Result<Vec<ServerProfile>, String> {
    validate_id(&profile.id)?;
    if profile.id == DEFAULT_SERVER_ID {
        return Err(crate::error_key::err("servers.default_id_reserved").into());
    }
    profile.name = profile.name.trim().to_string();
    if profile.name.is_empty() {
        return Err(crate::error_key::err("servers.profile_name_empty").into());
    }
    profile.description = profile.description.trim().chars().take(200).collect();
    profile.config.binary_path = Default::default();
    profile.config.mcp_config_path = None;
    store_api_key(
        &mut profile.config,
        &crate::secrets::profile_ref(&profile.id),
    )?;
    domain().save_profile_view(&profile)?;
    Ok(load_profiles())
}

pub fn delete_profile(id: &str) -> Result<Vec<ServerProfile>, String> {
    if id == DEFAULT_SERVER_ID {
        return Err(crate::error_key::err("servers.default_id_reserved").into());
    }
    domain().delete_profile(id)?;
    crate::secrets::api_key_delete(&crate::secrets::profile_ref(id));
    Ok(load_profiles())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_id() {
        assert!(validate_id("default").is_ok());
        assert!(validate_id("qwen-coder_2").is_ok());
        assert!(validate_id("").is_err());
        assert!(validate_id("../x").is_err());
        assert!(validate_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn test_profile_plaintext_key_is_migrated_to_keychain() {
        let id = "mig-test-profile";
        let legacy = format!(
            r#"[{{"id":"{id}","name":"Legacy","config":{{"model_path":"m.gguf","gpu_layers":1,"context_size":512,"host":"127.0.0.1","port":8081,"api_key":"plain-secret-1"}}}}]"#
        );
        let path = crate::paths::profiles_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, legacy).unwrap();

        let profiles = load_profiles();
        let p = profiles.iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.config.api_key, None);
        assert_eq!(
            p.config.api_key_ref.as_deref(),
            Some("profile:mig-test-profile")
        );
        assert_eq!(
            crate::secrets::api_key_get("profile:mig-test-profile").as_deref(),
            Some("plain-secret-1")
        );
        // The legacy file was moved aside without the key; no config file contains it and
        // neither does the API JSON.
        assert!(!path.exists());
        for entry in std::fs::read_dir(path.parent().unwrap()).unwrap().flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            assert!(!text.contains("plain-secret-1"), "{:?}", entry.path());
        }
        assert!(!serde_json::to_string(&profiles)
            .unwrap()
            .contains("plain-secret-1"));
        // Resolving for launch reads the keychain.
        assert_eq!(
            resolve_api_key(&p.config).unwrap().as_deref(),
            Some("plain-secret-1")
        );

        delete_profile(id).unwrap();
        assert!(crate::secrets::api_key_get("profile:mig-test-profile").is_none());
        for e in std::fs::read_dir(path.parent().unwrap()).unwrap().flatten() {
            if e.file_name()
                .to_string_lossy()
                .starts_with("profiles.json.pre-v2")
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Every launch setting (and the name/description) survives save -> load.
    #[test]
    fn test_profile_round_trip_keeps_every_setting() {
        let id = "roundtrip-test-profile";
        let config = LaunchConfig {
            model_path: "models/a.gguf".into(),
            gpu_layers: 21,
            context_size: 16384,
            host: "0.0.0.0".to_string(),
            port: 18123,
            threads: Some(6),
            flash_attn: true,
            cache_type_k: Some("q8_0".to_string()),
            cache_type_v: Some("q4_0".to_string()),
            parallel_slots: Some(2),
            logical_batch: Some(1024),
            physical_batch: Some(256),
            context_shift: Some(true),
            split_mode: Some("layer".to_string()),
            main_gpu: Some(1),
            tensor_split: Some("3,1".to_string()),
            rope_scaling: Some("yarn".to_string()),
            rope_freq_base: Some(10000.0),
            rope_freq_scale: Some(0.5),
            chat_template: Some("chatml".to_string()),
            api_key: Some("roundtrip-secret".to_string()),
            custom_args: Some("--no-mmap --verbose".to_string()),
            draft_model_path: Some("models/draft.gguf".into()),
            draft_gpu_layers: Some(9),
            lora_path: Some("models/l.gguf".into()),
            lora_scale: Some(0.8),
            mmproj_path: Some("models/mm.gguf".into()),
            router_mode: false,
            mcp_enabled: true,
            ..Default::default()
        };
        let profile = ServerProfile {
            id: id.to_string(),
            name: "  OpenCode küçük model  ".to_string(),
            model_id: Some("models/a.gguf".to_string()),
            description: "  OpenCode  ".to_string(),
            config: config.clone(),
            auto_start: true,
            updated_at: 0,
        };
        save_profile(profile).unwrap();
        let mut loaded = load_profiles().into_iter().find(|p| p.id == id).unwrap();
        delete_profile(id).unwrap();
        // The resolved launch names the profile's definition and serves its alias (a
        // profile-only definition: `<stem>-<profile name>`).
        assert!(loaded
            .config
            .alias
            .take()
            .is_some_and(|a| a.starts_with("a-")));
        assert!(loaded.config.definition_id.take().is_some());

        assert_eq!(loaded.name, "OpenCode küçük model");
        assert_eq!(loaded.description, "OpenCode");
        assert!(loaded.auto_start);
        assert_eq!(loaded.model_id.as_deref(), Some("models/a.gguf"));
        // Compare everything except the key (moved to the keychain) through JSON.
        let mut expected = config;
        expected.api_key = None;
        expected.api_key_ref = Some(crate::secrets::profile_ref(id));
        assert_eq!(
            serde_json::to_value(&loaded.config).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }

    #[test]
    fn test_suggest_port_skips_reserved_and_listening() {
        let reg = ServerRegistry::default();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let busy = listener.local_addr().unwrap().port();
        let got = reg.suggest_port("127.0.0.1", busy, &[busy + 1]).unwrap();
        assert!(got > busy + 1, "got {got}, busy {busy}");
    }

    #[test]
    fn test_resolve_api_key_precedence() {
        let mut c = LaunchConfig::default();
        assert_eq!(resolve_api_key(&c).unwrap(), None);
        crate::secrets::adopt_api_key("profile:resolve-test", Some("saved"), None).unwrap();
        c.api_key_ref = Some("profile:resolve-test".to_string());
        assert_eq!(resolve_api_key(&c).unwrap().as_deref(), Some("saved"));
        c.api_key = Some("typed".to_string());
        assert_eq!(resolve_api_key(&c).unwrap().as_deref(), Some("typed"));
        c.api_key = Some("".to_string()); // explicit clear
        assert_eq!(resolve_api_key(&c).unwrap(), None);
        crate::secrets::api_key_delete("profile:resolve-test");
    }

    #[tokio::test]
    async fn test_registry_default_and_remove() {
        let reg = ServerRegistry::default();
        assert!(reg.get(DEFAULT_SERVER_ID).is_some());
        assert!(reg.remove(DEFAULT_SERVER_ID).await.is_err());
        reg.get_or_create("second");
        assert_eq!(reg.summaries().len(), 2);
        reg.remove("second").await.unwrap();
        assert_eq!(reg.summaries().len(), 1);
        assert!(!reg.any_active());
        assert_eq!(reg.port_conflict("x", 8080), None);
    }

    /// Holds a free port while `taken` runs, then checks `free` once it is released. A test
    /// running in parallel can be handed the freed port by its own `bind(0)`; then this tries
    /// again with another port.
    fn with_released_port(taken: impl Fn(u16), free: impl Fn(u16) -> bool) {
        for _ in 0..5 {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            taken(port);
            drop(listener);
            // macOS can take a moment to release a just-closed listening socket; poll briefly.
            for _ in 0..20 {
                if free(port) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        panic!("a released port was taken again every time");
    }

    #[test]
    fn test_ensure_port_free_detects_listener() {
        with_released_port(
            |port| assert!(ensure_port_free("127.0.0.1", port).is_err()),
            |port| ensure_port_free("127.0.0.1", port).is_ok(),
        );
    }

    #[test]
    fn test_check_port_free_names_the_program_holding_the_port() {
        with_released_port(
            |port| {
                let e = check_port_free("127.0.0.1", port).unwrap_err();
                assert!(e.starts_with(crate::orchestrator::ERR_PORT_IN_USE), "{e}");
                let (key, params) = crate::error_key::parse(&e).unwrap();
                assert_eq!(key, "servers.port_in_use_by", "{e}");
                assert_eq!(params["port"], port);
                assert_eq!(params["pid"], std::process::id());
                assert!(!params["program"].as_str().unwrap().is_empty());
            },
            |port| check_port_free("127.0.0.1", port).is_ok(),
        );
    }

    /// Two real servers side by side + telemetry for both. Run with
    /// HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<small .gguf> cargo test e2e -- --ignored
    #[tokio::test]
    #[ignore]
    async fn e2e_two_servers_concurrently() {
        use std::time::{Duration, Instant};
        let (Ok(bin), Ok(model)) = (
            std::env::var("HS_LLAMA_SERVER"),
            std::env::var("HS_TEST_MODEL"),
        ) else {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        };
        let reg = ServerRegistry::default();
        let cfg = |port| LaunchConfig {
            binary_path: bin.clone().into(),
            model_path: model.clone().into(),
            context_size: 512,
            host: "127.0.0.1".to_string(),
            port,
            ..Default::default()
        };
        reg.get_or_create("a").start(cfg(18091)).await.unwrap();
        reg.get_or_create("b").start(cfg(18092)).await.unwrap();
        assert_eq!(reg.port_conflict("c", 18091).as_deref(), Some("a"));

        let t = Instant::now();
        loop {
            let running = reg
                .summaries()
                .iter()
                .filter(|s| s.status == ServerStatus::Running)
                .count();
            if running == 2 {
                break;
            }
            assert!(
                t.elapsed() < Duration::from_secs(60),
                "{:?}",
                reg.summaries()
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        // A listening llama-server must be detected as "port in use".
        assert!(ensure_port_free("127.0.0.1", 18091).is_err());
        assert!(ensure_port_free("0.0.0.0", 18092).is_err());

        let mut poller = crate::telemetry::TelemetryPoller::new(None);
        poller.poll(&reg.active_servers()).await;
        assert!(poller.last("a").server_online && poller.last("b").server_online);
        assert_eq!(poller.get_history("a", None).len(), 1);

        reg.get("a").unwrap().stop().await.unwrap();
        poller.poll(&reg.active_servers()).await;
        assert!(!poller.last("a").server_online, "stopped server is offline");
        assert_eq!(poller.get_history("a", None).len(), 1, "history kept");
        assert!(poller.last("b").server_online);
        reg.stop_all().await;
        assert!(!reg.any_active());
    }

    /// Helper for `e2e_server_dies_with_app`: started as a child process, launches a
    /// server and exits abruptly without stopping it.
    #[tokio::test]
    #[ignore]
    async fn e2e_orphan_helper() {
        let Ok(port) = std::env::var("HS_ORPHAN_PORT") else {
            return;
        };
        let reg = ServerRegistry::default();
        reg.get_or_create("x")
            .start(LaunchConfig {
                binary_path: std::env::var("HS_LLAMA_SERVER").unwrap().into(),
                model_path: std::env::var("HS_TEST_MODEL").unwrap().into(),
                context_size: 512,
                host: "127.0.0.1".to_string(),
                port: port.parse().unwrap(),
                ..Default::default()
            })
            .await
            .unwrap();
        let t = std::time::Instant::now();
        while reg.get("x").unwrap().get_status() != ServerStatus::Running {
            assert!(t.elapsed() < std::time::Duration::from_secs(60));
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        std::process::exit(0); // no destructors, no stop(): simulates a crash
    }

    /// llama-server must not outlive Harness Studio (Windows job object kill-on-close).
    #[test]
    #[ignore]
    fn e2e_server_dies_with_app() {
        use std::time::{Duration, Instant};
        if std::env::var("HS_LLAMA_SERVER").is_err() || std::env::var("HS_TEST_MODEL").is_err() {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            return;
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["servers::tests::e2e_orphan_helper", "--ignored", "--exact"])
            .args(["--test-threads", "1"])
            .env("HS_ORPHAN_PORT", "18093")
            .status()
            .unwrap();
        assert!(status.success());
        if !cfg!(windows) {
            return; // Unix relies on the RunEvent::Exit cleanup instead
        }
        let t = Instant::now();
        while ensure_port_free("127.0.0.1", 18093).is_err() {
            assert!(
                t.elapsed() < Duration::from_secs(5),
                "llama-server outlived the app"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
