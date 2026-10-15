use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::paths;
use crate::secrets;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeMode {
    Managed,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ManagedRuntimeConfig {
    pub installed_version: Option<String>,
    /// Version kept in the backup slot (restored on rollback).
    pub previous_version: Option<String>,
    pub auto_update: bool,
    pub backend: Option<String>,
}

impl Default for ManagedRuntimeConfig {
    fn default() -> Self {
        Self {
            installed_version: None,
            previous_version: None,
            auto_update: true,
            backend: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct CustomBinaryConfig {
    pub binary_path: Option<PathBuf>,
    pub version: Option<String>,
    pub supports_metrics: bool,
    pub is_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Default bind address (Settings > Network & security): used by every launch whose
    /// profile/session has no host of its own. `127.0.0.1` = this computer only.
    pub host: String,
    /// Port of a Dashboard server that was never saved. 0 (the default) = automatic: Harness
    /// picks a free one from `ports::AUTO_RANGE` at launch. The old default 8080 of earlier
    /// files became 0 once (schema v3); a port set since is kept.
    pub port: u16,
    pub default_ngl: u32,
    /// Context size (`-c`) of a launch that sets none. 0 (the default) = automatic: a chat
    /// server gets the model's trained context, at most 32768 (`prepare::default_chat_context`);
    /// other servers keep 4096. The old shipped default 4096 is read as 0 (it cannot be told
    /// apart from a choice; a context the user wants fixed is any other number).
    #[serde(deserialize_with = "default_ctx_from_stored")]
    pub default_ctx: u32,
    /// Seconds a start waits for `/health` before it fails (`servers.load_timeout`). A launch
    /// can set its own (`load_timeout` param).
    pub load_timeout_secs: u32,
    /// Restart a server automatically after it crashed while running (off by default: a
    /// crash loop would keep taking GPU memory; see docs/en/ARCHITECTURE.md).
    pub auto_restart: bool,
    /// Restarts before giving up; the delay doubles from 2 s (2, 4, 8... at most 60 s). A run
    /// that stays up for 5 minutes resets the count.
    pub restart_max_attempts: u32,
    /// Measure a model once (a short llama-bench: CPU vs full GPU) the first time it loads on
    /// an integrated GPU, so the `-ngl` recommendation follows the measured direction
    /// (`auto_bench`).
    pub auto_benchmark: bool,
}

/// The context stored by builds before the automatic value existed (the old default).
const LEGACY_DEFAULT_CTX: u32 = 4096;

fn default_ctx_from_stored<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let n = u32::deserialize(d)?;
    Ok(if n == LEGACY_DEFAULT_CTX { 0 } else { n })
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 0,
            default_ngl: 33,
            default_ctx: 0,
            load_timeout_secs: 300,
            auto_restart: false,
            restart_max_attempts: 3,
            auto_benchmark: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct PrivacySettings {
    pub onboarding_completed: bool,
    pub crash_reports: bool,
    pub app_errors: bool,
    pub llama_errors: bool,
    pub hardware_info: bool,
    pub performance_metrics: bool,
    pub anonymous_analytics: bool,
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_language() -> String {
    "tr".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Experimental unauthenticated local TTS server. Explicit opt-in in Settings > Runtime > Installed Runtime > Other Runtimes.
    pub audio_cpp_enabled: bool,
    /// None preserves the full interface for existing installations.
    #[serde(default)]
    pub ui_level: Option<UiLevel>,
    pub runtime_mode: RuntimeMode,
    pub managed_runtime: ManagedRuntimeConfig,
    pub custom_binary: CustomBinaryConfig,
    pub server: ServerConfig,
    pub privacy: PrivacySettings,
    pub models_dir: String,
    pub extra_models_dirs: Vec<String>,
    /// Load a chat model right after its download finished (Settings > Storage > Downloads).
    /// Off by default: loading takes memory the user did not ask for.
    pub auto_load_after_download: bool,
    /// Never ask "save these load settings as the model's default?" when manual load changes
    /// that were used are left unsaved ("Don't Ask Again" in that prompt). Off by default.
    #[serde(default)]
    pub no_save_load_prompt: bool,
    /// Hugging Face token. Lives in the OS keychain only: read from settings.json solely to
    /// migrate old plaintext files, never serialized (neither to disk nor to the frontend).
    #[serde(skip_serializing)]
    pub hf_token: Option<String>,
    /// Whether a token is saved (this is all the frontend ever sees of it).
    pub hf_token_set: bool,
    /// Client id of the Hugging Face OAuth app used by "Sign in with Hugging Face" (public, not a
    /// secret). Empty = the `HARNESS_HF_OAUTH_CLIENT_ID` environment/build value (see `hf_auth`).
    #[serde(default)]
    pub hf_oauth_client_id: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Full colour theme id (`dark`, `light`, `system`, `oled`, `night`, `hc-dark`, `hc-light`).
    /// Empty = use `theme`. `theme` keeps the nearest base mode (dark/light/system) so older
    /// builds, which only know `theme`, still open in a sensible palette.
    #[serde(default)]
    pub color_theme: String,
    /// Colour-vision-deficiency-safe status/chart colours on top of any theme.
    #[serde(default)]
    pub cvd_safe: bool,
    #[serde(default = "default_language")]
    pub language: String,
    /// Interface size in percent (90/100/115/130/150). `None` = never chosen, the frontend picks
    /// a default from the screen width.
    #[serde(default)]
    pub ui_scale: Option<u32>,
    pub startup: StartupSettings,
    /// Bind/CORS/exposure settings (see `network`).
    pub network: crate::network::NetworkSettings,
    /// Harness Control API (off by default; see `control_api`).
    pub control_api: crate::control_api::ControlApiSettings,
    /// Gateway: one managed endpoint for every model (off by default; see `gateway`).
    pub gateway: crate::gateway::GatewaySettings,
    /// Request analytics (`analytics.db`; see `analytics`).
    pub analytics: crate::analytics::AnalyticsSettings,
    /// OpenTelemetry (OTLP) export of gateway traces and metrics (off by default; see `otel`).
    pub otel: crate::otel::OtelSettings,
    /// Tool engine policy: off / ask every time / allow trusted / allow all, the trusted tools
    /// and the default working directory (see `tools`).
    pub tools: crate::tools::ToolSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiLevel {
    Simple,
    Advanced,
}

/// What closing the main window does (like Ollama / LM Studio: the app can keep running in
/// the tray, and quitting is in the tray menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CloseAction {
    /// Ask on close (the first close, and every close until "remember" is ticked).
    #[default]
    Ask,
    /// Hide the window to the system tray; servers keep running.
    Tray,
    /// Quit the app (servers stop).
    Quit,
}

/// Background / tray behaviour. "Launch at login" itself lives in the OS
/// (autostart plugin), not here.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StartupSettings {
    /// Legacy switch (true = hide to the tray on close), still written in step with
    /// `close_action` so an older build behaves the same. Read through [`Self::close_action`].
    pub close_to_tray: bool,
    /// What closing the window does. `None` in files written before this choice existed:
    /// "tray" if `close_to_tray` was on, otherwise "ask".
    pub close_action: Option<CloseAction>,
    /// When started at login, stay in the tray instead of opening the window.
    pub start_minimized: bool,
    /// Relaunch the Dashboard's last server configuration on app start.
    pub restore_last_session: bool,
    /// Keep the window's web view loaded while hidden to the tray. Off (the default): it is
    /// closed once the window has been hidden a while with nothing in flight and rebuilt on
    /// "show", so the tray costs only the app process (16 instead of 73 MiB measured on
    /// Windows); reopening takes about half a second instead of being instant.
    pub keep_window_in_tray: bool,
}

impl StartupSettings {
    pub fn close_action(&self) -> CloseAction {
        self.close_action.unwrap_or(if self.close_to_tray {
            CloseAction::Tray
        } else {
            CloseAction::Ask
        })
    }

    pub fn set_close_action(&mut self, action: CloseAction) {
        self.close_action = Some(action);
        self.close_to_tray = action == CloseAction::Tray;
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            audio_cpp_enabled: false,
            runtime_mode: RuntimeMode::Managed,
            managed_runtime: ManagedRuntimeConfig::default(),
            custom_binary: CustomBinaryConfig::default(),
            server: ServerConfig::default(),
            privacy: PrivacySettings::default(),
            models_dir: paths::default_models_dir().to_string_lossy().to_string(),
            extra_models_dirs: Vec::new(),
            auto_load_after_download: false,
            no_save_load_prompt: false,
            hf_token: None,
            hf_token_set: false,
            hf_oauth_client_id: String::new(),
            theme: default_theme(),
            color_theme: String::new(),
            cvd_safe: false,
            language: default_language(),
            ui_scale: None,
            ui_level: None,
            startup: StartupSettings::default(),
            network: crate::network::NetworkSettings::default(),
            control_api: crate::control_api::ControlApiSettings::default(),
            gateway: crate::gateway::GatewaySettings::default(),
            analytics: crate::analytics::AnalyticsSettings::default(),
            otel: crate::otel::OtelSettings::default(),
            tools: crate::tools::ToolSettings::default(),
        }
    }
}

/// In-memory copy of the settings so hot paths don't hit disk/keychain each call.
static CACHE: Mutex<Option<AppSettings>> = Mutex::new(None);

/// `settings.json`: the settings object plus `schema_version` (files of older builds have no
/// version and are version 1).
///
/// * v2: `network` block (CORS origins, "open anyway" confirmations); `server.host` is
///   canonicalized and an unusable value falls back to `127.0.0.1` (never wider than before).
/// * v3: `server.port` 8080 becomes 0 (automatic) once. 8080 was the default before automatic
///   ports (01.10, `4c4aab2`) and every save wrote it, so it cannot be told apart from a choice;
///   the user decided to move it. A file at v3 keeps whatever port it has, 8080 included.
pub const SETTINGS_SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "settings.json",
    current: 3,
    envelope: crate::store::Envelope::Merged,
    migrations: &[settings_v1_to_v2, settings_v2_to_v3],
};

/// The old fixed default port of the Dashboard server.
const LEGACY_DEFAULT_PORT: u64 = 8080;

fn settings_v2_to_v3(mut doc: serde_json::Value) -> Result<serde_json::Value, String> {
    let map = doc
        .as_object_mut()
        .ok_or_else(|| String::from(crate::error_key::err("config.not_an_object")))?;
    if let Some(server) = map.get_mut("server").and_then(|s| s.as_object_mut()) {
        if server.get("port").and_then(|p| p.as_u64()) == Some(LEGACY_DEFAULT_PORT) {
            server.insert("port".into(), 0.into());
        }
    }
    Ok(doc)
}

fn settings_v1_to_v2(mut doc: serde_json::Value) -> Result<serde_json::Value, String> {
    let map = doc
        .as_object_mut()
        .ok_or_else(|| String::from(crate::error_key::err("config.not_an_object")))?;
    if let Some(server) = map.get_mut("server").and_then(|s| s.as_object_mut()) {
        if let Some(host) = server.get("host").and_then(|h| h.as_str()) {
            let host = crate::network::normalize_bind_host(host)
                .unwrap_or_else(|_| crate::network::DEFAULT_BIND_HOST.to_string());
            server.insert("host".into(), host.into());
        }
    }
    if !map.get("network").is_some_and(|n| n.is_object()) {
        map.insert(
            "network".into(),
            serde_json::to_value(crate::network::NetworkSettings::default()).map_err(|e| {
                String::from(crate::error_key::err("json.serialize_failed").with("error", e))
            })?,
        );
    }
    Ok(doc)
}

impl AppSettings {
    pub fn config_path() -> PathBuf {
        paths::settings_file()
    }

    /// Absolute path of the primary models directory.
    pub fn primary_models_dir(&self) -> PathBuf {
        paths::resolve_dir(&self.models_dir)
    }

    /// Absolute paths of all configured model directories (primary first, deduplicated).
    pub fn all_models_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.primary_models_dir()];
        for extra in &self.extra_models_dirs {
            let p = paths::resolve_dir(extra);
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
        dirs
    }

    pub fn load() -> Self {
        if let Some(cached) = CACHE.lock().unwrap().clone() {
            return cached;
        }
        let settings = Self::load_from_disk();
        *CACHE.lock().unwrap() = Some(settings.clone());
        settings
    }

    fn load_from_disk() -> Self {
        let path = Self::config_path();
        let mut settings = match crate::store::JsonStore::sensitive(&path)
            .load_versioned::<AppSettings>(&SETTINGS_SCHEMA)
        {
            Ok(Some(loaded)) => loaded.data,
            Ok(None) => Self::default(),
            Err(e) => {
                {
                    // Never silently overwrite a user's broken file: keep a backup first.
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let backup = path.with_file_name(format!("settings.corrupt-{}.json", ts));
                    let _ = fs::copy(&path, &backup);
                    eprintln!(
                        "[Harness Studio] settings.json could not be parsed ({}); backup saved to {:?}, using defaults",
                        e, backup
                    );
                    Self::default()
                }
            }
        };

        // Migrate the legacy single default folder in memory; the next save persists it.
        settings.tools = settings.tools.normalized();

        // Store library directories as absolute paths from here on.
        settings.models_dir = settings.primary_models_dir().to_string_lossy().to_string();
        settings.extra_models_dirs = settings
            .extra_models_dirs
            .iter()
            .map(|d| paths::resolve_dir(d).to_string_lossy().to_string())
            .collect();

        match settings.hf_token.clone().filter(|t| !t.trim().is_empty()) {
            // Legacy plaintext token found in the file: move it into the keychain and
            // rewrite the file without it. If the keychain fails the file is left untouched.
            Some(token) => match secrets::hf_set(token.trim()) {
                Ok(()) => {
                    let _ = settings.write_file();
                }
                Err(e) => eprintln!("[Harness Studio] {}", e),
            },
            None => settings.hf_token = secrets::hf_get(),
        }
        settings.hf_token_set = settings.hf_token.is_some();
        settings
    }

    /// The dedicated experimental-engine command must carry explicit risk consent.
    pub fn set_audio_cpp_enabled(
        &mut self,
        enabled: bool,
        acknowledged: bool,
    ) -> Result<(), String> {
        if enabled && !acknowledged {
            return Err(
                "Enabling audio.cpp requires acknowledgement of the experimental engine risks."
                    .into(),
            );
        }
        self.audio_cpp_enabled = enabled;
        Ok(())
    }

    /// Persists settings; the token changes only through its dedicated commands.
    pub fn save(&self) -> Result<(), String> {
        self.write_file()?;
        let mut cached = self.clone();
        cached.hf_token = Self::load().hf_token;
        cached.hf_token_set = cached.hf_token.is_some();
        *CACHE.lock().unwrap() = Some(cached);
        crate::models::invalidate_scan();
        Ok(())
    }

    /// Stores a new Hugging Face token in the OS keychain. Fails (instead of falling back
    /// to a plaintext file) when the keychain is unavailable.
    pub fn set_hf_token(token: &str) -> Result<(), String> {
        let token = token.trim();
        if token.is_empty() {
            return Err(crate::error_key::err("secrets.token_empty").into());
        }
        // A pasted token replaces a "Sign in with Hugging Face" session (removed first, so a
        // failure cannot leave the old session's token in charge).
        secrets::hf_oauth_delete()?;
        secrets::hf_set(token)?;
        Self::cache_hf_token(Some(token.to_string()));
        Ok(())
    }

    /// Removes the token and any OAuth session (sign out).
    pub fn clear_hf_token() -> Result<(), String> {
        secrets::hf_oauth_delete()?;
        secrets::hf_delete().map_err(|e| {
            String::from(crate::error_key::err("secrets.hf_delete_failed").with("error", e))
        })?;
        Self::cache_hf_token(None);
        Ok(())
    }

    /// Updates the in-memory token after the keychain changed (`hf_auth` login / refresh).
    pub(crate) fn cache_hf_token(token: Option<String>) {
        let mut settings = Self::load();
        settings.hf_token_set = token.is_some();
        settings.hf_token = token;
        *CACHE.lock().unwrap() = Some(settings);
    }

    /// Writes settings.json atomically (the token is never serialized).
    fn write_file(&self) -> Result<(), String> {
        // The legacy switch follows the choice (older builds read only `close_to_tray`).
        let mut doc = self.clone();
        let action = doc.startup.close_action();
        if doc.startup.close_action.is_some() {
            doc.startup.set_close_action(action);
        }
        crate::store::JsonStore::sensitive(Self::config_path())
            .save_versioned(&SETTINGS_SCHEMA, &doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_action_reads_the_legacy_switch_and_keeps_it_in_step() {
        let old: StartupSettings = serde_json::from_str(r#"{"close_to_tray":true}"#).unwrap();
        assert_eq!(old.close_action(), CloseAction::Tray);
        let old: StartupSettings = serde_json::from_str(r#"{"close_to_tray":false}"#).unwrap();
        assert_eq!(old.close_action(), CloseAction::Ask);
        assert_eq!(StartupSettings::default().close_action(), CloseAction::Ask);
        let mut s: StartupSettings =
            serde_json::from_str(r#"{"close_to_tray":true,"close_action":"quit"}"#).unwrap();
        assert_eq!(s.close_action(), CloseAction::Quit);
        s.set_close_action(CloseAction::Tray);
        assert!(s.close_to_tray);
        s.set_close_action(CloseAction::Quit);
        assert!(!s.close_to_tray);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["close_action"], "quit");
    }

    #[test]
    fn audio_cpp_enable_requires_explicit_acknowledgement() {
        let mut settings = AppSettings::default();
        assert!(settings.set_audio_cpp_enabled(true, false).is_err());
        assert!(!settings.audio_cpp_enabled);
        settings.set_audio_cpp_enabled(true, true).unwrap();
        assert!(settings.audio_cpp_enabled);
        assert!(settings.set_audio_cpp_enabled(true, false).is_err());
        assert!(settings.audio_cpp_enabled);
        settings.set_audio_cpp_enabled(false, false).unwrap();
        assert!(!settings.audio_cpp_enabled);
    }

    #[test]
    fn test_default_settings() {
        let settings = AppSettings::default();
        assert!(!settings.audio_cpp_enabled);
        assert!(
            !serde_json::from_str::<AppSettings>("{}")
                .unwrap()
                .audio_cpp_enabled
        );
        assert!(
            serde_json::from_str::<AppSettings>(r#"{"audio_cpp_enabled":true}"#)
                .unwrap()
                .audio_cpp_enabled
        );
        assert_eq!(settings.runtime_mode, RuntimeMode::Managed);
        assert_eq!(settings.server.port, 0);
        assert!(!settings.privacy.onboarding_completed);
        assert!(!settings.privacy.crash_reports);
    }

    #[test]
    fn test_settings_serialization() {
        let mut settings = AppSettings::default();
        settings.privacy.crash_reports = true;
        settings.server.port = 9000;

        let json_str = serde_json::to_string(&settings).unwrap();
        let parsed: AppSettings = serde_json::from_str(&json_str).unwrap();

        assert!(parsed.privacy.crash_reports);
        assert_eq!(parsed.server.port, 9000);
    }

    #[test]
    fn test_ui_level_defaults_and_roundtrip() {
        assert_eq!(AppSettings::default().ui_level, None);
        for json in [r#"{}"#, r#"{"ui_level":null}"#] {
            let old: AppSettings = serde_json::from_str(json).unwrap();
            assert_eq!(old.ui_level, None);
            let roundtrip: AppSettings =
                serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
            assert_eq!(roundtrip.ui_level, None);
        }
        for (name, level) in [("simple", UiLevel::Simple), ("advanced", UiLevel::Advanced)] {
            let set: AppSettings =
                serde_json::from_str(&format!(r#"{{"ui_level":"{name}"}}"#)).unwrap();
            assert_eq!(set.ui_level, Some(level));
            let json = serde_json::to_value(&set).unwrap();
            assert_eq!(json["ui_level"], name);
            let roundtrip: AppSettings = serde_json::from_value(json).unwrap();
            assert_eq!(roundtrip.ui_level, Some(level));
        }
    }

    #[test]
    fn test_ui_scale_defaults_and_roundtrip() {
        assert_eq!(AppSettings::default().ui_scale, None);
        let old: AppSettings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(old.ui_scale, None);
        let set: AppSettings = serde_json::from_str(r#"{"ui_scale":130}"#).unwrap();
        assert_eq!(set.ui_scale, Some(130));
    }

    #[test]
    fn test_color_theme_defaults_and_roundtrip() {
        // Files written before the extra themes have neither field.
        let old: AppSettings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(old.theme, "light");
        assert_eq!(old.color_theme, "");
        assert!(!old.cvd_safe);
        let set: AppSettings =
            serde_json::from_str(r#"{"theme":"dark","color_theme":"oled","cvd_safe":true}"#)
                .unwrap();
        let json = serde_json::to_value(&set).unwrap();
        assert_eq!(json["theme"], "dark");
        assert_eq!(json["color_theme"], "oled");
        assert_eq!(json["cvd_safe"], true);
    }

    #[test]
    fn auto_load_after_download_defaults_off_and_round_trips() {
        assert!(!AppSettings::default().auto_load_after_download);
        let old: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!old.auto_load_after_download);
        let on: AppSettings = serde_json::from_str(r#"{"auto_load_after_download":true}"#).unwrap();
        assert!(on.auto_load_after_download);
    }

    #[test]
    fn test_token_never_serialized() {
        let settings = AppSettings {
            hf_token: Some("hf_supersecret".to_string()),
            hf_token_set: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("hf_supersecret"));
        assert!(!json.contains("\"hf_token\""));
        assert!(json.contains("\"hf_token_set\":true"));
    }

    #[test]
    fn test_legacy_plaintext_token_still_deserializes() {
        // Old settings.json files carry `hf_token`; it must stay readable for migration.
        let parsed: AppSettings = serde_json::from_str(r#"{"hf_token":"hf_old"}"#).unwrap();
        assert_eq!(parsed.hf_token.as_deref(), Some("hf_old"));
        assert!(!parsed.hf_token_set);
    }

    /// A real v1 file (as written by an older build) on disk: migrated to v2 with a backup,
    /// defaults for the new block, an unusable host narrowed to loopback, other fields kept.
    #[test]
    fn test_settings_v1_file_migrates_to_v2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let v1 = r#"{
  "schema_version": 1,
  "runtime_mode": "Managed",
  "server": { "host": "localhost", "port": 8090, "default_ngl": 20, "default_ctx": 8192 },
  "theme": "light",
  "hf_token_set": false
}"#;
        std::fs::write(&path, v1).unwrap();
        let store = crate::store::JsonStore::sensitive(&path);
        let loaded = store
            .load_versioned::<AppSettings>(&SETTINGS_SCHEMA)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.version, 3);
        assert!(!loaded.read_only);
        assert_eq!(loaded.data.server.host, "127.0.0.1");
        assert_eq!(loaded.data.server.port, 8090);
        assert_eq!(loaded.data.theme, "light");
        assert!(loaded.data.network.cors_origins.is_empty());
        assert!(loaded.data.network.exposure_acks.is_empty());
        // Backup of the old version, rewritten file carries the new version and block.
        assert_eq!(std::fs::read_to_string(store.backup_path(1)).unwrap(), v1);
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["schema_version"], 3);
        assert!(doc["network"].is_object());

        // Unversioned legacy file with a garbage host and a LAN host.
        std::fs::write(&path, r#"{"server":{"host":"0.0.0.0,evil"}}"#).unwrap();
        let _ = std::fs::remove_file(store.backup_path(1));
        let loaded = store
            .load_versioned::<AppSettings>(&SETTINGS_SCHEMA)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.data.server.host, "127.0.0.1");
        std::fs::write(&path, r#"{"server":{"host":" 0.0.0.0 "}}"#).unwrap();
        let loaded = store
            .load_versioned::<AppSettings>(&SETTINGS_SCHEMA)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.data.server.host, "0.0.0.0");

        // A v2 file keeps the user's network block untouched.
        let v2 = r#"{"schema_version":2,"server":{"host":"192.168.1.20"},
            "network":{"cors_origins":["http://a.example"],
            "exposure_acks":{"default":{"acknowledged_at":5,"host":"0.0.0.0"}}}}"#;
        std::fs::write(&path, v2).unwrap();
        let loaded = store
            .load_versioned::<AppSettings>(&SETTINGS_SCHEMA)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.data.network.cors_origins, vec!["http://a.example"]);
        assert_eq!(
            loaded.data.network.exposure_acks["default"].acknowledged_at,
            5
        );
        assert_eq!(loaded.data.server.host, "192.168.1.20");
    }

    /// The old default port 8080 moves to automatic once (v2 -> v3) with a backup; another
    /// port stays, and an 8080 chosen after the move (a v3 file) is kept.
    #[test]
    fn test_settings_v3_moves_legacy_8080_to_automatic_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let store = crate::store::JsonStore::sensitive(&path);
        let load = || {
            store
                .load_versioned::<AppSettings>(&SETTINGS_SCHEMA)
                .unwrap()
                .unwrap()
        };
        let v2 =
            r#"{"schema_version":2,"server":{"host":"127.0.0.1","port":8080,"default_ngl":33}}"#;
        std::fs::write(&path, v2).unwrap();
        let loaded = load();
        assert_eq!((loaded.version, loaded.data.server.port), (3, 0));
        assert_eq!(loaded.data.server.default_ngl, 33);
        assert_eq!(std::fs::read_to_string(store.backup_path(2)).unwrap(), v2);
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (
                doc["schema_version"].as_u64(),
                doc["server"]["port"].as_u64()
            ),
            (Some(3), Some(0))
        );

        // The user picks 8080 again: saved at v3, the next load keeps it.
        let mut settings = loaded.data;
        settings.server.port = 8080;
        let doc = serde_json::to_value(&settings).unwrap();
        store.save_versioned(&SETTINGS_SCHEMA, &doc).unwrap();
        assert_eq!(load().data.server.port, 8080);

        // Another port, and an unversioned (v1) file with 8080.
        let _ = std::fs::remove_file(store.backup_path(2));
        std::fs::write(&path, r#"{"schema_version":2,"server":{"port":9000}}"#).unwrap();
        assert_eq!(load().data.server.port, 9000);
        std::fs::write(&path, r#"{"server":{"host":"127.0.0.1","port":8080}}"#).unwrap();
        assert_eq!(load().data.server.port, 0);
    }

    #[test]
    fn stored_old_default_context_means_automatic() {
        let load = |json: &str| serde_json::from_str::<AppSettings>(json).unwrap();
        assert_eq!(
            load(r#"{"server":{"default_ctx":4096}}"#)
                .server
                .default_ctx,
            0
        );
        assert_eq!(
            load(r#"{"server":{"default_ctx":8192}}"#)
                .server
                .default_ctx,
            8192
        );
        assert_eq!(load("{}").server.default_ctx, 0);
        assert_eq!(ServerConfig::default().default_ctx, 0);
    }

    #[test]
    fn test_set_and_clear_token_uses_keychain() {
        AppSettings::set_hf_token("  hf_abc ").unwrap();
        assert_eq!(secrets::hf_get().as_deref(), Some("hf_abc"));
        assert!(AppSettings::set_hf_token("  ").is_err());
        AppSettings::clear_hf_token().unwrap();
        assert!(secrets::hf_get().is_none());
    }
}
