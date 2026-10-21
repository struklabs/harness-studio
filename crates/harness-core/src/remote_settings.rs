//! The app settings other clients (the Control API, the mobile client) may read and change: an
//! allow-list of preferences with no security weight. Appearance, window and startup
//! behaviour, the load-after-download preference and request analytics.
//!
//! Never part of it: secrets (tokens, keys, `hf_*`), bind addresses, CORS and exposure
//! (`network`, `server`, `gateway`, `control_api`), the tool policy, folders, runtimes, privacy
//! and crash reporting, and connections to other services (links, n8n, ComfyUI, OTLP). Those
//! keep their own, narrower routes or stay desktop-only.

use crate::config::{AppSettings, CloseAction, UiLevel};
use serde::{Deserialize, Serialize};

/// Colour themes of the desktop UI (`src/lib/theme.ts`) and the base mode older builds read.
const THEMES: [(&str, &str); 7] = [
    ("system", "system"),
    ("dark", "dark"),
    ("light", "light"),
    ("oled", "dark"),
    ("night", "dark"),
    ("hc-dark", "dark"),
    ("hc-light", "light"),
];
/// Interface sizes in percent (`src/lib/uiScale.ts`).
const UI_SCALES: [u32; 5] = [90, 100, 115, 130, 150];
const LANGUAGES: [&str; 2] = ["tr", "en"];
const RETENTION_DAYS: std::ops::RangeInclusive<u32> = 1..=3650;

/// The readable settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RemoteSettings {
    /// `tr` or `en`.
    pub language: String,
    /// Colour theme: `system`, `dark`, `light`, `oled`, `night`, `hc-dark`, `hc-light`.
    pub theme: String,
    /// Colour-vision-safe status and chart colours.
    pub cvd_safe: bool,
    /// Interface size in percent (90, 100, 115, 130, 150); null = chosen from the screen.
    pub ui_scale: Option<u32>,
    /// `simple` or `advanced`; null = the full interface of an older installation.
    pub ui_level: Option<String>,
    /// Closing the window: `ask`, `tray` or `quit`.
    pub close_action: String,
    /// Started at login: stay in the tray.
    pub start_minimized: bool,
    /// Relaunch the last server configuration on start.
    pub restore_last_session: bool,
    /// Keep the hidden window's web view loaded.
    pub keep_window_in_tray: bool,
    /// Load a chat model right after its download.
    pub auto_load_after_download: bool,
    /// Record gateway request metadata.
    pub analytics_enabled: bool,
    /// Days raw request rows are kept (1–3650).
    pub analytics_retention_days: u32,
}

/// A change: only the fields given are written. Unknown fields (anything outside the
/// allow-list) are refused.
#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RemoteSettingsPatch {
    pub language: Option<String>,
    pub theme: Option<String>,
    pub cvd_safe: Option<bool>,
    pub ui_scale: Option<u32>,
    pub ui_level: Option<String>,
    pub close_action: Option<String>,
    pub start_minimized: Option<bool>,
    pub restore_last_session: Option<bool>,
    pub keep_window_in_tray: Option<bool>,
    pub auto_load_after_download: Option<bool>,
    pub analytics_enabled: Option<bool>,
    pub analytics_retention_days: Option<u32>,
}

fn close_action_name(action: CloseAction) -> &'static str {
    match action {
        CloseAction::Ask => "ask",
        CloseAction::Tray => "tray",
        CloseAction::Quit => "quit",
    }
}

/// The allow-listed view of `settings`.
pub fn read(settings: &AppSettings) -> RemoteSettings {
    let theme = if settings.color_theme.is_empty() {
        settings.theme.clone()
    } else {
        settings.color_theme.clone()
    };
    RemoteSettings {
        language: settings.language.clone(),
        theme,
        cvd_safe: settings.cvd_safe,
        ui_scale: settings.ui_scale,
        ui_level: settings.ui_level.map(|l| {
            match l {
                UiLevel::Simple => "simple",
                UiLevel::Advanced => "advanced",
            }
            .to_string()
        }),
        close_action: close_action_name(settings.startup.close_action()).into(),
        start_minimized: settings.startup.start_minimized,
        restore_last_session: settings.startup.restore_last_session,
        keep_window_in_tray: settings.startup.keep_window_in_tray,
        auto_load_after_download: settings.auto_load_after_download,
        analytics_enabled: settings.analytics.enabled,
        analytics_retention_days: settings.analytics.raw_retention_days,
    }
}

fn invalid(field: &str) -> String {
    crate::error_key::err("settings.invalid_value")
        .with("field", field)
        .into()
}

/// Checks the whole change, then writes it into `settings` (nothing is written when one field
/// is invalid).
pub fn apply(settings: &mut AppSettings, patch: &RemoteSettingsPatch) -> Result<(), String> {
    if let Some(lang) = &patch.language {
        if !LANGUAGES.contains(&lang.as_str()) {
            return Err(invalid("language"));
        }
    }
    let theme = match &patch.theme {
        Some(id) => Some(
            THEMES
                .iter()
                .find(|(t, _)| t == id)
                .ok_or_else(|| invalid("theme"))?,
        ),
        None => None,
    };
    if let Some(scale) = patch.ui_scale {
        if !UI_SCALES.contains(&scale) {
            return Err(invalid("ui_scale"));
        }
    }
    let level = match patch.ui_level.as_deref() {
        None => None,
        Some("simple") => Some(UiLevel::Simple),
        Some("advanced") => Some(UiLevel::Advanced),
        Some(_) => return Err(invalid("ui_level")),
    };
    let close = match patch.close_action.as_deref() {
        None => None,
        Some("ask") => Some(CloseAction::Ask),
        Some("tray") => Some(CloseAction::Tray),
        Some("quit") => Some(CloseAction::Quit),
        Some(_) => return Err(invalid("close_action")),
    };
    if let Some(days) = patch.analytics_retention_days {
        if !RETENTION_DAYS.contains(&days) {
            return Err(invalid("analytics_retention_days"));
        }
    }

    if let Some(lang) = &patch.language {
        settings.language = lang.clone();
    }
    if let Some((id, base)) = theme {
        settings.color_theme = id.to_string();
        settings.theme = base.to_string();
    }
    if let Some(v) = patch.cvd_safe {
        settings.cvd_safe = v;
    }
    if patch.ui_scale.is_some() {
        settings.ui_scale = patch.ui_scale;
    }
    if level.is_some() {
        settings.ui_level = level;
    }
    if let Some(action) = close {
        settings.startup.set_close_action(action);
    }
    if let Some(v) = patch.start_minimized {
        settings.startup.start_minimized = v;
    }
    if let Some(v) = patch.restore_last_session {
        settings.startup.restore_last_session = v;
    }
    if let Some(v) = patch.keep_window_in_tray {
        settings.startup.keep_window_in_tray = v;
    }
    if let Some(v) = patch.auto_load_after_download {
        settings.auto_load_after_download = v;
    }
    if let Some(v) = patch.analytics_enabled {
        settings.analytics.enabled = v;
    }
    if let Some(days) = patch.analytics_retention_days {
        settings.analytics.raw_retention_days = days;
    }
    Ok(())
}

/// Loads, changes and saves the settings, then tells the open windows to reload theirs (so a
/// window does not write its older copy back over this change).
pub fn update(patch: &RemoteSettingsPatch) -> Result<RemoteSettings, String> {
    let mut settings = AppSettings::load();
    apply(&mut settings, patch)?;
    settings.save()?;
    crate::changes::settings_changed();
    Ok(read(&settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_writes_only_its_fields_and_maps_the_theme_base() {
        let mut s = AppSettings::default();
        let before = read(&s);
        let patch: RemoteSettingsPatch = serde_json::from_value(serde_json::json!({
            "theme": "oled", "close_action": "tray", "analytics_retention_days": 30
        }))
        .unwrap();
        apply(&mut s, &patch).unwrap();
        assert_eq!((s.theme.as_str(), s.color_theme.as_str()), ("dark", "oled"));
        assert!(s.startup.close_to_tray);
        let after = read(&s);
        assert_eq!(after.theme, "oled");
        assert_eq!(after.close_action, "tray");
        assert_eq!(after.analytics_retention_days, 30);
        assert_eq!(after.language, before.language);
        assert_eq!(after.cvd_safe, before.cvd_safe);
    }

    #[test]
    fn an_invalid_field_writes_nothing() {
        let mut s = AppSettings::default();
        let before = read(&s);
        for bad in [
            serde_json::json!({ "language": "de" }),
            serde_json::json!({ "theme": "pink" }),
            serde_json::json!({ "ui_scale": 120 }),
            serde_json::json!({ "ui_level": "expert" }),
            serde_json::json!({ "close_action": "minimize" }),
            serde_json::json!({ "language": "en", "analytics_retention_days": 0 }),
        ] {
            let patch: RemoteSettingsPatch = serde_json::from_value(bad.clone()).unwrap();
            assert!(apply(&mut s, &patch).is_err(), "{bad}");
            assert_eq!(read(&s), before, "{bad}");
        }
    }

    #[test]
    fn fields_outside_the_allow_list_are_refused() {
        for field in [
            "hf_token",
            "network",
            "gateway",
            "control_api",
            "tools",
            "models_dir",
            "links",
            "server",
        ] {
            let body = serde_json::json!({ field: "x" });
            assert!(
                serde_json::from_value::<RemoteSettingsPatch>(body).is_err(),
                "{field}"
            );
        }
        // The readable view carries none of them either.
        let view = serde_json::to_value(read(&AppSettings::default())).unwrap();
        let keys: Vec<_> = view.as_object().unwrap().keys().cloned().collect();
        for secret in [
            "hf_token",
            "hf_token_set",
            "network",
            "control_api",
            "tools",
        ] {
            assert!(!keys.iter().any(|k| k == secret), "{secret}");
        }
    }
}
