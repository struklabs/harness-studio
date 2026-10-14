//! Runtime capability registry: what a specific llama-server binary supports.
//!
//! Each binary is probed once (`--version`, `--help`, `--list-devices`) and the result is
//! cached in memory and in `config/runtime_capabilities.json`, keyed by path + mtime + size
//! (build and commit are recorded too). Endpoint checks made after a server starts are
//! stored on the same entry, so they reset automatically when the binary changes.
//!
//! "The flag is in `--help`" only means a setting can be shown; see [`features`] for the
//! separate capability levels.

pub mod audio_cpp;
pub mod crispasr;
pub mod features;
pub mod lemonade;
pub mod mistralrs;
pub mod parse;
pub mod sdcpp;
pub mod sherpa;
pub mod verify;
pub mod whisper;

pub use parse::{Device, FlagInfo, VersionInfo};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

/// Bump when the persisted shape changes; older files are ignored (and re-probed).
pub const SCHEMA_VERSION: u32 = 1;
/// Binaries remembered on disk (most recently probed first).
const MAX_PERSISTED: usize = 6;
/// Models remembered per binary for model-dependent observations.
const MAX_MODELS: usize = 32;
/// `--list-devices` initializes every backend (Vulkan/CUDA), which can take a while.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// `--help` is ~60 KB today; leave room for growth.
const PROBE_MAX_BYTES: usize = 1024 * 1024;

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Key for per-model observations: `/props` reports `C:/x/m.gguf`, the UI `C:\x\m.gguf`.
pub fn model_key(path: &str) -> String {
    let p = path.replace('\\', "/");
    if cfg!(windows) {
        p.to_lowercase()
    } else {
        p
    }
}

/// Identity of a binary on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BinaryKey {
    pub path: PathBuf,
    /// Modification time in ms since the Unix epoch.
    pub mtime_ms: Option<u64>,
    pub size: Option<u64>,
    pub build: Option<u64>,
    pub commit: Option<String>,
    pub version: Option<String>,
}

impl BinaryKey {
    /// Cheap part of the key (no process spawn): path + mtime + size.
    fn file_id(path: &Path) -> (PathBuf, Option<u64>, Option<u64>) {
        let meta = std::fs::metadata(path).ok();
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        (crate::paths::normalize(path), mtime, meta.map(|m| m.len()))
    }

    /// Stable id of this binary (used as the cache key and in the UI).
    pub fn id(&self) -> String {
        format!(
            "{}|{}|{}",
            self.path.to_string_lossy(),
            self.mtime_ms.unwrap_or(0),
            self.size.unwrap_or(0)
        )
    }

    fn matches(&self, file: &(PathBuf, Option<u64>, Option<u64>)) -> bool {
        self.path == file.0 && self.mtime_ms == file.1 && self.size == file.2
    }
}

/// Result of a best-effort check against a running server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verification {
    pub ok: bool,
    /// Short technical detail (`HTTP 404`, metric count, ...); not user-facing prose.
    pub detail: Option<String>,
    /// Unix seconds.
    pub verified_at: u64,
}

/// What a run of a model showed through `/props` and `/v1/models`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelObservation {
    /// `/props.modalities` (`vision`, `audio`, `video` -> bool).
    #[serde(default)]
    pub modalities: BTreeMap<String, bool>,
    /// `/props.chat_template_caps` (`supports_tools`, `supports_reasoning_effort`, ...).
    #[serde(default)]
    pub chat_template_caps: BTreeMap<String, bool>,
    /// The chat template the server really uses (`/props.chat_template`, so a definition's
    /// override counts) lets the model reason, by the same text check as the GGUF template
    /// (`gguf::template_supports_reasoning`). `None`: the server sent no template.
    #[serde(default)]
    pub template_reasoning: Option<bool>,
    #[serde(default)]
    pub reasoning_control: crate::reasoning::ReasoningControl,
    pub n_ctx_train: Option<u64>,
    /// Whether an mmproj was loaded in the observed run (vision depends on it).
    #[serde(default)]
    pub with_mmproj: bool,
    pub observed_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub schema_version: u32,
    pub key: BinaryKey,
    pub version: VersionInfo,
    /// `--help` produced a flag table (false = probe failed; nothing is known).
    pub help_parsed: bool,
    /// Canonical long flag -> info.
    pub flags: BTreeMap<String, FlagInfo>,
    pub devices: Vec<Device>,
    /// Unix seconds.
    pub probed_at: u64,
    /// Feature id -> result of a check against a running server of this binary.
    #[serde(default)]
    pub endpoint_verified: BTreeMap<String, Verification>,
    /// Metric names seen on `/metrics`.
    #[serde(default)]
    pub metric_names: Vec<String>,
    /// Model path -> what a run of that model showed.
    #[serde(default)]
    pub models: BTreeMap<String, ModelObservation>,
    /// Spelling -> canonical flag (rebuilt after load).
    #[serde(skip)]
    alias_index: HashMap<String, String>,
}

impl RuntimeCapabilities {
    /// Builds the capabilities from the raw probe outputs.
    pub fn from_outputs(key: BinaryKey, version: &str, help: &str, devices: &str) -> Self {
        let version = parse::parse_version(version);
        let flags = parse::parse_help(help);
        let mut caps = Self {
            schema_version: SCHEMA_VERSION,
            key: BinaryKey {
                build: version.build,
                commit: version.commit.clone(),
                version: version.version.clone(),
                ..key
            },
            help_parsed: !flags.is_empty(),
            version,
            flags,
            devices: parse::parse_list_devices(devices),
            probed_at: now_secs(),
            ..Default::default()
        };
        caps.reindex();
        caps
    }

    pub(crate) fn reindex(&mut self) {
        self.alias_index = self
            .flags
            .iter()
            .flat_map(|(canonical, info)| {
                info.aliases
                    .iter()
                    .map(move |a| (a.clone(), canonical.clone()))
            })
            .collect();
    }

    /// Looks a flag up by any of its spellings (including removed ones).
    pub fn flag(&self, spelling: &str) -> Option<&FlagInfo> {
        let canonical = self.alias_index.get(spelling)?;
        self.flags.get(canonical)
    }

    /// The flag exists and is usable (not listed as removed).
    pub fn has_flag(&self, spelling: &str) -> bool {
        self.flag(spelling).is_some_and(|f| !f.removed)
    }

    /// First spelling of `candidates` this binary accepts.
    pub fn first_flag<'a>(&self, candidates: &'a [String]) -> Option<&'a str> {
        candidates
            .iter()
            .map(String::as_str)
            .find(|c| self.has_flag(c))
    }

    pub fn verification(&self, feature: &str) -> Option<&Verification> {
        self.endpoint_verified.get(feature)
    }

    /// Flag dialect used by `build_args` for the typed `LaunchConfig` fields.
    pub fn server_caps(&self) -> crate::process::ServerCaps {
        let flash_attn_takes_value = self
            .flag("--flash-attn")
            .is_some_and(|f| f.value_hint.is_some());
        let models_dir_flag = ["--models-dir", "--models-path"]
            .into_iter()
            .find(|f| self.has_flag(f))
            .map(str::to_string);
        crate::process::ServerCaps {
            flash_attn_takes_value,
            models_dir_flag,
            supports_metrics: self.has_flag("--metrics"),
            supports_tools: self.has_flag("--tools"),
            supports_mcp: self.has_flag("--mcp-servers-config"),
            supports_cors_origins: self.has_flag("--cors-origins"),
        }
    }

    /// The help looked like a llama-server flag table (same test the old `ServerCaps` used).
    pub fn looks_like_server(&self) -> bool {
        self.has_flag("--gpu-layers") || self.has_flag("-ngl")
    }

    /// Distinct backends of the devices (`Vulkan`, `CUDA`, ...).
    pub fn backends(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for d in &self.devices {
            if !out.contains(&d.backend) {
                out.push(d.backend.clone());
            }
        }
        out
    }
}

/// Runs `binary <arg>` without a console window and returns stdout + stderr
/// (llama.cpp prints `--version` to stderr and `--help` to stdout).
fn probe_output(binary: &Path, arg: &str) -> Option<String> {
    probe_output_args(binary, &[arg])
}

/// [`probe_output`] with several arguments (`mistralrs serve --help`).
fn probe_output_args(binary: &Path, args: &[&str]) -> Option<String> {
    let cwd = binary.parent().unwrap_or_else(|| Path::new("."));
    let mut cmd = crate::process::quiet_command(binary);
    cmd.args(args).current_dir(cwd);
    let out =
        crate::runtime::run_command_with_timeout(&mut cmd, PROBE_TIMEOUT, PROBE_MAX_BYTES).ok()?;
    Some(format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// Probes a binary (three short process runs). Blocking.
pub fn probe_binary(binary: &Path) -> RuntimeCapabilities {
    let (path, mtime_ms, size) = BinaryKey::file_id(binary);
    let key = BinaryKey {
        path,
        mtime_ms,
        size,
        ..Default::default()
    };
    let version = probe_output(binary, "--version").unwrap_or_default();
    let help = probe_output(binary, "--help").unwrap_or_default();
    let devices = probe_output(binary, "--list-devices").unwrap_or_default();
    RuntimeCapabilities::from_outputs(key, &version, &help, &devices)
}

/// `runtime_capabilities.json`: `{schema_version, entries: [...]}` (a cache: entries of another
/// entry schema are dropped on load; a file of a newer build is read but never overwritten).
pub const FILE_SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "runtime_capabilities.json",
    current: SCHEMA_VERSION,
    envelope: crate::store::Envelope::Merged,
    migrations: &[],
};

#[derive(Serialize, Deserialize, Default)]
struct PersistedFile {
    #[serde(default)]
    entries: Vec<RuntimeCapabilities>,
}

/// In-memory + on-disk cache of [`RuntimeCapabilities`], one entry per binary.
pub struct CapabilityRegistry {
    file: Option<PathBuf>,
    entries: Mutex<Option<Vec<Arc<RuntimeCapabilities>>>>,
    /// Serializes probes so two callers don't spawn the same binary three times each.
    probe_lock: Mutex<()>,
}

impl CapabilityRegistry {
    /// Registry persisted to `file` (`None` = memory only).
    pub fn with_file(file: Option<PathBuf>) -> Self {
        Self {
            file,
            entries: Mutex::new(None),
            probe_lock: Mutex::new(()),
        }
    }

    fn with_entries<R>(&self, f: impl FnOnce(&mut Vec<Arc<RuntimeCapabilities>>) -> R) -> R {
        let mut guard = self.entries.lock().unwrap();
        let entries = guard.get_or_insert_with(|| self.load_file());
        f(entries)
    }

    fn load_file(&self) -> Vec<Arc<RuntimeCapabilities>> {
        let Some(file) = &self.file else {
            return Vec::new();
        };
        let parsed = crate::store::JsonStore::new(file)
            .load_versioned::<PersistedFile>(&FILE_SCHEMA)
            .ok()
            .flatten();
        match parsed {
            Some(p) => p
                .data
                .entries
                .into_iter()
                .filter(|e| e.schema_version == SCHEMA_VERSION)
                .map(|mut e| {
                    e.reindex();
                    Arc::new(e)
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn persist(&self, entries: &[Arc<RuntimeCapabilities>]) {
        let Some(file) = &self.file else {
            return;
        };
        let mut list: Vec<RuntimeCapabilities> = entries
            .iter()
            .filter(|e| e.help_parsed)
            .map(|e| (**e).clone())
            .collect();
        list.sort_by_key(|e| std::cmp::Reverse(e.probed_at));
        list.truncate(MAX_PERSISTED);
        let data = PersistedFile { entries: list };
        if let Err(e) = crate::store::JsonStore::new(file).save_versioned(&FILE_SCHEMA, &data) {
            eprintln!("[Harness Studio] runtime capability cache not saved: {}", e);
        }
    }

    /// Cached capabilities of `binary` if the file hasn't changed since it was probed.
    pub fn cached(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        let file = BinaryKey::file_id(binary);
        self.with_entries(|entries| entries.iter().find(|e| e.key.matches(&file)).cloned())
    }

    /// Cached capabilities, probing the binary first when it is new or changed. Blocking.
    pub fn get_or_probe(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        self.get_or_probe_with(binary, probe_binary)
    }

    /// [`CapabilityRegistry::get_or_probe`] with the probe of another runtime family
    /// (whisper-server has no `--version` / `--list-devices`, see [`whisper`]). Blocking.
    pub fn get_or_probe_with(
        &self,
        binary: &Path,
        probe: impl FnOnce(&Path) -> RuntimeCapabilities,
    ) -> Arc<RuntimeCapabilities> {
        if let Some(hit) = self.cached(binary) {
            return hit;
        }
        let _probe = self.probe_lock.lock().unwrap();
        if let Some(hit) = self.cached(binary) {
            return hit;
        }
        let caps = Arc::new(probe(binary));
        self.insert(caps.clone());
        caps
    }

    /// Adds (or replaces) an entry; failed probes stay in memory only so they are retried
    /// after a restart.
    pub fn insert(&self, caps: Arc<RuntimeCapabilities>) {
        self.with_entries(|entries| {
            entries.retain(|e| e.key.path != caps.key.path);
            entries.push(caps);
            self.persist(entries);
        });
    }

    /// Applies `f` to the entry with binary id `id` (copy-on-write) and persists it.
    fn update(&self, id: &str, f: impl FnOnce(&mut RuntimeCapabilities)) -> bool {
        self.with_entries(|entries| {
            let Some(slot) = entries.iter_mut().find(|e| e.key.id() == id) else {
                return false;
            };
            let mut next = (**slot).clone();
            f(&mut next);
            next.reindex();
            *slot = Arc::new(next);
            self.persist(entries);
            true
        })
    }

    /// What the newest run of `model_path` showed through `/props`, across every remembered
    /// binary (a model keeps its observation when the runtime is updated and not yet re-run).
    pub fn observation_for(&self, model_path: &str) -> Option<ModelObservation> {
        let key = model_key(model_path);
        self.with_entries(|entries| {
            entries
                .iter()
                .filter_map(|e| e.models.get(&key))
                .max_by_key(|o| o.observed_at)
                .cloned()
        })
    }

    /// Stores the results of an endpoint check (see [`verify`]).
    pub fn record_probe(&self, binary_id: &str, report: &verify::ProbeReport) -> bool {
        self.update(binary_id, |caps| {
            for (feature, v) in &report.features {
                caps.endpoint_verified.insert(feature.clone(), v.clone());
            }
            if !report.metric_names.is_empty() {
                caps.metric_names = report.metric_names.clone();
            }
            if let (Some(model), Some(obs)) = (&report.model_path, &report.model) {
                caps.models.insert(model_key(model), obs.clone());
                if caps.models.len() > MAX_MODELS {
                    if let Some(oldest) = caps
                        .models
                        .iter()
                        .min_by_key(|(_, o)| o.observed_at)
                        .map(|(k, _)| k.clone())
                    {
                        caps.models.remove(&oldest);
                    }
                }
            }
        })
    }
}

/// A `--help` flag for the "all flags" explorer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RuntimeFlag {
    #[serde(flatten)]
    pub info: FlagInfo,
    /// Catalog parameter that already covers this flag (set it there instead).
    pub param_id: Option<String>,
    /// Harness Studio manages this flag itself; it can't be added as a custom argument.
    pub protected: bool,
}

/// The flags of a build with the catalog parameter of the build's family that covers each.
pub fn runtime_flags(
    engine: &dyn crate::engine::Engine,
    caps: &RuntimeCapabilities,
) -> Vec<RuntimeFlag> {
    let catalog = engine.param_catalog();
    caps.flags
        .values()
        .map(|info| RuntimeFlag {
            param_id: info
                .aliases
                .iter()
                .find_map(|a| catalog.id_for_flag(a))
                .map(str::to_string),
            protected: info.aliases.iter().any(|a| crate::params::is_protected(a)),
            info: info.clone(),
        })
        .collect()
}

/// Process-wide registry persisted under the config directory.
pub fn registry() -> &'static CapabilityRegistry {
    static REGISTRY: OnceLock<CapabilityRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        CapabilityRegistry::with_file(Some(
            crate::paths::config_dir().join("runtime_capabilities.json"),
        ))
    })
}

#[cfg(test)]
pub(crate) mod testdata {
    use super::*;

    pub fn read(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e))
    }

    /// Capabilities built from the committed real outputs of a build (`b11236`, `b11026`).
    pub fn caps(build: &str) -> RuntimeCapabilities {
        RuntimeCapabilities::from_outputs(
            BinaryKey {
                path: PathBuf::from(format!("C:/rt/{build}/llama-server.exe")),
                mtime_ms: Some(1),
                size: Some(2),
                ..Default::default()
            },
            &read(&format!("llama-server-{build}.version.txt")),
            &read(&format!("llama-server-{build}.help.txt")),
            &read(&format!("llama-server-{build}.list-devices.txt")),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_from_real_outputs() {
        for build in ["b11236", "b11026"] {
            let caps = testdata::caps(build);
            assert!(caps.help_parsed);
            assert!(caps.looks_like_server());
            assert_eq!(caps.key.build, caps.version.build);
            assert!(caps.has_flag("-md"));
            assert!(caps.has_flag("--model-draft"));
            assert_eq!(caps.flag("-md").unwrap().canonical, "--spec-draft-model");
            assert!(
                !caps.has_flag("--draft-max"),
                "removed flags are not usable"
            );
            assert!(caps.flag("--draft-max").is_some());
            assert_eq!(caps.backends(), vec!["Vulkan"]);
            let sc = caps.server_caps();
            assert!(sc.flash_attn_takes_value);
            assert_eq!(sc.models_dir_flag.as_deref(), Some("--models-dir"));
            assert!(sc.supports_metrics && sc.supports_tools && sc.supports_mcp);
        }
    }

    #[test]
    fn runtime_flags_mark_catalog_and_protected() {
        let caps = testdata::caps("b11236");
        let flags = runtime_flags(crate::engine::llama_cpp(), &caps);
        let by = |name: &str| flags.iter().find(|f| f.info.canonical == name).unwrap();
        assert_eq!(by("--jinja").param_id.as_deref(), Some("jinja"));
        assert_eq!(
            by("--spec-draft-model").param_id.as_deref(),
            Some("draft_model_path")
        );
        assert!(by("--host").protected);
        assert!(by("--api-key").protected);
        assert!(by("--agent").protected);
        assert!(!by("--verbose").protected);
        assert!(by("--verbose").param_id.is_none());
    }

    #[test]
    fn registry_persists_and_reloads_by_file_identity() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("caps.json");
        // Any real file works as the "binary" identity; probing is not exercised here.
        let bin = dir.path().join("llama-server.exe");
        std::fs::write(&bin, b"x").unwrap();

        let reg = CapabilityRegistry::with_file(Some(file.clone()));
        let mut caps = testdata::caps("b11236");
        let (path, mtime_ms, size) = BinaryKey::file_id(&bin);
        caps.key.path = path;
        caps.key.mtime_ms = mtime_ms;
        caps.key.size = size;
        let id = caps.key.id();
        reg.insert(Arc::new(caps));
        assert!(reg.cached(&bin).is_some());

        let report = verify::ProbeReport {
            features: BTreeMap::from([(
                "metrics".to_string(),
                Verification {
                    ok: true,
                    detail: None,
                    verified_at: 5,
                },
            )]),
            metric_names: vec!["llamacpp:prompt_tokens_total".to_string()],
            ..Default::default()
        };
        assert!(reg.record_probe(&id, &report));

        let reloaded = CapabilityRegistry::with_file(Some(file.clone()));
        let hit = reloaded.cached(&bin).expect("persisted entry");
        assert!(hit.has_flag("-md"), "alias index rebuilt after load");
        assert!(hit.verification("metrics").unwrap().ok);
        assert_eq!(hit.metric_names.len(), 1);

        // A changed binary is a different key: the old entry no longer matches.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&bin, b"changed").unwrap();
        assert!(reloaded.cached(&bin).is_none());

        // Unknown schema versions are ignored.
        std::fs::write(&file, r#"{"schema_version":999,"entries":[]}"#).unwrap();
        assert!(CapabilityRegistry::with_file(Some(file))
            .cached(&bin)
            .is_none());
    }
}

/// Real binary probe. Run with:
/// HS_LLAMA_SERVER=<llama-server> cargo test -p harness-core capabilities::e2e -- --ignored
#[cfg(test)]
mod e2e {
    use super::*;

    #[test]
    #[ignore]
    fn test_real_binary_probe() {
        let Ok(bin) = std::env::var("HS_LLAMA_SERVER") else {
            eprintln!("HS_LLAMA_SERVER not set, skipping");
            return;
        };
        let reg = CapabilityRegistry::with_file(None);
        let caps = reg.get_or_probe(Path::new(&bin));
        assert!(caps.help_parsed, "help not parsed");
        assert!(caps.version.build.is_some(), "{:?}", caps.version);
        assert!(caps.looks_like_server());
        assert!(caps.flags.len() > 200);
        eprintln!(
            "build {:?} commit {:?}: {} flags, devices {:?}",
            caps.version.build,
            caps.version.commit,
            caps.flags.len(),
            caps.devices
        );
        // Second call is served from the cache (same Arc).
        assert!(Arc::ptr_eq(&caps, &reg.get_or_probe(Path::new(&bin))));
    }
}
